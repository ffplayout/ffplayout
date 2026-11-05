//! Bounded preparation and ownership of the channel's time-varying logo source.
use std::{
    collections::VecDeque,
    fs, mem,
    sync::{
        Arc, Mutex, PoisonError, TryLockError,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime},
};

use anyhow::{Context, Result, ensure};
use ff_graphics::{
    ffmpeg::source::{LogoDecoder, SourceFrame},
    preparation::{PreparationService, ResourceBudget},
    source::{FrameTiming, LoopDuration},
    style::LogoStyle,
};
use log::warn;

use crate::utils::{config::LogoConfig, helper::is_network_url};

use super::{
    clip::ClipSceneFrame,
    control::{LogoUpdate, ProgramScene, ReadyLogo},
    frame::GraphicsFrame,
    logo_adapter::LogoOverlay,
    logo_control::LogoControl,
    manual::ManualFrame,
    text_adapter::TextOverlays,
};

#[cfg(test)]
const CACHE_BYTES: usize = 128 * 1024 * 1024;
const CACHE_FRAMES: usize = 256;
const CACHE_TIME_US: i64 = 5_000_000;
const LOOKAHEAD_US: i64 = 500_000;

#[derive(Clone)]
struct PreparedFrame {
    timing: FrameTiming,
    surface: Arc<LogoOverlay>,
}

#[derive(Default)]
struct Ready {
    frames: VecDeque<PreparedFrame>,
    duration: Option<LoopDuration>,
    complete: bool,
}

#[derive(Default)]
struct Shared {
    ready: Mutex<Ready>,
    requested: AtomicI64,
    stop: AtomicBool,
    visible: AtomicBool,
    failed: AtomicBool,
}

pub(super) struct LogoSource {
    generation: u64,
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
    origin_us: Option<i64>,
    last: Option<Arc<LogoOverlay>>,
    cached: Option<Ready>,
}

impl LogoSource {
    pub(super) fn open(
        config: LogoConfig,
        width: u32,
        height: u32,
        channel: Option<i32>,
        generation: u64,
        budget: Option<ResourceBudget>,
    ) -> Result<Self> {
        let permit = budget.as_ref().map(ResourceBudget::job).transpose()?;
        let shared = Arc::new(Shared {
            visible: AtomicBool::new(true),
            ..Shared::default()
        });
        let state = shared.clone();
        let worker = thread::Builder::new()
            .name("ffplayout-graphics".into())
            .spawn(move || {
                let _permit = permit;

                if let Err(error) = prepare(&config, width, height, &state, budget.as_ref())
                    && !state.stop.load(Ordering::Relaxed)
                {
                    state.failed.store(true, Ordering::Release);
                    warn!(
                        channel = channel.unwrap_or_default();
                        "Logo source {} failed; retaining its last available surface: {error:#}",
                        config.path
                    );
                }
            })
            .context("starting graphics preparation worker")?;

        Ok(Self {
            generation,
            shared,
            worker: Some(worker),
            origin_us: None,
            last: None,
            cached: None,
        })
    }

    fn set_visible(&self, visible: bool) {
        if self.cached.is_some() {
            return;
        }

        if self.shared.visible.swap(visible, Ordering::Relaxed) != visible
            && let Some(worker) = &self.worker
        {
            worker.thread().unpark();
        }
    }

    /// Nonblocking: the video thread consumes only prepared, immutable surfaces.
    pub(super) fn at(&mut self, program_us: i64) -> Option<Arc<LogoOverlay>> {
        let origin = *self.origin_us.get_or_insert(program_us);
        let requested = program_us.saturating_sub(origin).max(0);

        if let Some(cached) = &self.cached {
            return select_frame(cached, requested, &mut self.last);
        }

        if self.shared.requested.swap(requested, Ordering::Relaxed) != requested
            && self.shared.visible.load(Ordering::Relaxed)
            && let Some(worker) = &self.worker
        {
            worker.thread().unpark();
        }

        let mut ready = match self.shared.ready.try_lock() {
            Ok(ready) => ready,
            Err(TryLockError::Poisoned(error)) => error.into_inner(),
            Err(TryLockError::WouldBlock) => return self.last.clone(),
        };
        select_frame(&ready, requested, &mut self.last);

        if ready.complete {
            self.cached = Some(mem::take(&mut *ready));
        }

        self.last.clone()
    }
}

fn select_frame(
    ready: &Ready,
    requested: i64,
    last: &mut Option<Arc<LogoOverlay>>,
) -> Option<Arc<LogoOverlay>> {
    let local = ready
        .duration
        .map_or(requested, |duration| duration.position_us(requested));

    if let Some(frame) = ready
        .frames
        .iter()
        .find(|frame| frame.timing.contains(local))
    {
        *last = Some(frame.surface.clone());
    }

    last.clone()
}

