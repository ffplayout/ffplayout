//! Bounded preparation and ownership of the channel's time-varying logo source.
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex, PoisonError, TryLockError,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
    thread::{self, JoinHandle},
    time::SystemTime,
};

use anyhow::{Context, Result, ensure};
use ff_graphics::{
    ffmpeg::source::LogoDecoder,
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
    text_adapter::TextOverlays,
};

const CACHE_BYTES: usize = 64 * 1024 * 1024;
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
    fn open(
        config: LogoConfig,
        width: u32,
        height: u32,
        channel: Option<i32>,
        generation: u64,
        budget: Option<ff_graphics::preparation::ResourceBudget>,
    ) -> Result<Self> {
        let permit = budget
            .as_ref()
            .map(ff_graphics::preparation::ResourceBudget::job)
            .transpose()?;
        let shared = Arc::new(Shared {
            visible: AtomicBool::new(true),
            ..Shared::default()
        });
        let state = shared.clone();
        let worker = thread::Builder::new().name("ffplayout-graphics".into()).spawn(move || {
            let _permit = permit;
            if let Err(error) = prepare(&config, width, height, &state, budget.as_ref())
                && !state.stop.load(Ordering::Relaxed) {
                state.failed.store(true, Ordering::Release);
                warn!(channel = channel.unwrap_or_default(); "Logo source {} failed; retaining its last available surface: {error:#}", config.path);
            }
        }).context("starting graphics preparation worker")?;

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
            self.cached = Some(std::mem::take(&mut *ready));
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

#[derive(PartialEq)]
struct SourceKey {
    config: LogoConfig,
    width: u32,
    height: u32,
    revision: Option<(u64, SystemTime)>,
}

#[derive(Default)]
pub(crate) struct ProgramGraphicsState {
    key: Option<SourceKey>,
    source: Option<LogoSource>,
    generation: u64,
    budget: Option<ff_graphics::preparation::ResourceBudget>,
    scene: ProgramScene,
}

impl ProgramGraphicsState {
    pub(super) fn set_budget(&mut self, budget: ff_graphics::preparation::ResourceBudget) {
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
        let key = logo.map(|logo| SourceKey {
            config: logo.clone(),
            width,
            height,
            revision: std::fs::metadata(&logo.path)
                .and_then(|metadata| Ok((metadata.len(), metadata.modified()?)))
                .ok(),
        });
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
        self.set_visible(opacity > 0.0);
        let selected = self.at(program_us);
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
            .filter(|key| key.config.opacity > 0.0)
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
    budget: Option<&ff_graphics::preparation::ResourceBudget>,
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
        std::fs::metadata(&config.path)
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
    let mut frames = VecDeque::<PreparedFrame>::new();
    let mut bytes = 0_usize;
    let mut duration: Option<LoopDuration> = None;
    let mut loop_end = LoopDuration::from_ticks(1, 1, 1_000_000);
    let mut complete = true;
    let mut end_us = 0;
    let mut eof = false;

    while !shared.stop.load(Ordering::Relaxed) {
        if !complete && !shared.visible.load(Ordering::Relaxed) {
            thread::park();
            continue;
        }
        let requested = shared.requested.load(Ordering::Relaxed);
        let local = duration.map_or(requested, |duration| duration.position_us(requested));
        if !complete {
            if (eof && frames.is_empty())
                || frames
                    .front()
                    .is_some_and(|frame| local < frame.timing.start_us)
            {
                decoder.rewind().context("rewinding animated logo")?;
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
        }
        let capacity = frames.back().is_none_or(|frame| {
            frames.len() < CACHE_FRAMES
                && bytes.saturating_add(frame.surface.0.storage_bytes()) <= CACHE_BYTES
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
            continue;
        }
        let can_decode =
            !eof && (complete || capacity && end_us <= local.saturating_add(LOOKAHEAD_US));
        if can_decode {
            match decoder.next_frame_with_cancel(|| shared.stop.load(Ordering::Relaxed))? {
                Some(mut frame) => {
                    if let Some(budget) = budget {
                        frame.surface = Arc::new(
                            frame
                                .surface
                                .positioned(None, width, height)?
                                .with_budget(budget)?,
                        );
                    }
                    let size = frame.surface.storage_bytes();
                    ensure!(
                        size <= CACHE_BYTES,
                        "one prepared logo frame exceeds the 64 MiB budget; reduce logo scale"
                    );
                    if complete
                        && (bytes.saturating_add(size) > CACHE_BYTES
                            || frames.len() >= CACHE_FRAMES
                            || frame.timing.end_us() > CACHE_TIME_US)
                    {
                        complete = false;
                        decoder.rewind().context("starting bounded logo playback")?;
                        frames.clear();
                        bytes = 0;
                        end_us = 0;
                        continue;
                    }
                    end_us = frame.timing.end_us();
                    loop_end = frame.loop_end;
                    bytes = bytes.saturating_add(size);
                    frames.push_back(PreparedFrame {
                        timing: frame.timing,
                        surface: Arc::new(LogoOverlay(frame.surface)),
                    });
                }
                None => {
                    ensure!(end_us > 0, "logo produced no frames");
                    duration = Some(loop_end);
                    eof = true;
                }
            }
            {
                let mut ready = shared.ready.lock().unwrap_or_else(PoisonError::into_inner);
                // The shared view stores references only. Decoder work, scaling
                // and alpha preparation happen before acquiring this short lock.
                ready.frames.clone_from(&frames);
                ready.duration = duration;
                ready.complete = eof && complete;
            }
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

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use ff_graphics::ffmpeg::source::SourceFrame;

    fn config(name: &str) -> LogoConfig {
        LogoConfig {
            path: std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
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
        config.scale = Some("4096:3072".into());
        let mut source = LogoSource::open(config, 4096, 3072, None, 0, None).unwrap();
        let mut first = None;
        let mut previous = None;
        for time in [0, 100_000, 400_000, 600_000, 700_000] {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if let Some(frame) = source.at(time) {
                    let value = frame.0.frame.data(0)[frame.0.frame.stride(0) * 1536 + 2048];
                    if previous != Some(value) && (time != 600_000 || first == Some(value)) {
                        first.get_or_insert(value);
                        previous = Some(value);
                        break;
                    }
                }
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
