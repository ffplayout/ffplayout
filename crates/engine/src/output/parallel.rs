//! One program clock and bounded, independent destination workers.
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex, PoisonError, Weak},
    thread::{self, JoinHandle},
    time::Duration,
};

use anyhow::{Result, ensure};
use ffmpeg_next::{frame, rescale};

use crate::{
    HlsSubtitle, HlsVariant, OutputConfig,
    analysis::{audio_level::AudioLevelMeter, loudness::LoudnessMeter},
    clock::{PlayoutClock, SharedPlayoutClock},
    compositor::logo::LogoOverlay,
    utils::{
        ffmpeg::reference_audio_frame,
        pipeline_queue::{PipelineActivity, PipelineControl, PipelineQueue, QueueLimits},
    },
};

use super::{
    composition::{ProgramVideo, VideoCompositor},
    encoded::EncodedFormat,
};

#[derive(Debug, Clone)]
pub struct OutputState {
    pub phase: &'static str,
    pub error: Option<String>,
}

#[derive(Clone, Default)]
pub struct OutputStatus(Arc<Mutex<OutputRegistry>>, Option<u64>);

impl std::fmt::Debug for OutputStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OutputStatus").finish_non_exhaustive()
    }
}

#[derive(Debug, Default)]
struct OutputRegistry {
    states: BTreeMap<i32, OutputState>,
    reconnectable: BTreeSet<i32>,
    retry: BTreeSet<i32>,
    retry_wake: BTreeMap<i32, Weak<PipelineActivity>>,
    generations: BTreeMap<i32, u64>,
    delivery_locks: BTreeMap<i32, Weak<Mutex<()>>>,
    next_generation: u64,
    dynamic: bool,
    update: Option<Vec<OutputDestination>>,
}

impl OutputStatus {
    pub fn supports_live_updates(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .dynamic
    }

    /// Replace the pending update; media threads consume only the latest snapshot.
    pub fn update_outputs(&self, destinations: Vec<OutputDestination>) -> Result<()> {
        ensure!(
            destinations.len() <= 8,
            "at most eight outputs are supported"
        );
        super::shared::groups(&destinations)?;
        let mut registry = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        ensure!(
            registry.dynamic,
            "output runtime does not support live changes"
        );
        registry.update = Some(destinations);

        Ok(())
    }

    fn take_update(&self) -> Option<Vec<OutputDestination>> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .update
            .take()
    }

    pub(super) fn delivery_lock(&self, id: i32) -> Arc<Mutex<()>> {
        let mut registry = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        registry
            .delivery_locks
            .retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = registry.delivery_locks.get(&id).and_then(Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(Mutex::new(()));
        registry.delivery_locks.insert(id, Arc::downgrade(&lock));

        lock
    }

    pub(super) fn scoped(&self, id: i32) -> Self {
        let mut registry = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        registry.next_generation += 1;
        let generation = registry.next_generation;
        registry.generations.insert(id, generation);
        registry.retry.remove(&id);
        registry.retry_wake.remove(&id);

        Self(self.0.clone(), Some(generation))
    }

    fn current(&self, registry: &OutputRegistry, id: i32) -> bool {
        self.1
            .is_none_or(|generation| registry.generations.get(&id) == Some(&generation))
    }

    pub fn snapshot(&self) -> BTreeMap<i32, OutputState> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .states
            .clone()
    }

    pub fn clear(&self) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = OutputRegistry::default();
    }

    /// Retry only a live destination worker, using its existing configuration.
    pub fn request_retry(&self, id: i32) -> bool {
        let mut registry = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if !registry.reconnectable.contains(&id)
            || !registry
                .states
                .get(&id)
                .is_some_and(|state| state.phase == "failed")
        {
            return false;
        }

        if !registry.retry.insert(id) {
            return false;
        }

        if let Some(state) = registry.states.get_mut(&id) {
            state.phase = "starting";
        }
        if let Some(activity) = registry.retry_wake.get(&id).and_then(Weak::upgrade) {
            activity.notify();
        }

        true
    }

    pub fn can_retry(&self, id: i32) -> bool {
        let registry = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        registry.reconnectable.contains(&id)
            && registry
                .states
                .get(&id)
                .is_some_and(|state| state.phase == "failed")
    }

    pub(super) fn reconnectable(&self, id: i32, available: bool) {
        let mut registry = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if !self.current(&registry, id) {
            return;
        }

        if available {
            registry.reconnectable.insert(id);
        } else {
            registry.reconnectable.remove(&id);
            registry.retry.remove(&id);
            registry.retry_wake.remove(&id);
        }
    }

    pub(super) fn take_retry(&self, id: i32) -> bool {
        let mut registry = self.0.lock().unwrap_or_else(PoisonError::into_inner);

        self.current(&registry, id) && registry.retry.remove(&id)
    }

    pub(super) fn observe_retry(&self, id: i32, activity: &Arc<PipelineActivity>) {
        let mut registry = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if self.current(&registry, id) {
            registry.retry_wake.insert(id, Arc::downgrade(activity));
        }
    }

    pub fn fail(&self, id: i32, error: impl std::fmt::Display) {
        self.set(id, "failed", Some(error.to_string()));
    }

    pub(super) fn set(&self, id: i32, phase: &'static str, error: Option<String>) {
        let mut registry = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if self.current(&registry, id) {
            registry.states.insert(id, OutputState { phase, error });
        }
    }
}

/// Resolves or recreates an application-owned transport on each connection attempt.
/// Called only from the destination's blocking delivery worker.
pub trait OutputEndpoint: std::fmt::Debug + Send + Sync {
    fn connect_url(&self) -> Result<String>;
    fn disconnect(&self) {}
    /// First accepted encoded audio PTS of this connection, before muxer time shifts.
    fn audio_started(&self, _timestamp_us: i64) {}
}

#[derive(Debug, Clone)]
pub enum OutputTarget {
    Stream {
        url: String,
    },
    Hls {
        playlist: String,
        variants: Vec<HlsVariant>,
        subtitle: Option<HlsSubtitle>,
        segment_seconds: u32,
        list_size: u32,
    },
}

#[derive(Debug, Clone)]
pub struct OutputDestination {
    pub id: i32,
    pub config: OutputConfig,
    pub target: OutputTarget,
}

impl OutputDestination {
    pub(super) fn writes_subtitles(&self) -> bool {
        self.config.delivery_enabled
            && matches!(
                self.target,
                OutputTarget::Hls {
                    subtitle: Some(_),
                    ..
                }
            )
    }
}

pub(super) enum Message {
    Video(Arc<ProgramVideo>),
    Audio(frame::Audio),
    Rate(f64),
    Destinations(Vec<OutputDestination>),
    Subtitles(String, i64, i64),
    ClearSubtitles,
    AdvanceSubtitles(i64),
}