impl Drop for LogoSource {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);

        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            let _ = worker.join();
        }
    }
}

#[derive(Clone, PartialEq)]
pub(super) struct SourceKey {
    config: LogoConfig,
    width: u32,
    height: u32,
    revision: Option<(u64, SystemTime)>,
}

impl SourceKey {
    pub(super) fn new(config: &LogoConfig, width: u32, height: u32) -> Self {
        Self {
            config: config.clone(),
            width,
            height,
            revision: fs::metadata(&config.path)
                .and_then(|metadata| Ok((metadata.len(), metadata.modified()?)))
                .ok(),
        }
    }

    pub(super) fn same_media(&self, other: &Self) -> bool {
        self.config.path == other.config.path
            && self.config.scale == other.config.scale
            && self.config.high_quality == other.config.high_quality
            && self.config.cache_mib == other.config.cache_mib
            && self.width == other.width
            && self.height == other.height
            && self.revision == other.revision
    }
}

#[derive(Default)]
pub(crate) struct ProgramGraphicsState {
    pub(super) manual_frame: Option<Arc<ManualFrame>>,
    key: Option<SourceKey>,
    source: Option<LogoSource>,
    generation: u64,
    budget: Option<ResourceBudget>,
    scene: ProgramScene,
    live_revision: u64,
    live_style: Option<LogoConfig>,
    styled: Option<(Arc<LogoOverlay>, Arc<LogoOverlay>)>,
}

impl ProgramGraphicsState {
    pub(super) fn set_budget(&mut self, budget: ResourceBudget) {
        self.budget = Some(budget);
    }

    pub(super) fn configure(
        &mut self,
        logo: Option<&LogoConfig>,
        width: u32,
        height: u32,
        channel: Option<i32>,
    ) {
        // This is called at clip setup, never while selecting a video frame.
        let key = logo.map(|logo| SourceKey::new(logo, width, height));

        if key == self.key {
            return;
        }
        // Dropping the old source cancels and joins its preparation worker.
        self.source = None;
        self.key = key;
        self.generation = self.generation.saturating_add(1);

        if let Some(logo) = logo.filter(|logo| logo.opacity > 0.0) {
            match LogoSource::open(
                logo.clone(),
                width,
                height,
                channel,
                self.generation,
                self.budget.clone(),
            ) {
                Ok(source) => self.source = Some(source),
                Err(error) => {
                    warn!(channel = channel.unwrap_or_default(); "Logo preparation unavailable: {error:#}");
                }
            }
        }
    }

    pub(super) fn setup_live(
        &mut self,
        control: &LogoControl,
        config: Option<&LogoConfig>,
        canvas: (u32, u32),
        channel: Option<i32>,
        preparation: &PreparationService,
    ) {
        if self.live_revision == 0 {
            self.configure(config, canvas.0, canvas.1, channel);

            if let Some(mut state) = control.try_state() {
                state.active = self.source.as_ref().and(self.key.clone());
            }
        }

        control.refresh(canvas, channel, preparation.clone());
    }

    pub(super) fn update_live(&mut self, control: &LogoControl, now: i64) {
        let Some(mut shared) = control.try_state() else {
            return;
        };

        if shared
            .pending
            .as_ref()
            .is_some_and(|change| change.revision != shared.revision)
        {
            return;
        }

        if shared.retired.is_some() {
            return;
        }

        let Some(change) = shared.pending.as_mut() else {
            return;
        };

        if let Some(source) = change.source.as_mut()
            && source.at(now).is_none()
        {
            if source.shared.failed.load(Ordering::Acquire) {
                let mut failed = shared.pending.take().expect("pending logo was inspected");
                shared.retired = failed.source.take();
                control.wake();
            }
            return;
        }

        let Some(mut change) = shared.pending.take() else {
            return;
        };

        if change.source.is_some() || change.config.is_none() {
            if let Some(old) = self.source.take() {
                old.shared.stop.store(true, Ordering::Release);
                shared.retired = Some(old);
                control.wake();
            }

            self.generation = self.generation.saturating_add(1);

            if let Some(source) = change.source.as_mut() {
                source.generation = self.generation;
            }

            self.source = change.source.take();
        }

        self.key = change.key;
        self.live_style = change.config;
        self.live_revision = change.revision;
        self.styled = None;
        shared.active = self.key.clone();
    }

    pub(super) fn set_visible(&self, visible: bool) {
        if let Some(source) = &self.source {
            source.set_visible(visible);
        }
    }

    #[cfg(test)]
    pub(super) fn has_source(&self) -> bool {
        self.source.is_some()
    }

    pub(super) fn at(&mut self, program_us: i64) -> Option<Arc<LogoOverlay>> {
        self.source
            .as_mut()
            .and_then(|source| source.at(program_us))
    }

    pub(super) fn frame(
        &mut self,
        program_us: i64,
        canvas: (u32, u32),
        opacity: f64,
        texts: TextOverlays,
        clip: Option<ClipSceneFrame>,
    ) -> GraphicsFrame {
        self.set_visible(
            opacity > 0.0
                && self
                    .live_style
                    .as_ref()
                    .is_none_or(|style| style.opacity > 0.0),
        );
        let selected = self.at(program_us);
        let selected =
            if let (Some(source), Some(style)) = (selected.as_ref(), self.live_style.as_ref()) {
                if !self
                    .styled
                    .as_ref()
                    .is_some_and(|(previous, _)| Arc::ptr_eq(previous, source))
                    && let Ok(styled) = source.restyled(style, canvas.0, canvas.1)
                {
                    self.styled = Some((source.clone(), Arc::new(styled)));
                }

                self.styled.as_ref().map(|(_, styled)| styled.clone())
            } else {
                selected
            };
        let logo = self
            .source
            .as_ref()
            .zip(selected)
            .map(|(source, surface)| ReadyLogo {
                generation: source.generation,
                surface,
            });
        // An enabled request still exists if its worker could not be spawned.
        // Treat that as a failed replacement, not as explicit logo removal.
        let generation = self
            .key
            .as_ref()
            .filter(|key| key.config.opacity > 0.0 || self.live_revision > 0)
            .map(|_| self.generation);
        let failed = generation.is_some()
            && self
                .source
                .as_ref()
                .is_none_or(|source| source.shared.failed.load(Ordering::Acquire));

        self.scene.frame(
            program_us,
            canvas,
            LogoUpdate {
                generation,
                ready: logo,
                failed,
                opacity,
            },
            texts,
            clip,
        )
    }

    pub(super) fn end_occurrence(&mut self) {
        self.scene.end_occurrence();
    }
}