struct DestinationWorker {
    encoding_revision: String,
    gop: u32,
    subtitles: bool,
    id: i32,
    ids: Vec<i32>,
    video_resolutions: Vec<(u32, u32)>,
    queue: Arc<PipelineQueue<Message>>,
    control: Arc<PipelineControl>,
    worker: Option<JoinHandle<()>>,
    channel: i32,
    status: OutputStatus,
}

impl DestinationWorker {
    fn start_group(
        mut destinations: Vec<OutputDestination>,
        program: &OutputConfig,
    ) -> Result<Self> {
        for destination in &mut destinations {
            ensure!(
                destination.config.fps == program.fps,
                "parallel outputs must use the program frame rate"
            );
            ensure!(
                destination.config.sample_rate == program.sample_rate,
                "parallel outputs must use the program sample rate"
            );
            destination.config.input_size = Some((program.width, program.height));
            destination.config.destinations.clear();
            destination.config.audio_effects = crate::AudioEffectsControl::default();
            destination.config.audio_level_callback = None;
            destination.config.audio_frame_callback = None;
            destination.config.loudness_meter_control = crate::LoudnessMeterControl::default();
            destination.config.output_status = program.output_status.clone();
        }
        let subtitles = destinations.iter().any(OutputDestination::writes_subtitles);
        let encoding_revision = destinations[0].config.encoding_revision.clone();
        let gop = super::shared::gop_size(&destinations)?;
        let ids = destinations
            .iter()
            .map(|destination| destination.id)
            .collect::<Vec<_>>();
        let worker_ids = ids.clone();
        let root = &destinations[0];
        let mut video_resolutions = match &root.target {
            OutputTarget::Hls { variants, .. } if !variants.is_empty() => variants
                .iter()
                .map(|variant| (variant.width, variant.height))
                .collect(),
            _ => vec![(root.config.width, root.config.height)],
        };
        if destinations.iter().any(|destination| {
            destination
                .config
                .recording
                .as_ref()
                .is_some_and(|recording| recording.encode.is_some())
        }) {
            video_resolutions.push((program.width, program.height));
        }
        let mut program = program.clone();
        program.destinations.clear();
        let control = Arc::new(PipelineControl::default());
        let queue = PipelineQueue::new(
            QueueLimits {
                items: 128,
                bytes: 64 * 1024 * 1024,
                duration_us: 1_000_000,
            },
            control.clone(),
        );
        let worker_queue = queue.clone();
        let worker_control = control.clone();
        let id = ids[0];
        let channel = program.channel_id.unwrap_or_default();
        let status = program.output_status.clone();
        for destination in &destinations {
            status.set(
                destination.id,
                if destination.config.delivery_enabled {
                    "starting"
                } else {
                    "stopped"
                },
                None,
            );
        }
        let worker_status = status.clone();
        let worker = thread::Builder::new()
            .name(format!("playout-output-{id}"))
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    super::shared::run(destinations, &program, &worker_queue, &worker_control)
                }))
                .unwrap_or_else(|_| Err(anyhow::anyhow!("output worker panicked")));

                if let Err(error) = result {
                    if !worker_control.cancelled() {
                        log::warn!(channel = channel; "Output {id} stopped: {error:#}");
                    }
                    if !worker_control.cancelled() {
                        for id in &worker_ids {
                            worker_status.set(*id, "failed", Some(format!("{error:#}")));
                        }
                    }
                    worker_control.fail(error);
                }
            })?;

        Ok(Self {
            encoding_revision,
            gop,
            subtitles,
            id,
            ids,
            video_resolutions,
            queue,
            control,
            worker: Some(worker),
            channel,
            status,
        })
    }

    fn send(&self, message: Message, bytes: usize, timestamp: Option<i64>) {
        if self.control.cancelled() {
            return;
        }

        match self.queue.try_send(message, bytes, timestamp) {
            Ok(None) => {}
            result => {
                let reason = match result {
                    Ok(Some(_)) => "destination queue exceeded its bounded capacity".to_string(),
                    Err(error) => error.to_string(),
                    _ => unreachable!(),
                };
                log::warn!(channel = self.channel; "Output {} disabled: {reason}", self.id);
                // Stop the entire branch instead of dropping arbitrary packets or
                // continuing a damaged GOP. Other destinations keep playing.
                for id in &self.ids {
                    self.status.set(*id, "failed", Some(reason.clone()));
                }
                self.control.fail(reason);
                self.queue.close();
            }
        }
    }

    fn finish(&mut self) {
        self.queue.close();

        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for DestinationWorker {
    fn drop(&mut self) {
        self.control.cancel();
        self.finish();
    }
}

#[derive(Default)]
struct OutputContext {
    rate: Option<f64>,
    subtitles: Option<(String, i64, i64)>,
    position: Option<i64>,
}

impl OutputContext {
    fn restore(&self, worker: &DestinationWorker) {
        worker.send(Message::Rate(self.rate.unwrap_or(1.0)), 0, None);
        if let Some((path, start, source)) = &self.subtitles {
            let position = self.position.unwrap_or(*start).max(*start);
            let source = source.saturating_add(position.saturating_sub(*start));
            worker.send(
                Message::Subtitles(path.clone(), position, source),
                path.len(),
                None,
            );
        }
    }
}

pub(crate) struct ParallelOutput {
    workers: Mutex<Vec<DestinationWorker>>,
    config: OutputConfig,
    context: Mutex<OutputContext>,
    retired: Mutex<Vec<JoinHandle<()>>>,
    clock: SharedPlayoutClock,
    video_time_base: ffmpeg_next::Rational,
    audio_time_base: ffmpeg_next::Rational,
    paced: bool,
    meters: Mutex<(AudioLevelMeter, LoudnessMeter)>,
    compositor: Arc<Mutex<VideoCompositor>>,
}

impl ParallelOutput {
    pub(crate) fn new(config: &OutputConfig) -> Result<Self> {
        Self::start(config, true)
    }

    pub(crate) fn start(config: &OutputConfig, paced: bool) -> Result<Self> {
        ensure!(
            config.destinations.len() <= 8,
            "at most eight outputs are supported"
        );
        let workers = super::shared::groups(&config.destinations)?
            .into_iter()
            .map(|destinations| DestinationWorker::start_group(destinations, config))
            .collect::<Result<Vec<_>>>()?;

        config
            .output_status
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .dynamic = true;
        let mut program = config.clone();
        program.destinations.clear();

        Ok(Self {
            workers: Mutex::new(workers),
            config: program,
            context: Mutex::new(OutputContext::default()),
            retired: Mutex::new(Vec::new()),
            clock: Arc::new(Mutex::new(PlayoutClock::new())),
            video_time_base: config.video_time_base,
            audio_time_base: config.audio_time_base,
            paced,
            meters: Mutex::new((
                AudioLevelMeter::new(config.sample_rate, config.audio_level_callback.clone()),
                LoudnessMeter::new(config.sample_rate, config.loudness_meter_control.clone()),
            )),
            compositor: Arc::new(Mutex::new(VideoCompositor::new(
                config.logo.as_ref().map(|logo| logo.position.clone()),
            ))),
        })
    }

    fn reconcile(&self, workers: &mut Vec<DestinationWorker>) -> Result<()> {
        self.retired
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|worker| !worker.is_finished());
        let Some(destinations) = self.config.output_status.take_update() else {
            return Ok(());
        };
        self.compositor
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        let ids: BTreeSet<_> = destinations
            .iter()
            .filter(|destination| destination.config.delivery_enabled)
            .map(|destination| destination.id)
            .collect();
        let groups = super::shared::groups(&destinations)?;
        let mut previous = std::mem::take(workers);
        for group in groups {
            let root = &group[0];
            let gop = super::shared::gop_size(&group)?;
            let reusable = previous.iter().position(|worker| {
                worker.id == root.id
                    && worker.encoding_revision == root.config.encoding_revision
                    && gop.is_multiple_of(worker.gop.max(1))
                    && !worker.control.cancelled()
            });
            if let Some(index) = reusable {
                let mut worker = previous.remove(index);
                worker.ids = group.iter().map(|destination| destination.id).collect();
                worker.subtitles = group.iter().any(OutputDestination::writes_subtitles);
                worker.send(Message::Destinations(group), 0, None);
                workers.push(worker);
            } else {
                let worker = DestinationWorker::start_group(group, &self.config)?;
                self.context
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .restore(&worker);
                workers.push(worker);
            }
        }
        for mut worker in previous {
            for id in &worker.ids {
                if !ids.contains(id) {
                    self.config
                        .output_status
                        .scoped(*id)
                        .set(*id, "stopped", None);
                }
            }
            worker.control.cancel();
            worker.queue.close();
            // I/O shutdown must never stall the common program clock.
            let reaper = thread::spawn(move || worker.finish());
            let mut retired = self.retired.lock().unwrap_or_else(PoisonError::into_inner);
            retired.retain(|worker| !worker.is_finished());
            retired.push(reaper);
        }

        Ok(())
    }

    pub(crate) fn clock(&self) -> SharedPlayoutClock {
        self.clock.clone()
    }

    fn pace(&self, timestamp: Option<i64>, time_base: ffmpeg_next::Rational) {
        if !self.paced {
            return;
        }
        let delay = self
            .clock
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .delay_until(timestamp, time_base);
        // Retain a small encode lead without giving each destination control
        // over the shared source. AVIO and encoder stalls stay in their worker.
        let delay = delay.saturating_sub(Duration::from_millis(100));
        thread::sleep(delay);
    }

    pub(crate) fn encode_video(&self, frame: &frame::Video) -> Result<()> {
        self.video_with_logo(frame, None, 0.0)
    }

    pub(crate) fn video_with_logo(
        &self,
        frame: &frame::Video,
        logo: Option<Arc<LogoOverlay>>,
        opacity: f64,
    ) -> Result<()> {
        self.pace(frame.pts(), self.video_time_base);
        let bytes = (0..frame.planes())
            .map(|plane| frame.data(plane).len())
            .sum();
        let timestamp = frame.pts().map(|pts| {
            ffmpeg_next::Rescale::rescale(&pts, self.video_time_base, rescale::TIME_BASE)
        });

        let mut workers = self.workers.lock().unwrap_or_else(PoisonError::into_inner);
        self.reconcile(&mut workers)?;
        ensure!(
            workers.is_empty() || workers.iter().any(|worker| !worker.control.cancelled()),
            "all output encoding groups have failed"
        );
        let mut consumers = BTreeMap::new();
        if logo.is_some() && opacity > 0.0 {
            for resolution in workers
                .iter()
                .filter(|worker| !worker.control.cancelled())
                .flat_map(|worker| &worker.video_resolutions)
            {
                *consumers.entry(*resolution).or_insert(0) += 1;
            }
        }
        let prepared = ProgramVideo::new(frame, self.compositor.clone(), logo, opacity, consumers)?;
        for worker in workers.iter() {
            worker.send(Message::Video(prepared.clone()), bytes, timestamp);
        }

        Ok(())
    }

    pub(crate) fn encode_audio(&self, frame: &frame::Audio) -> Result<()> {
        self.pace(frame.pts(), self.audio_time_base);
        if self.paced {
            let mut meters = self.meters.lock().unwrap_or_else(PoisonError::into_inner);
            meters.0.process_frame(frame);
            meters.1.process_frame(frame);
        }
        let bytes = frame.samples().saturating_mul(8);
        let timestamp = frame.pts().map(|pts| {
            ffmpeg_next::Rescale::rescale(&pts, self.audio_time_base, rescale::TIME_BASE)
        });

        let mut workers = self.workers.lock().unwrap_or_else(PoisonError::into_inner);
        self.reconcile(&mut workers)?;
        for worker in workers.iter() {
            worker.send(
                Message::Audio(reference_audio_frame(frame)?),
                bytes,
                timestamp,
            );
        }

        Ok(())
    }

    pub(crate) fn encode_processed_audio(&self, frame: &frame::Audio) -> Result<()> {
        self.encode_audio(frame)
    }

    pub(crate) fn set_playout_rate(&self, rate: f64) {
        self.context
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .rate = Some(rate);
        self.clock
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .set_rate(rate);

        let mut workers = self.workers.lock().unwrap_or_else(PoisonError::into_inner);
        if let Err(error) = self.reconcile(&mut workers) {
            log::error!("Output update failed: {error:#}");
        }
        for worker in workers.iter() {
            worker.send(Message::Rate(rate), 0, None);
        }
    }

    pub(crate) fn write_vtt_subtitles(&self, path: &str, start: i64, source: i64) -> Result<()> {
        {
            let mut context = self.context.lock().unwrap_or_else(PoisonError::into_inner);
            context.subtitles = Some((path.to_string(), start, source));
            context.position = Some(start);
        }
        let mut workers = self.workers.lock().unwrap_or_else(PoisonError::into_inner);
        self.reconcile(&mut workers)?;
        for worker in workers.iter() {
            worker.send(
                Message::Subtitles(path.to_string(), start, source),
                path.len(),
                None,
            );
        }

        Ok(())
    }

    pub(crate) fn clear_vtt_subtitles(&self) -> Result<()> {
        {
            let mut context = self.context.lock().unwrap_or_else(PoisonError::into_inner);
            context.subtitles = None;
            context.position = None;
        }
        let mut workers = self.workers.lock().unwrap_or_else(PoisonError::into_inner);
        self.reconcile(&mut workers)?;
        for worker in workers.iter() {
            worker.send(Message::ClearSubtitles, 0, None);
        }

        Ok(())
    }

    pub(crate) fn advance_vtt_subtitles(&self, position: i64) -> Result<()> {
        let active = {
            let mut context = self.context.lock().unwrap_or_else(PoisonError::into_inner);
            context.position = Some(position);
            context.subtitles.is_some()
        };
        let mut workers = self.workers.lock().unwrap_or_else(PoisonError::into_inner);
        self.reconcile(&mut workers)?;
        if active {
            for worker in workers.iter().filter(|worker| worker.subtitles) {
                worker.send(Message::AdvanceSubtitles(position), 0, None);
            }
        }

        Ok(())
    }

    pub(crate) fn finish(mut self) -> Result<()> {
        let workers = self
            .workers
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner);
        for worker in workers.iter() {
            worker.queue.close();
        }
        for worker in workers.iter_mut() {
            worker.finish();
        }

        Ok(())
    }
}