fn prepare(
    config: &LogoConfig,
    width: u32,
    height: u32,
    shared: &Arc<Shared>,
    budget: Option<&ResourceBudget>,
) -> Result<()> {
    // Persistent network streams have no finite loop or bounded shutdown.
    // Logo assets are local files; their decoder is opened only on this worker.
    ensure!(
        !is_network_url(&config.path),
        "animated logo sources must be local files"
    );
    let style = LogoStyle {
        scale: config.scale.clone(),
        opacity: config.opacity,
        position: config.position.clone(),
    };
    ensure!(
        fs::metadata(&config.path)
            .context("reading logo metadata")?
            .is_file(),
        "logo source must be a regular file"
    );
    let cancelled = shared.clone();
    let input = ffmpeg_next::format::input_with_interrupt(&config.path, move || {
        cancelled.stop.load(Ordering::Relaxed)
    })
    .context("opening logo asset")?;
    let mut decoder = LogoDecoder::new(input, style, width, height)?;
    decoder.set_high_quality(config.high_quality);
    let cache_bytes = config.cache_mib.clamp(32, 1024) as usize * 1024 * 1024;
    let mut frames = VecDeque::<PreparedFrame>::new();
    let mut bytes = 0_usize;
    let mut duration: Option<LoopDuration> = None;
    let mut loop_end = LoopDuration::from_ticks(1, 1, 1_000_000);
    let mut complete = true;
    let mut end_us = 0;
    let mut eof = false;
    let mut pending: Option<SourceFrame> = None;

    while !shared.stop.load(Ordering::Relaxed) {
        if !complete && !shared.visible.load(Ordering::Relaxed) {
            thread::park();
            continue;
        }

        let requested = shared.requested.load(Ordering::Relaxed);
        let local = duration.map_or(requested, |duration| duration.position_us(requested));

        if !complete {
            let previous_len = frames.len();

            if (eof && frames.is_empty())
                || frames
                    .front()
                    .is_some_and(|frame| local < frame.timing.start_us)
            {
                decoder.rewind().context("rewinding animated logo")?;
                pending = None;
                frames.clear();
                bytes = 0;
                end_us = 0;
                eof = false;
            }

            while frames.len() > 1
                && frames
                    .get(1)
                    .is_some_and(|frame| frame.timing.start_us <= local)
            {
                if let Some(old) = frames.pop_front() {
                    bytes = bytes.saturating_sub(old.surface.0.storage_bytes());
                }
            }

            if frames.len() != previous_len {
                publish_ready(shared, &frames, duration, false);
            }
        }

        let capacity = frames.back().is_none_or(|frame| {
            frames.len() < CACHE_FRAMES
                && bytes.saturating_add(frame.surface.0.storage_bytes()) <= cache_bytes
        });

        if !complete
            && !capacity
            && frames
                .front()
                .is_some_and(|frame| frame.timing.end_us() <= local)
        {
            if let Some(old) = frames.pop_front() {
                bytes = bytes.saturating_sub(old.surface.0.storage_bytes());
            }

            publish_ready(shared, &frames, duration, false);

            continue;
        }

        let can_decode =
            !eof && (complete || capacity && end_us <= local.saturating_add(LOOKAHEAD_US));

        if can_decode {
            let next = match pending.take() {
                Some(frame) => Some(frame),
                None => decoder.next_frame_with_cancel(|| shared.stop.load(Ordering::Relaxed))?,
            };

            match next {
                Some(mut frame) => {
                    let size = frame.surface.storage_bytes();
                    ensure!(
                        size <= cache_bytes,
                        "one prepared logo frame exceeds the animation cache budget; reduce logo scale"
                    );
                    let reserved = match budget {
                        Some(budget) => frame
                            .surface
                            .positioned(None, width, height)
                            .and_then(|surface| surface.with_budget(budget))
                            .map(Arc::new),
                        None => Ok(frame.surface.clone()),
                    };

                    if complete
                        && (bytes.saturating_add(size) > cache_bytes
                            || frames.len() >= CACHE_FRAMES
                            || frame.timing.end_us() > CACHE_TIME_US
                            || (!frames.is_empty() && reserved.is_err()))
                    {
                        drop(reserved);
                        complete = false;
                        decoder.rewind().context("starting bounded logo playback")?;
                        frames.clear();
                        bytes = 0;
                        end_us = 0;
                        // Drop the shared cache references too, before retrying
                        // admission. Program/output snapshots retain their leases.
                        publish_ready(shared, &frames, duration, false);
                        continue;
                    }

                    match reserved {
                        Ok(surface) => frame.surface = surface,
                        Err(_) if !complete => {
                            // Retain just this decoded frame. Output snapshots or
                            // other graphics may temporarily own the remaining budget.
                            pending = Some(frame);
                            thread::park_timeout(Duration::from_millis(25));
                            continue;
                        }
                        Err(error) => return Err(error),
                    }

                    end_us = frame.timing.end_us();
                    loop_end = frame.loop_end;
                    bytes = bytes.saturating_add(size);
                    frames.push_back(PreparedFrame {
                        timing: frame.timing,
                        surface: Arc::new(LogoOverlay(frame.surface, None)),
                    });
                }
                None => {
                    ensure!(end_us > 0, "logo produced no frames");
                    duration = Some(loop_end);
                    eof = true;
                }
            }
            publish_ready(shared, &frames, duration, eof && complete);

            if eof && complete {
                // Static assets and short cached loops retain no decoder/thread.
                return Ok(());
            }

            continue;
        }

        if !shared.stop.load(Ordering::Relaxed) {
            thread::park();
        }
    }

    Ok(())
}

/// Publish only immutable references, releasing evicted cache leases on this
/// preparation worker rather than leaving stale references in the shared view.
fn publish_ready(
    shared: &Shared,
    frames: &VecDeque<PreparedFrame>,
    duration: Option<LoopDuration>,
    complete: bool,
) {
    let mut ready = shared.ready.lock().unwrap_or_else(PoisonError::into_inner);
    ready.frames.clone_from(frames);
    ready.duration = duration;
    ready.complete = complete;
}

#[cfg(test)]
mod tests {
    use std::{
        path::Path,
        time::{Duration, Instant},
    };

    use ff_graphics::ffmpeg::source::SourceFrame;

    use super::*;

    fn config(name: &str) -> LogoConfig {
        LogoConfig {
            high_quality: true,
            cache_mib: 128,
            path: Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests_assets/storage/media_images")
                .join(name)
                .to_string_lossy()
                .into_owned(),
            scale: None,
            opacity: 1.0,
            position: "0:0".into(),
        }
    }

    fn wait(source: &mut LogoSource, time: i64, expected: &[u8]) -> Arc<LogoOverlay> {
        let deadline = Instant::now() + Duration::from_secs(5);

        loop {
            if let Some(frame) = source.at(time)
                && frame.0.frame.data(0) == expected
            {
                return frame;
            }

            assert!(
                Instant::now() < deadline,
                "requested logo frame at {time} was not prepared"
            );
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn reference(name: &str) -> Vec<SourceFrame> {
        let config = config(name);
        let input = ffmpeg_next::format::input(&config.path).unwrap();
        let mut decoder = LogoDecoder::new(
            input,
            LogoStyle {
                scale: None,
                opacity: 1.0,
                position: "0:0".into(),
            },
            64,
            64,
        )
        .unwrap();
        let mut frames = Vec::new();

        while let Some(frame) = decoder.next_frame().unwrap() {
            frames.push(frame);
        }

        frames
    }

    fn pump_live(
        state: &mut ProgramGraphicsState,
        control: &LogoControl,
        time: i64,
    ) -> GraphicsFrame {
        state.update_live(control, time);
        state.frame(time, (64, 64), 1.0, Default::default(), None)
    }

    fn apply_live(
        state: &mut ProgramGraphicsState,
        control: &LogoControl,
        time: i64,
        previous: u64,
    ) -> GraphicsFrame {
        let deadline = Instant::now() + Duration::from_secs(5);

        loop {
            let frame = pump_live(state, control, time);

            if state.live_revision > previous {
                return frame;
            }

            assert!(
                Instant::now() < deadline,
                "live logo update did not reach a frame boundary"
            );
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn channel_budget_pressure_switches_logo_to_streaming_and_retries_held_snapshots() {
        let limit = 256 * 1024 * 1024;
        let budget = ResourceBudget::new(limit, 1024, 16);
        let other_graphics = budget.reserve(252 * 1024 * 1024).unwrap();
        let mut config = config("animated-logo-video.mkv");
        config.cache_mib = 128;
        config.high_quality = false;
        config.scale = Some("512:512".into());
        let mut source = LogoSource::open(config, 512, 512, None, 0, Some(budget.clone())).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);

        // Fill the available channel quota without reaching the per-logo limit.
        let held = loop {
            source.at(0);
            assert!(!source.shared.failed.load(Ordering::Acquire));
            assert!(Instant::now() < deadline);
            let ready = source.shared.ready.lock().unwrap();

            if ready.frames.len() == 5 {
                assert!(!ready.complete);
                break ready.frames.clone();
            }

            drop(ready);
            thread::sleep(Duration::from_millis(1));
        };
        source.at(2_000_000);
        thread::sleep(Duration::from_millis(75));
        assert!(!source.shared.failed.load(Ordering::Acquire));
        assert!(!source.worker.as_ref().unwrap().is_finished());
        assert!(budget.usage().0 <= limit);
        // Releasing output snapshots must unblock admission without recreating
        // the source or requesting a different timestamp.
        drop(held);

        for time in [2_000_000, 2_400_000, 3_000_000] {
            let deadline = Instant::now() + Duration::from_secs(5);

            loop {
                source.at(time);
                assert!(!source.shared.failed.load(Ordering::Acquire));
                assert!(Instant::now() < deadline, "logo stalled at {time}");
                let ready = source.shared.ready.lock().unwrap();

                if ready
                    .frames
                    .iter()
                    .any(|frame| frame.timing.start_us <= time && time < frame.timing.end_us())
                {
                    break;
                }

                drop(ready);
                thread::sleep(Duration::from_millis(1));
            }

            assert!(budget.usage().0 <= limit);
        }

        drop(source);
        drop(other_graphics);
        assert_eq!(budget.usage(), (0, 0, 0));
    }

    #[test]
    fn cache_or_quality_changes_require_new_pixels_but_layout_changes_reuse_them() {
        let original = config("animated-logo.gif");
        let key = SourceKey::new(&original, 64, 64);
        let mut updated = original.clone();
        updated.opacity = 0.5;
        updated.position = "10:10".into();
        assert!(key.same_media(&SourceKey::new(&updated, 64, 64)));
        updated.high_quality = false;
        assert!(!key.same_media(&SourceKey::new(&updated, 64, 64)));
        updated = original;
        updated.cache_mib = 32;
        assert!(!key.same_media(&SourceKey::new(&updated, 64, 64)));
    }

    #[test]
    fn stopped_logo_edits_override_the_previous_live_setting_on_restart() {
        let preparation = PreparationService::default();
        let control = LogoControl::default();

        for enabled in [false, true] {
            assert!(preparation.resume());
            let original = config("animated-logo.gif");
            let mut state = ProgramGraphicsState::default();
            state.setup_live(&control, Some(&original), (64, 64), None, &preparation);
            let mut live = original.clone();
            live.opacity = 0.25;
            control
                .request(Some(live), (64, 64), None, preparation.clone())
                .unwrap();
            drop(apply_live(&mut state, &control, 0, 0));
            control.stop();
            drop(state);
            assert!(preparation.shutdown(Duration::from_secs(5)));
            assert!(preparation.resume());
            let mut saved = original;
            saved.opacity = 0.75;
            let saved = enabled.then_some(saved);
            let mut restarted = ProgramGraphicsState::default();
            restarted.setup_live(&control, saved.as_ref(), (64, 64), None, &preparation);
            let no_stale_update = control.try_state().unwrap().idle();

            if enabled {
                let expected = reference("animated-logo.gif");
                wait(
                    restarted.source.as_mut().unwrap(),
                    0,
                    expected[0].surface.frame.data(0),
                );
            }

            let frame = pump_live(&mut restarted, &control, 0);
            let opacity = frame.logo().map(|(surface, _)| surface.opacity());
            control.stop();
            drop((frame, restarted));
            assert!(preparation.shutdown(Duration::from_secs(5)));
            assert_eq!(preparation.budget().usage(), (0, 0, 0));
            assert!(
                no_stale_update,
                "a previous run queued an obsolete logo update"
            );
            assert_eq!(opacity, enabled.then_some(191));
        }
    }

    #[test]
    fn rapid_logo_updates_and_service_restarts_keep_only_the_latest_request() {
        let preparation = PreparationService::default();
        let control = LogoControl::default();

        for _ in 0..5 {
            assert!(preparation.resume());
            let original = config("animated-logo.gif");
            let mut state = ProgramGraphicsState::default();
            state.setup_live(&control, Some(&original), (64, 64), None, &preparation);

            for index in 1..10 {
                let mut next = original.clone();
                next.opacity = f64::from(index) / 10.0;
                control
                    .request(Some(next), (64, 64), None, preparation.clone())
                    .unwrap();
            }

            let deadline = Instant::now() + Duration::from_secs(5);

            loop {
                let frame = pump_live(&mut state, &control, 100_000);

                if frame
                    .logo()
                    .is_some_and(|(surface, _)| surface.opacity() == 230)
                {
                    break;
                }

                assert!(
                    Instant::now() < deadline,
                    "latest update was lost after restart"
                );
                thread::sleep(Duration::from_millis(1));
            }

            control.stop();
            drop(state);
            assert!(preparation.shutdown(Duration::from_secs(5)));
            assert_eq!(preparation.budget().usage(), (0, 0, 0));
        }
    }

    #[test]
    fn live_logo_style_preserves_animation_clock_destination_position_and_clip_boundaries() {
        let preparation = PreparationService::default();
        let control = LogoControl::default();
        let original = config("animated-logo.gif");
        let mut state = ProgramGraphicsState::default();
        state.setup_live(&control, Some(&original), (64, 64), None, &preparation);
        let expected = reference("animated-logo.gif");
        wait(
            state.source.as_mut().unwrap(),
            0,
            expected[0].surface.frame.data(0),
        );
        let source = state.source.as_ref().unwrap().shared.clone();
        let origin = state.source.as_ref().unwrap().origin_us;
        let mut updated = original.clone();
        updated.opacity = 0.25;
        updated.position = "W-w-4:2".into();
        control
            .request(Some(updated.clone()), (64, 64), None, preparation.clone())
            .unwrap();
        let frame = apply_live(&mut state, &control, 100_000, 0);
        let (surface, _) = frame.logo().unwrap();
        assert_eq!(surface.opacity(), 64);
        assert_eq!(
            surface.positioned(Some("0:0"), 128, 64).unwrap().position(),
            (92, 2)
        );
        assert!(Arc::ptr_eq(&source, &state.source.as_ref().unwrap().shared));
        assert_eq!(origin, state.source.as_ref().unwrap().origin_us);
        state.setup_live(&control, Some(&original), (64, 64), None, &preparation);
        let next = pump_live(&mut state, &control, 200_000);
        assert_eq!(next.logo().unwrap().0.opacity(), 64);
        let previous = state.live_revision;
        updated.opacity = 0.0;
        control
            .request(Some(updated.clone()), (64, 64), None, preparation.clone())
            .unwrap();
        assert!(
            apply_live(&mut state, &control, 300_000, previous)
                .logo()
                .is_none()
        );
        let previous = state.live_revision;
        updated.opacity = 0.8;
        control
            .request(Some(updated), (64, 64), None, preparation.clone())
            .unwrap();
        assert_eq!(
            apply_live(&mut state, &control, 400_000, previous)
                .logo()
                .unwrap()
                .0
                .opacity(),
            204
        );
        assert!(Arc::ptr_eq(&source, &state.source.as_ref().unwrap().shared));
        let previous = state.live_revision;
        control
            .request(None, (64, 64), None, preparation.clone())
            .unwrap();
        assert!(
            apply_live(&mut state, &control, 500_000, previous)
                .logo()
                .is_none()
        );
        state.setup_live(&control, Some(&original), (64, 64), None, &preparation);
        assert!(pump_live(&mut state, &control, 600_000).logo().is_none());
        control.stop();
        drop((frame, next, state));
        assert!(preparation.shutdown(Duration::from_secs(5)));
        assert_eq!(preparation.budget().usage(), (0, 0, 0));
    }

    #[test]
    fn live_logo_replacements_keep_queued_pixels_and_failures_keep_the_active_decoder() {
        let preparation = PreparationService::default();
        let control = LogoControl::default();
        let original = config("animated-logo.gif");
        let mut state = ProgramGraphicsState::default();
        state.setup_live(&control, Some(&original), (64, 64), None, &preparation);
        let expected = reference("animated-logo.gif");
        wait(
            state.source.as_mut().unwrap(),
            0,
            expected[0].surface.frame.data(0),
        );
        let queued = pump_live(&mut state, &control, 0);
        let old_source = state.source.as_ref().unwrap().shared.clone();
        let mut scaled = original.clone();
        scaled.scale = Some("16:16".into());
        control
            .request(Some(scaled), (64, 64), None, preparation.clone())
            .unwrap();
        let changed = apply_live(&mut state, &control, 40_000, 0);
        assert_eq!(changed.logo().unwrap().0.width(), 16);
        assert_eq!(queued.logo().unwrap().0.width(), 32);
        assert!(old_source.stop.load(Ordering::Acquire));
        let active = state.source.as_ref().unwrap().shared.clone();
        let previous = state.live_revision;
        control
            .request(
                Some(config("missing.gif")),
                (64, 64),
                None,
                preparation.clone(),
            )
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);

        loop {
            pump_live(&mut state, &control, 80_000);
            let done = control.try_state().is_some_and(|shared| shared.idle());

            if active.stop.load(Ordering::Acquire) || Instant::now() >= deadline {
                panic!("failed replacement damaged the active source");
            }

            if done && preparation.budget().usage().2 == 1 {
                break;
            }

            thread::sleep(Duration::from_millis(1));
        }

        assert_eq!(state.live_revision, previous);
        assert!(Arc::ptr_eq(&active, &state.source.as_ref().unwrap().shared));
        control.stop();
        drop((queued, changed, state));
        assert!(preparation.shutdown(Duration::from_secs(5)));
        assert_eq!(preparation.budget().usage(), (0, 0, 0));
    }

    #[test]
    fn cached_animation_selects_by_time_and_finishes_its_worker() {
        let frames = reference("animated-logo.gif");
        let mut source =
            LogoSource::open(config("animated-logo.gif"), 64, 64, None, 0, None).unwrap();
        wait(&mut source, 0, frames[0].surface.frame.data(0));
        let deadline = Instant::now() + Duration::from_secs(5);

        while !source.worker.as_ref().unwrap().is_finished() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }

        for fps in [24_i64, 25, 30, 50, 60] {
            for n in 0..fps * 3 {
                let time = n * 1_000_000 / fps;
                let local = time % 600_000;
                let expected = frames
                    .iter()
                    .find(|frame| frame.timing.contains(local))
                    .unwrap();
                wait(&mut source, time, expected.surface.frame.data(0));
            }
        }

        let surface = source.at(0).unwrap();
        let same_surface = source.at(600_000 * 100_000).unwrap();
        assert!(
            Arc::ptr_eq(&surface, &same_surface),
            "loops must reuse the prepared surface"
        );
        let shared = source.shared.clone();
        let locked = shared.ready.lock().unwrap();
        assert!(
            Arc::ptr_eq(&source.at(0).unwrap(), &same_surface),
            "cached loops must not acquire the preparation lock"
        );
        drop(locked);
    }

    #[test]
    fn long_video_streams_with_bounded_cache_and_repeats_after_eof() {
        let frames = reference("animated-logo-video.mkv");
        let mut source =
            LogoSource::open(config("animated-logo-video.mkv"), 64, 64, None, 0, None).unwrap();
        wait(&mut source, 0, frames[0].surface.frame.data(0));
        source.set_visible(false);
        source.at(7_900_000);
        thread::sleep(Duration::from_millis(20));
        assert!(
            source
                .shared
                .ready
                .lock()
                .unwrap()
                .frames
                .back()
                .is_none_or(|frame| frame.timing.end_us() <= CACHE_TIME_US)
        );
        source.set_visible(true);

        for time in [
            100_000, 1_000_000, 4_900_000, 7_900_000, 8_100_000, 15_900_000, 16_000_000,
        ] {
            let expected = &frames[(time % 8_000_000 / 100_000) as usize];
            wait(&mut source, time, expected.surface.frame.data(0));
            let ready = source.shared.ready.lock().unwrap();
            assert!(ready.frames.len() <= CACHE_FRAMES);
            assert!(
                ready
                    .frames
                    .iter()
                    .map(|frame| frame.surface.0.storage_bytes())
                    .sum::<usize>()
                    <= CACHE_BYTES
            );
        }

        assert!(!source.worker.as_ref().unwrap().is_finished());
        let started = Instant::now();
        drop(source);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "shutdown must wake and join the decoder"
        );
    }