impl Drop for ParallelOutput {
    fn drop(&mut self) {
        let mut registry = self
            .config
            .output_status
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        registry.dynamic = false;
        registry.update = None;
        drop(registry);
        for worker in self
            .retired
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .drain(..)
        {
            let _ = worker.join();
        }
    }
}

pub(super) fn destination_mux(destination: &OutputDestination) -> Result<(String, EncodedFormat)> {
    let format = match &destination.target {
        OutputTarget::Stream { .. } => EncodedFormat::Stream {
            muxer: destination
                .config
                .stream_type
                .muxer(&destination.config.stream_format)
                .to_string(),
        },
        OutputTarget::Hls {
            variants,
            subtitle,
            segment_seconds,
            list_size,
            ..
        } => EncodedFormat::Hls {
            variants: variants.clone(),
            subtitle: subtitle.clone(),
            segment_seconds: *segment_seconds,
            list_size: *list_size,
        },
    };
    let path = match &destination.target {
        OutputTarget::Stream { url } => match &destination.config.stream_endpoint {
            Some(endpoint) => endpoint.connect_url()?,
            None => url.clone(),
        },
        OutputTarget::Hls { playlist, .. } => playlist.clone(),
    };
    Ok((path, format))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ffmpeg_next::{
        ChannelLayout,
        format::{Sample, pixel::Pixel, sample::Type},
    };

    struct TestDirectory(std::path::PathBuf);
    impl TestDirectory {
        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn subtitle_progress_only_reaches_vtt_destinations_during_an_active_clip() {
        let config = OutputConfig::new(160, 90, 25, 48_000);
        let output = ParallelOutput::start(&config, false).unwrap();
        for (id, subtitles) in [(1, false), (2, true)] {
            let control = Arc::new(PipelineControl::default());
            output.workers.lock().unwrap().push(DestinationWorker {
                encoding_revision: String::new(),
                gop: 50,
                subtitles,
                id,
                ids: vec![id],
                video_resolutions: Vec::new(),
                queue: PipelineQueue::new(
                    QueueLimits {
                        items: 8,
                        bytes: 1024,
                        duration_us: 1_000_000,
                    },
                    control.clone(),
                ),
                control,
                worker: None,
                channel: 1,
                status: config.output_status.clone(),
            });
        }
        output.context.lock().unwrap().subtitles = Some(("clip.mp4".into(), 0, 0));
        output.advance_vtt_subtitles(500).unwrap();
        {
            let workers = output.workers.lock().unwrap();
            assert!(workers[0].queue.try_recv().unwrap().is_none());
            assert!(matches!(
                workers[1].queue.try_recv().unwrap(),
                Some(Message::AdvanceSubtitles(500))
            ));
        }
        output.clear_vtt_subtitles().unwrap();
        for worker in output.workers.lock().unwrap().iter() {
            assert!(matches!(
                worker.queue.try_recv().unwrap(),
                Some(Message::ClearSubtitles)
            ));
        }
        output.advance_vtt_subtitles(1_000).unwrap();
        for worker in output.workers.lock().unwrap().iter() {
            assert!(worker.queue.try_recv().unwrap().is_none());
        }
        assert_eq!(output.context.lock().unwrap().position, Some(1_000));
    }

    #[test]
    fn live_destinations_keep_shared_encoder_and_unrelated_stream_continuous() {
        ffmpeg_next::init().unwrap();
        let directory = TestDirectory(std::env::temp_dir().join(format!(
                "live-output-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )));
        std::fs::create_dir_all(directory.path()).unwrap();
        let mut config = OutputConfig::new(160, 90, 25, 48_000);
        config.stream_type = crate::StreamType::Custom;
        config.stream_format = "mpegts".into();
        config
            .video_options
            .insert("preset".into(), "ultrafast".into());
        config
            .video_options
            .insert("tune".into(), "zerolatency".into());
        let destination = |id, name: &str, source| {
            let mut settings = config.clone();
            settings.encoding_source = source;
            settings.encoding_revision = format!("owner-{}", source.map_or(id, |(owner, _)| owner));
            settings.destination_revision = name.into();
            OutputDestination {
                id,
                config: settings,
                target: OutputTarget::Stream {
                    url: directory.path().join(name).to_string_lossy().into_owned(),
                },
            }
        };
        let root = destination(1, "owner.ts", None);
        let healthy = destination(5, "healthy.ts", None);
        let first = destination(2, "shared-first.ts", Some((1, 0)));
        let second = destination(2, "shared-second.ts", Some((1, 0)));
        config.destinations = vec![root.clone(), healthy.clone()];
        let output = ParallelOutput::new(&config).unwrap();
        let owner_thread = output
            .workers
            .lock()
            .unwrap()
            .iter()
            .find(|worker| worker.id == 1)
            .unwrap()
            .worker
            .as_ref()
            .unwrap()
            .thread()
            .id();
        for index in 0..200 {
            let update = match index {
                20 => Some(vec![root.clone(), first.clone(), healthy.clone()]),
                80 => Some(vec![root.clone(), healthy.clone()]),
                100 => Some(vec![root.clone(), second.clone(), healthy.clone()]),
                150 => {
                    let mut disabled = root.clone();
                    disabled.config.delivery_enabled = false;
                    Some(vec![disabled, second.clone(), healthy.clone()])
                }
                175 => Some(vec![healthy.clone()]),
                180 => {
                    let mut resized = root.clone();
                    resized.config.width = 80;
                    resized.config.height = 46;
                    resized.config.encoding_revision = "resized-owner".into();
                    resized.config.destination_revision = "owner-resized".into();
                    resized.target = OutputTarget::Stream {
                        url: directory
                            .path()
                            .join("owner-resized.ts")
                            .to_string_lossy()
                            .into_owned(),
                    };
                    Some(vec![resized, healthy.clone()])
                }
                _ => None,
            };
            if let Some(update) = update {
                config.output_status.update_outputs(update).unwrap();
            }
            let mut video = frame::Video::new(Pixel::YUV420P, 160, 90);
            for plane in 0..video.planes() {
                video.data_mut(plane).fill(128);
            }
            video.set_pts(Some(index));
            output.encode_video(&video).unwrap();
            if matches!(index, 25 | 85 | 110 | 160) {
                let workers = output.workers.lock().unwrap();
                let worker = workers.iter().find(|worker| worker.id == 1).unwrap();
                assert_eq!(
                    worker.worker.as_ref().unwrap().thread().id(),
                    owner_thread,
                    "changing a shared delivery restarted the encoder"
                );
            }
            let mut audio =
                frame::Audio::new(Sample::F32(Type::Planar), 1920, ChannelLayout::STEREO);
            audio.set_rate(48_000);
            audio.set_pts(Some(index * 1920));
            for channel in 0..2 {
                audio.plane_mut::<f32>(channel).fill(0.0);
            }
            output.encode_audio(&audio).unwrap();
        }
        output.finish().unwrap();
        let mut input = ffmpeg_next::format::input(&directory.path().join("healthy.ts")).unwrap();
        let video = input
            .streams()
            .best(ffmpeg_next::media::Type::Video)
            .unwrap()
            .index();
        let pts: Vec<_> = input
            .packets()
            .filter(|(stream, _)| stream.index() == video)
            .map(|(_, packet)| packet.pts().unwrap())
            .collect();
        assert_eq!(
            pts.len(),
            200,
            "unrelated destination lost video during live changes"
        );
        for pair in pts.windows(2) {
            assert_eq!(pair[1] - pair[0], 3600);
        }
        for name in ["shared-first.ts", "shared-second.ts"] {
            let mut input = ffmpeg_next::format::input(&directory.path().join(name)).unwrap();
            assert!(
                input
                    .packets()
                    .any(|(stream, _)| stream.parameters().medium()
                        == ffmpeg_next::media::Type::Video),
                "shared output did not resume from a keyframe"
            );
        }
        assert_eq!(config.output_status.snapshot()[&1].phase, "stopped");
        assert!(!config.output_status.supports_live_updates());
    }

    #[test]
    fn an_idle_program_can_enable_its_first_encoding_group() {
        ffmpeg_next::init().unwrap();
        let mut config = OutputConfig::new(160, 90, 25, 48_000);
        config.stream_type = crate::StreamType::Custom;
        config.stream_format = "null".into();
        config
            .video_options
            .insert("preset".into(), "ultrafast".into());
        let output = ParallelOutput::start(&config, false).unwrap();
        assert!(output.workers.lock().unwrap().is_empty());
        let mut video = frame::Video::new(Pixel::YUV420P, 160, 90);
        for plane in 0..video.planes() {
            video.data_mut(plane).fill(128);
        }
        video.set_pts(Some(100));
        output.encode_video(&video).unwrap();
        config
            .output_status
            .update_outputs(vec![OutputDestination {
                id: 1,
                config: config.clone(),
                target: OutputTarget::Stream {
                    url: if cfg!(windows) { "NUL" } else { "/dev/null" }.into(),
                },
            }])
            .unwrap();
        video.set_pts(Some(101));
        output.encode_video(&video).unwrap();
        assert_eq!(output.workers.lock().unwrap().len(), 1);
        config.output_status.update_outputs(Vec::new()).unwrap();
        video.set_pts(Some(102));
        output.encode_video(&video).unwrap();
        assert!(output.workers.lock().unwrap().is_empty());
        output.finish().unwrap();
    }

    #[test]
    fn replaced_delivery_cannot_overwrite_new_runtime_status() {
        let status = OutputStatus::default();
        let old = status.scoped(7);
        old.set(7, "running", None);
        let current = status.scoped(7);
        current.set(7, "starting", None);
        old.set(7, "failed", Some("old connection".into()));
        assert_eq!(status.snapshot()[&7].phase, "starting");
        current.reconnectable(7, true);
        current.set(7, "failed", None);
        old.reconnectable(7, false);
        assert!(status.can_retry(7));
    }

    #[test]
    fn parallel_streams_and_hls_keep_media_when_another_destination_fails() {
        ffmpeg_next::init().unwrap();
        let directory = TestDirectory(std::env::temp_dir().join(format!(
                "parallel-output-test-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )));
        std::fs::create_dir(directory.path()).unwrap();
        let mut config = OutputConfig::new(320, 240, 25, 48_000);
        config.stream_type = crate::StreamType::Custom;
        config.stream_format = "mpegts".into();
        config
            .video_options
            .insert("preset".into(), "ultrafast".into());
        config
            .video_options
            .insert("tune".into(), "zerolatency".into());
        for (id, name) in [
            (3, "missing/fails.ts"),
            (1, "first.ts"),
            (2, "second.ts"),
            (5, "shared.ts"),
        ] {
            let mut settings = config.clone();
            if id != 1 && id != 2 {
                settings.encoding_source = Some((1, 0));
            }
            if id == 2 {
                settings.width = 160;
                settings.height = 120;
            }
            config.destinations.push(OutputDestination {
                id,
                config: settings,
                target: OutputTarget::Stream {
                    url: directory.path().join(name).to_string_lossy().into_owned(),
                },
            });
        }
        let mut settings = config.clone();
        settings.destinations.clear();
        settings.encoding_source = Some((1, 0));
        config.destinations.push(OutputDestination {
            id: 4,
            config: settings,
            target: OutputTarget::Hls {
                playlist: directory
                    .path()
                    .join("stream.m3u8")
                    .to_string_lossy()
                    .into_owned(),
                variants: Vec::new(),
                subtitle: None,
                segment_seconds: 1,
                list_size: 6,
            },
        });
        let mut flv = config
            .destinations
            .iter()
            .find(|destination| destination.id == 1)
            .unwrap()
            .clone();
        flv.id = 6;
        flv.config.encoding_source = Some((1, 0));
        flv.config.recording = Some(crate::RecordingConfig::new(
            directory
                .path()
                .join("recordings")
                .to_string_lossy()
                .into_owned(),
            30,
        ));
        flv.config.stream_type = crate::StreamType::Rtmp;
        flv.target = OutputTarget::Stream {
            url: directory
                .path()
                .join("shared.flv")
                .to_string_lossy()
                .into_owned(),
        };
        config.destinations.push(flv);
        let output = ParallelOutput::new(&config).unwrap();
        assert_eq!(
            output.workers.lock().unwrap().len(),
            2,
            "selected destinations reuse their owner; independent encoding stays separate"
        );
        for index in 0..25 {
            let mut video = frame::Video::new(Pixel::YUV420P, 320, 240);
            for plane in 0..video.planes() {
                video
                    .data_mut(plane)
                    .fill(if plane == 0 { 32 } else { 128 });
            }
            video.set_pts(Some(index));
            output.encode_video(&video).unwrap();
            let mut audio =
                frame::Audio::new(Sample::F32(Type::Planar), 1920, ChannelLayout::STEREO);
            audio.set_rate(48_000);
            audio.set_pts(Some(index * 1920));
            for channel in 0..2 {
                audio.plane_mut::<f32>(channel).fill(0.1);
            }
            output.encode_processed_audio(&audio).unwrap();
        }
        output.finish().unwrap();
        let status = config.output_status.snapshot();
        assert_eq!(status[&1].phase, "stopped");
        assert_eq!(status[&2].phase, "stopped");
        assert_eq!(status[&3].phase, "failed");
        assert_eq!(status[&4].phase, "stopped");
        assert_eq!(status[&5].phase, "stopped");
        assert_eq!(status[&6].phase, "stopped");
        for (name, size) in [
            ("first.ts", (320, 240)),
            ("second.ts", (160, 120)),
            ("shared.ts", (320, 240)),
            ("shared.flv", (320, 240)),
        ] {
            let mut input = ffmpeg_next::format::input(&directory.path().join(name)).unwrap();
            let video = input
                .streams()
                .best(ffmpeg_next::media::Type::Video)
                .unwrap();
            let index = video.index();
            let decoder = ffmpeg_next::codec::context::Context::from_parameters(video.parameters())
                .unwrap()
                .decoder()
                .video()
                .unwrap();
            assert_eq!((decoder.width(), decoder.height()), size);
            let mut videos = 0;
            let mut audios = 0;
            for (stream, _) in input.packets() {
                if stream.index() == index {
                    videos += 1;
                } else {
                    audios += 1;
                }
            }
            assert_eq!(videos, 25, "{name}");
            assert!(audios >= 46, "{name}: {audios}");
        }
        let payloads = |name: &str| {
            let mut input = ffmpeg_next::format::input(&directory.path().join(name)).unwrap();
            input
                .packets()
                .map(|(stream, packet)| {
                    (
                        stream.index(),
                        packet.pts(),
                        packet.dts(),
                        packet.data().unwrap_or_default().to_vec(),
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            payloads("first.ts"),
            payloads("shared.ts"),
            "shared destinations receive the same encoded media and timestamps"
        );
        let recording = std::fs::read_dir(directory.path().join("recordings"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.extension().is_some_and(|extension| extension == "mkv"))
            .unwrap();
        let mut recording_input = ffmpeg_next::format::input(&recording).unwrap();
        let recording_video = recording_input
            .streams()
            .best(ffmpeg_next::media::Type::Video)
            .unwrap()
            .index();
        assert_eq!(
            recording_input
                .packets()
                .filter(|(stream, _)| stream.index() == recording_video)
                .count(),
            25,
            "recording copies the shared encoder"
        );
        let playlist = std::fs::read_to_string(directory.path().join("stream.m3u8")).unwrap();
        assert!(playlist.contains("#EXTINF:"));
        assert!(directory.path().join("stream_0.ts").is_file());
        // Exercise the public playout API with actual demux/decode and the
        // same program feeding several outputs across repeated clip boundaries.
        let mut replay = config.clone();
        for destination in &mut replay.destinations {
            match &mut destination.target {
                OutputTarget::Stream { url } if destination.id != 3 => {
                    *url = directory
                        .path()
                        .join(format!("replay-{}.ts", destination.id))
                        .to_string_lossy()
                        .into_owned();
                }
                OutputTarget::Hls { playlist, .. } => {
                    *playlist = directory
                        .path()
                        .join("replay.m3u8")
                        .to_string_lossy()
                        .into_owned();
                }
                _ => {}
            }
        }
        let mut playout = crate::Playout::open_parallel(replay, 1.0).unwrap();
        let source = directory.path().join("first.ts");
        playout.play(source.to_str().unwrap()).unwrap();
        playout.play(source.to_str().unwrap()).unwrap();
        playout.finish().unwrap();
        for id in [1, 2, 5, 6] {
            let mut input =
                ffmpeg_next::format::input(&directory.path().join(format!("replay-{id}.ts")))
                    .unwrap();
            let index = input
                .streams()
                .best(ffmpeg_next::media::Type::Video)
                .unwrap()
                .index();
            let count = input
                .packets()
                .filter(|(stream, _)| stream.index() == index)
                .count();
            assert!(
                count >= 50,
                "output {id} lost video across clip boundaries: {count}"
            );
        }
    }

    #[test]
    fn the_program_continues_when_every_destination_is_unavailable() {
        ffmpeg_next::init().unwrap();
        let directory = TestDirectory(
            std::env::temp_dir().join(format!("all-outputs-unavailable-{}", std::process::id())),
        );
        std::fs::create_dir_all(directory.path()).unwrap();
        let mut config = OutputConfig::new(160, 90, 25, 48_000);
        config.stream_type = crate::StreamType::Custom;
        config.stream_format = "mpegts".into();
        config
            .video_options
            .insert("preset".into(), "ultrafast".into());
        config.destinations.push(OutputDestination {
            id: 1,
            config: config.clone(),
            target: OutputTarget::Stream {
                url: directory
                    .path()
                    .join("missing/output.ts")
                    .to_string_lossy()
                    .into_owned(),
            },
        });
        let output = ParallelOutput::start(&config, false).unwrap();
        for index in 0..20 {
            let mut video = frame::Video::new(Pixel::YUV420P, 160, 90);
            for plane in 0..video.planes() {
                video.data_mut(plane).fill(128);
            }
            video.set_pts(Some(index));
            output.encode_video(&video).unwrap();
            let mut audio =
                frame::Audio::new(Sample::F32(Type::Planar), 1920, ChannelLayout::STEREO);
            audio.set_rate(48_000);
            audio.set_pts(Some(index * 1920));
            for channel in 0..2 {
                audio.plane_mut::<f32>(channel).fill(0.0);
            }
            output.encode_audio(&audio).unwrap();
        }
        output.finish().unwrap();
        assert_eq!(config.output_status.snapshot()[&1].phase, "failed");
    }

    #[test]
    fn shared_encoding_reconnects_one_transport_without_interrupting_another() {
        use std::{
            io::Read,
            net::{Shutdown, TcpListener},
            time::Instant,
        };

        ffmpeg_next::init().unwrap();
        let directory = TestDirectory(std::env::temp_dir().join(format!(
                "parallel-disconnect-test-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )));
        std::fs::create_dir(directory.path()).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let peer = thread::spawn(move || {
            let mut recovered = Vec::new();
            for round in 0..2 {
                let deadline = Instant::now() + Duration::from_secs(20);
                let mut socket = loop {
                    if let Ok((socket, _)) = listener.accept() {
                        break socket;
                    }
                    assert!(Instant::now() < deadline, "output never reconnected");
                    thread::sleep(Duration::from_millis(5));
                };
                socket.set_nonblocking(false).unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                if round == 0 {
                    let mut buffer = [0; 512];
                    assert!(socket.read(&mut buffer).unwrap() > 0);
                    socket.shutdown(Shutdown::Both).unwrap();
                } else {
                    socket.read_to_end(&mut recovered).unwrap();
                }
            }
            recovered
        });
        let mut config = OutputConfig::new(160, 120, 25, 48_000);
        config.stream_type = crate::StreamType::Custom;
        config.stream_format = "mpegts".into();
        config
            .video_options
            .insert("preset".into(), "ultrafast".into());
        for (id, url) in [
            (1, format!("tcp://{address}")),
            (
                2,
                directory
                    .path()
                    .join("healthy.ts")
                    .to_string_lossy()
                    .into_owned(),
            ),
        ] {
            config.destinations.push(OutputDestination {
                id,
                config: config.clone(),
                target: OutputTarget::Stream { url },
            });
        }
        config.destinations[1].config.encoding_source = Some((config.destinations[0].id, 0));
        let recording_path = directory.path().join("recording");
        config.destinations[0].config.recording = Some(crate::RecordingConfig::new(
            recording_path.to_string_lossy(),
            60,
        ));
        let output = ParallelOutput::new(&config).unwrap();
        assert_eq!(output.workers.lock().unwrap().len(), 1);
        let mut noise = 7_u32;

        for index in 0..450 {
            let mut video = frame::Video::new(Pixel::YUV420P, 160, 120);
            for plane in 0..video.planes() {
                for byte in video.data_mut(plane) {
                    noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    *byte = (noise >> 24) as u8;
                }
            }
            video.set_pts(Some(index));
            output.encode_video(&video).unwrap();
            let mut audio =
                frame::Audio::new(Sample::F32(Type::Planar), 1920, ChannelLayout::STEREO);
            audio.set_rate(48_000);
            audio.set_pts(Some(index * 1920));
            for channel in 0..2 {
                audio.plane_mut::<f32>(channel).fill(0.1);
            }
            output.encode_audio(&audio).unwrap();
        }
        output.finish().unwrap();
        let recovered = peer.join().unwrap();
        assert!(
            recovered.len() > 188,
            "the failed destination never resumed media"
        );
        let recovered_path = directory.path().join("recovered.ts");
        std::fs::write(&recovered_path, recovered).unwrap();
        let mut recovered_input = ffmpeg_next::format::input(&recovered_path).unwrap();
        let first = recovered_input
            .packets()
            .find(|(stream, _)| stream.parameters().medium() == ffmpeg_next::media::Type::Video)
            .unwrap()
            .1;
        assert!(
            first.is_key(),
            "reconnected output must start at a keyframe"
        );
        assert_eq!(config.output_status.snapshot()[&1].phase, "stopped");
        assert_eq!(config.output_status.snapshot()[&2].phase, "stopped");
        let mut input = ffmpeg_next::format::input(&directory.path().join("healthy.ts")).unwrap();
        let video = input
            .streams()
            .best(ffmpeg_next::media::Type::Video)
            .unwrap()
            .index();
        let timestamps = input
            .packets()
            .filter(|(stream, _)| stream.index() == video)
            .map(|(_, packet)| packet.pts().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(timestamps.len(), 450);
        assert!(
            timestamps.windows(2).all(|pair| pair[1] - pair[0] == 3600),
            "healthy output lost its continuous 25 FPS timeline"
        );
        let files = std::fs::read_dir(&recording_path)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(files.len(), 1, "reconnect must not reopen recording");
        let mut recorded = ffmpeg_next::format::input(&files[0]).unwrap();
        let timestamps = recorded
            .packets()
            .filter(|(stream, _)| stream.parameters().medium() == ffmpeg_next::media::Type::Video)
            .map(|(_, packet)| packet.pts().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            timestamps.len(),
            450,
            "recording lost media during the outage"
        );
        assert!(timestamps.windows(2).all(|pair| pair[1] - pair[0] == 40));
    }

    #[test]
    fn hls_variants_feed_stream_and_hls_without_another_encoder() {
        ffmpeg_next::init().unwrap();
        let dir = TestDirectory(
            std::env::temp_dir().join(format!("explicit-variant-reuse-{}", std::process::id())),
        );
        std::fs::create_dir_all(dir.path().join("derived")).unwrap();
        let mut config = OutputConfig::new(320, 240, 25, 48_000);
        config.stream_type = crate::StreamType::Custom;
        config.stream_format = "mpegts".into();
        config
            .video_options
            .insert("preset".into(), "ultrafast".into());
        let variants = vec![
            HlsVariant {
                name: "stream".into(),
                width: 320,
                height: 240,
                video_bitrate: 800_000,
                audio_bitrate: 128_000,
            },
            HlsVariant {
                name: "low".into(),
                width: 160,
                height: 120,
                video_bitrate: 400_000,
                audio_bitrate: 96_000,
            },
        ];
        let captions = Some(HlsSubtitle {
            name: "Subtitles".into(),
            language: "en".into(),
            default: true,
        });
        let mut source_config = config.clone();
        let mut dedicated =
            crate::RecordingConfig::new(dir.path().join("dedicated").to_string_lossy(), 30);
        dedicated.encode = Some(crate::EncodingConfig {
            width: 80,
            height: 60,
            video_codec: "libx264".into(),
            video_options: config.video_options.clone(),
            audio_codec: "aac".into(),
            audio_options: BTreeMap::new(),
            audio_bitrate: 96_000,
        });
        source_config.recording = Some(dedicated);
        config.destinations.push(OutputDestination {
            id: 1,
            config: source_config,
            target: OutputTarget::Hls {
                playlist: dir
                    .path()
                    .join("stream.m3u8")
                    .to_string_lossy()
                    .into_owned(),
                variants,
                subtitle: captions.clone(),
                segment_seconds: 1,
                list_size: 6,
            },
        });
        let mut borrowed = config.clone();
        borrowed.destinations.clear();
        borrowed.encoding_source = Some((1, 1));
        borrowed.recording = Some(crate::RecordingConfig::new(
            dir.path().join("recording").to_string_lossy(),
            30,
        ));
        config.destinations.push(OutputDestination {
            id: 2,
            config: borrowed.clone(),
            target: OutputTarget::Stream {
                url: dir.path().join("low.ts").to_string_lossy().into_owned(),
            },
        });
        borrowed.recording = None;
        config.destinations.push(OutputDestination {
            id: 3,
            config: borrowed,
            target: OutputTarget::Hls {
                playlist: dir
                    .path()
                    .join("derived/copy.m3u8")
                    .to_string_lossy()
                    .into_owned(),
                variants: Vec::new(),
                subtitle: captions,
                segment_seconds: 1,
                list_size: 6,
            },
        });
        let output = ParallelOutput::new(&config).unwrap();
        assert_eq!(output.workers.lock().unwrap().len(), 1);
        let media = dir.path().join("clip.mp4");
        std::fs::write(
            media.with_extension("vtt"),
            "WEBVTT\n\n00:00:00.000 --> 00:00:00.800\nShared captions\n",
        )
        .unwrap();
        output
            .write_vtt_subtitles(media.to_str().unwrap(), 0, 0)
            .unwrap();
        for index in 0..30 {
            let mut video = frame::Video::new(Pixel::YUV420P, 320, 240);
            for plane in 0..video.planes() {
                video.data_mut(plane).fill(100 + index as u8);
            }
            video.set_pts(Some(index));
            output.encode_video(&video).unwrap();
            for part in 0..2 {
                let mut audio =
                    frame::Audio::new(Sample::F32(Type::Planar), 1024, ChannelLayout::STEREO);
                audio.set_rate(48_000);
                audio.set_pts(Some((index * 2 + part) * 1024));
                for channel in 0..2 {
                    audio.plane_mut::<f32>(channel).fill(0.0);
                }
                output.encode_audio(&audio).unwrap();
            }
            output.advance_vtt_subtitles(index * 40).unwrap();
        }
        output.finish().unwrap();
        // Production playlists remain live; mark completed test fixtures as finite before demuxing.
        for directory in [dir.path().to_path_buf(), dir.path().join("derived")] {
            for entry in std::fs::read_dir(directory).unwrap().flatten() {
                if entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "m3u8")
                {
                    let mut playlist = std::fs::read_to_string(entry.path()).unwrap();
                    playlist.push_str("\n#EXT-X-ENDLIST\n");
                    std::fs::write(entry.path(), playlist).unwrap();
                }
            }
        }
        let video_packets = |path: &std::path::Path| {
            let mut input = ffmpeg_next::format::input(path).unwrap();
            let stream = input
                .streams()
                .best(ffmpeg_next::media::Type::Video)
                .unwrap();
            let index = stream.index();
            let decoder =
                ffmpeg_next::codec::context::Context::from_parameters(stream.parameters())
                    .unwrap()
                    .decoder()
                    .video()
                    .unwrap();
            assert_eq!((decoder.width(), decoder.height()), (160, 120));
            input
                .packets()
                .filter(|(stream, _)| stream.index() == index)
                .map(|(_, packet)| packet.data().unwrap().to_vec())
                .collect::<Vec<_>>()
        };
        let source = video_packets(&dir.path().join("low.m3u8"));
        assert_eq!(source.len(), 30);
        assert_eq!(source, video_packets(&dir.path().join("low.ts")));
        assert_eq!(source, video_packets(&dir.path().join("derived/copy.m3u8")));
        let recording = std::fs::read_dir(dir.path().join("recording"))
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "mkv")
            })
            .unwrap()
            .path();
        assert_eq!(video_packets(&recording).len(), 30);
        let dedicated = std::fs::read_dir(dir.path().join("dedicated"))
            .unwrap()
            .flatten()
            .find(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "mkv")
            })
            .unwrap()
            .path();
        let mut input = ffmpeg_next::format::input(&dedicated).unwrap();
        let stream = input
            .streams()
            .best(ffmpeg_next::media::Type::Video)
            .unwrap();
        let index = stream.index();
        let decoder = ffmpeg_next::codec::context::Context::from_parameters(stream.parameters())
            .unwrap()
            .decoder()
            .video()
            .unwrap();
        assert_eq!((decoder.width(), decoder.height()), (80, 60));
        assert_eq!(
            input
                .packets()
                .filter(|(stream, _)| stream.index() == index)
                .count(),
            30
        );

        let has_captions = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "vtt")
            })
            .filter(|entry| entry.path() != media.with_extension("vtt"))
            .any(|entry| {
                std::fs::read_to_string(entry.path())
                    .unwrap()
                    .contains("Shared captions")
            });
        assert!(has_captions);
        assert!(
            config
                .output_status
                .snapshot()
                .values()
                .all(|state| state.phase == "stopped")
        );
    }

    #[test]
    fn a_full_destination_queue_is_disabled_without_blocking_other_destinations() {
        let status = OutputStatus::default();
        let control = Arc::new(PipelineControl::default());
        let queue = PipelineQueue::new(
            QueueLimits {
                items: 1,
                bytes: 1024,
                duration_us: 1_000_000,
            },
            control.clone(),
        );
        let blocked = DestinationWorker {
            encoding_revision: String::new(),
            gop: 50,
            subtitles: false,
            id: 1,
            ids: vec![1],
            video_resolutions: Vec::new(),
            queue,
            control,
            worker: None,
            channel: 1,
            status: status.clone(),
        };
        blocked.send(Message::Rate(1.0), 0, None);
        blocked.send(Message::Rate(1.0), 0, None);
        assert!(blocked.control.cancelled());
        assert_eq!(status.snapshot()[&1].phase, "failed");
        let healthy_control = Arc::new(PipelineControl::default());
        let healthy = DestinationWorker {
            encoding_revision: String::new(),
            gop: 50,
            subtitles: false,
            id: 2,
            ids: vec![2],
            video_resolutions: Vec::new(),
            queue: PipelineQueue::new(
                QueueLimits {
                    items: 1,
                    bytes: 1024,
                    duration_us: 1_000_000,
                },
                healthy_control.clone(),
            ),
            control: healthy_control,
            worker: None,
            channel: 1,
            status,
        };
        healthy.send(Message::Rate(1.0), 0, None);
        assert!(healthy.queue.try_recv().unwrap().is_some());
        assert!(!healthy.control.cancelled());
    }
}