    #[test]
    fn single_frame_cache_rewinds_after_evicting_the_last_frame() {
        let mut config = config("animated-logo.gif");
        config.cache_mib = 64; // Keep the one-frame boundary independent of the default.
        config.scale = Some("4096:3072".into());
        let mut source = LogoSource::open(config, 4096, 3072, None, 0, None).unwrap();
        let mut first = None;
        let mut previous = None;

        for time in [0, 100_000, 400_000, 600_000, 700_000] {
            // This cache-boundary regression intentionally prepares 12-megapixel
            // frames. Debug conversion is much slower than release preparation;
            // throughput is measured separately by the graphics benchmark.
            let deadline = Instant::now() + Duration::from_secs(30);

            loop {
                if let Some(frame) = source.at(time) {
                    let value = frame.0.frame.data(0)[frame.0.frame.stride(0) * 1536 + 2048];

                    if previous != Some(value) && (time != 600_000 || first == Some(value)) {
                        first.get_or_insert(value);
                        previous = Some(value);
                        break;
                    }
                }

                assert!(
                    !source.shared.failed.load(Ordering::Acquire),
                    "logo preparation failed at {time}"
                );
                assert!(Instant::now() < deadline, "logo froze at {time}");
                thread::sleep(Duration::from_millis(1));
            }

            let ready = source.shared.ready.lock().unwrap();
            assert!(ready.frames.len() <= 1);
            assert!(
                ready
                    .frames
                    .iter()
                    .map(|f| f.surface.0.storage_bytes())
                    .sum::<usize>()
                    <= CACHE_BYTES
            );
        }
    }

    #[test]
    fn missing_or_invalid_assets_fail_once_without_blocking_or_reactivation() {
        let mut state = ProgramGraphicsState::default();
        let missing = config("missing-animation.gif");
        state.configure(Some(&missing), 64, 64, None);
        let deadline = Instant::now() + Duration::from_secs(5);

        while !state
            .source
            .as_ref()
            .unwrap()
            .worker
            .as_ref()
            .unwrap()
            .is_finished()
        {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }

        for time in [0, 100_000, 2_000_000] {
            assert!(state.at(time).is_none());
        }

        let same_resource = state.source.as_ref().unwrap().shared.clone();
        state.configure(Some(&missing), 64, 64, None);
        assert!(Arc::ptr_eq(
            &same_resource,
            &state.source.as_ref().unwrap().shared
        ));
        state.configure(None, 64, 64, None);
        assert!(state.at(0).is_none());
    }

    #[test]
    fn failed_worker_replacement_keeps_good_pixels_until_the_logo_is_cleared() {
        let mut state = ProgramGraphicsState::default();
        let config = config("animated-logo.gif");
        state.configure(Some(&config), 64, 64, None);
        let deadline = Instant::now() + Duration::from_secs(5);
        let queued = loop {
            let snapshot = state.frame(0, (64, 64), 1.0, Default::default(), None);

            if !snapshot.is_empty() {
                break snapshot;
            }

            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        };
        let weak = Arc::downgrade(queued.logo().unwrap().0);
        let mut missing = config;
        missing.path.push_str(".missing");
        state.configure(Some(&missing), 64, 64, None);
        let deadline = Instant::now() + Duration::from_secs(5);

        while !state
            .source
            .as_ref()
            .unwrap()
            .shared
            .failed
            .load(Ordering::Acquire)
        {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }

        let retained = state.frame(1, (64, 64), 1.0, Default::default(), None);
        assert!(Arc::ptr_eq(
            queued.logo().unwrap().0,
            retained.logo().unwrap().0
        ));
        state.configure(None, 64, 64, None);
        assert!(
            state
                .frame(2, (64, 64), 1.0, Default::default(), None)
                .is_empty()
        );
        drop(retained);
        assert!(weak.upgrade().is_some());
        drop(queued);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn static_source_is_cached_and_replacement_retires_old_resources() {
        let mut state = ProgramGraphicsState::default();
        let static_config = config("lower-third.png");
        state.configure(Some(&static_config), 1280, 720, None);
        let deadline = Instant::now() + Duration::from_secs(5);

        while state.at(0).is_none()
            || !state
                .source
                .as_ref()
                .unwrap()
                .worker
                .as_ref()
                .unwrap()
                .is_finished()
        {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }

        let old = Arc::downgrade(state.source.as_ref().unwrap().last.as_ref().unwrap());
        state.configure(None, 1280, 720, None);
        assert!(old.upgrade().is_none());
        assert!(!state.has_source());
        state.configure(Some(&config("animated-logo.gif")), 64, 64, None);
        state.configure(None, 64, 64, None);
        assert!(!state.has_source());
    }
}
