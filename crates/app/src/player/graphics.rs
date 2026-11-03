//! Playlist graphics schema and bounded background preparation.
use std::{
    collections::BTreeMap,
    num::NonZeroU32,
    path::{Path, PathBuf},
    sync::{Arc, Mutex as StdMutex, PoisonError},
    time::Duration,
};

use anyhow::{Context, Result, anyhow, ensure};
use ff_engine::ClipGraphics;
use ff_graphics::{
    ffmpeg::{scene::CpuScene, sources::animation::AnimationOptions},
    playback::{Repeat, Transition},
    playlist::{Action, ClipSchedule, Event, Limits},
    scene::{LayerId, LayerSpec, Scope},
    source::Completion,
    surface::{Resource, ResourceKey},
};
use log::warn;
use serde::Deserialize;
use serde_json::Value;
use tokio::{
    sync::{Mutex, Notify, Semaphore},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

use crate::{file::norm_abs_path, player::utils::Media};

#[derive(Deserialize)]
struct GraphicEvent {
    at: f64,
    #[serde(flatten)]
    action: GraphicAction,
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum GraphicAction {
    Take {
        layer: u64,
        source: String,
        #[serde(default)]
        size: Option<(u32, u32)>,
        #[serde(default)]
        position: (i32, i32),
        #[serde(default = "full_opacity")]
        opacity: f64,
        #[serde(default)]
        z_index: i32,
        #[serde(default)]
        fade_in: f64,
        #[serde(default)]
        hold: Option<f64>,
        #[serde(default)]
        duration: Option<f64>,
        #[serde(default)]
        fade_out: f64,
        #[serde(default)]
        pause: f64,
        #[serde(default)]
        repeat: Repetitions,
        #[serde(default = "forever")]
        source_repeat: Repetitions,
        #[serde(default)]
        source_end: SourceEnd,
    },
    Update {
        layer: u64,
        #[serde(default)]
        position: Option<(i32, i32)>,
        #[serde(default)]
        opacity: Option<f64>,
        #[serde(default)]
        z_index: Option<i32>,
    },
    Stop {
        layer: u64,
    },
    Visibility {
        layer: u64,
        visible: bool,
    },
    Clear {
        layer: u64,
    },
    ClearAll,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Repetitions {
    Count(NonZeroU32),
    Keyword(Forever),
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Forever {
    Forever,
}

impl Default for Repetitions {
    fn default() -> Self {
        Self::Count(NonZeroU32::MIN)
    }
}

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum SourceEnd {
    #[default]
    Hide,
    HoldLast,
}

fn forever() -> Repetitions {
    Repetitions::Keyword(Forever::Forever)
}

impl Repetitions {
    fn count(&self) -> u32 {
        match self {
            Self::Count(count) => count.get(),
            Self::Keyword(Forever::Forever) => 0,
        }
    }

    fn playback(&self) -> Repeat {
        match self {
            Self::Count(count) => Repeat::Count(*count),
            Self::Keyword(Forever::Forever) => Repeat::Forever,
        }
    }
}

fn full_opacity() -> f64 {
    1.0
}

fn microseconds(seconds: f64) -> Result<i64> {
    let micros = (seconds * 1_000_000.0).round();
    ensure!(
        seconds.is_finite() && seconds >= 0.0 && micros < i64::MAX as f64,
        "graphics times must be finite, nonnegative seconds within range"
    );

    Ok(micros as i64)
}

/// Two bounded workers and one replaceable pending clip per channel. Completed
/// clips never publish through a later occurrence's weak completion ticket.
#[derive(Debug)]
pub(crate) struct GraphicsPreparer {
    preparation: ff_graphics::preparation::PreparationService,
    active: Arc<Semaphore>,
    state: Arc<StdMutex<PreparationState>>,
    wake: Arc<Notify>,
}

type PreparationOperation = Box<dyn FnOnce(CancellationToken) + Send + 'static>;

struct PreparationState {
    accepting: bool,
    jobs: Vec<Arc<PreparationJob>>,
    pending: Option<PreparationOperation>,
    dispatcher: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for PreparationState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparationState")
            .field("accepting", &self.accepting)
            .field("jobs", &self.jobs.len())
            .field("pending", &self.pending.is_some())
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
struct PreparationJob {
    cancellation: CancellationToken,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl GraphicsPreparer {
    pub(crate) fn new() -> Self {
        Self {
            preparation: Default::default(),
            active: Arc::new(Semaphore::new(2)),
            state: Arc::new(StdMutex::new(PreparationState {
                accepting: true,
                jobs: Vec::new(),
                pending: None,
                dispatcher: None,
            })),
            wake: Arc::new(Notify::new()),
        }
    }

    pub(crate) fn preparation(&self) -> ff_graphics::preparation::PreparationService {
        self.preparation.clone()
    }

    pub(crate) fn resume(&self) {
        // Remember the channel's intent even while the old text worker is
        // finishing. Admission retries recovery after that worker exits.
        self.preparation.resume();
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .accepting = true;
    }

    pub(crate) fn cancel(&self) {
        self.preparation.cancel();
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.accepting = false;
        state.pending = None;
        for job in &state.jobs {
            job.cancellation.cancel();
        }
        self.wake.notify_one();
    }

    pub(crate) async fn join(&self, channel: i32) {
        let preparation = self.preparation.clone();
        let stopped = tokio::task::spawn_blocking(move || preparation.join(Duration::from_secs(2)))
            .await
            .unwrap_or(false);
        if !stopped {
            warn!(channel = channel; "Graphics preparation still stopping; retaining its worker slot");
        }
        self.join_with_timeout(channel, Duration::from_secs(2))
            .await;
    }

    async fn join_with_timeout(&self, channel: i32, wait: Duration) {
        let jobs = self
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .jobs
            .clone();
        let deadline = tokio::time::Instant::now() + wait;
        for job in jobs {
            let mut slot = job.task.lock().await;
            let Some(task) = slot.as_mut() else {
                continue;
            };
            match tokio::time::timeout_at(deadline, task).await {
                Ok(result) => {
                    *slot = None;
                    if let Err(error) = result {
                        warn!(channel = channel; "Playlist graphics preparation failed: {error}");
                    }
                }
                Err(_) => {
                    // Retain every handle and permit while non-interruptible I/O
                    // finishes, including across channel restarts.
                    warn!(channel = channel; "Playlist graphics preparation is still stopping; retaining active worker slots");
                    break;
                }
            }
        }
    }

    fn cancel_clips(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.pending = None;
        for job in &state.jobs {
            job.cancellation.cancel();
        }
    }

    fn spawn(&self, operation: impl FnOnce(CancellationToken) + Send + 'static) -> bool {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if !state.accepting {
            return false;
        }
        for job in &state.jobs {
            job.cancellation.cancel();
        }
        // Replacing this closure immediately drops an obsolete clip's JSON,
        // paths and weak completion ticket. No blocking task exists for it yet.
        state.pending = Some(Box::new(operation));
        if state.dispatcher.is_none() {
            state.dispatcher = Some(tokio::spawn(Self::dispatch(
                self.state.clone(),
                self.active.clone(),
                self.preparation.clone(),
                self.wake.clone(),
            )));
        }
        self.wake.notify_one();

        true
    }

    async fn dispatch(
        shared: Arc<StdMutex<PreparationState>>,
        active: Arc<Semaphore>,
        preparation: ff_graphics::preparation::PreparationService,
        wake: Arc<Notify>,
    ) {
        loop {
            let notified = wake.notified();
            let waiting = {
                let mut state = shared.lock().unwrap_or_else(PoisonError::into_inner);
                state.jobs.retain(|job| {
                    let Ok(mut task) = job.task.try_lock() else {
                        return true;
                    };
                    if task.as_ref().is_some_and(|task| !task.is_finished()) {
                        return true;
                    }
                    task.take();

                    false
                });
                if state.pending.is_none() && state.jobs.is_empty() {
                    // Retire the coordinator too. Admission starts another one
                    // under this same lock if a new clip arrives after cleanup.
                    state.dispatcher = None;

                    return;
                }
                if state.accepting
                    && state.pending.is_some()
                    && state.jobs.len() < 2
                    && preparation.ready()
                    && let Ok(permit) = active.clone().try_acquire_owned()
                    && let Ok(job_permit) = preparation.budget().job()
                    && let Some(operation) = state.pending.take()
                {
                    let cancellation = CancellationToken::new();
                    let worker_cancellation = cancellation.clone();
                    let finished = wake.clone();
                    let task = tokio::spawn(async move {
                        let result = tokio::task::spawn_blocking(move || {
                            let _permit = permit;
                            let _job_permit = job_permit;
                            if !worker_cancellation.is_cancelled() {
                                operation(worker_cancellation);
                            }
                        })
                        .await;
                        if let Err(error) = result {
                            warn!("Playlist graphics preparation worker failed: {error}");
                        }
                        finished.notify_one();
                    });
                    state.jobs.push(Arc::new(PreparationJob {
                        cancellation,
                        task: Mutex::new(Some(task)),
                    }));
                }
                // Reap the final completed handles too, even if no new
                // clip arrives and completion wakes us just before the
                // wrapper task changes to its finished state.
                (state.accepting && state.pending.is_some()) || !state.jobs.is_empty()
            };
            if waiting {
                // Also recover when shared text/decoder work releases a
                // budget slot; it has no notification for this dispatcher.
                tokio::select! {
                    _ = notified => {},
                    _ = tokio::time::sleep(Duration::from_millis(10)) => {},
                }
            } else {
                notified.await;
            }
        }
    }

    pub(crate) fn for_clip(
        &self,
        node: &Media,
        root: &Path,
        canvas: (u32, u32),
        channel: i32,
    ) -> Option<ClipGraphics> {
        self.cancel_clips();
        let data = node.graphics.as_ref()?;
        if data.is_null() || data.as_array().is_some_and(Vec::is_empty) {
            return None;
        }
        let position_us = match microseconds(node.graphics_elapsed) {
            Ok(position) => position,
            Err(error) => {
                warn!(channel = channel; "Playlist graphics omitted: {error:#}");
                return None;
            }
        };
        let duration_us = match microseconds(node.out - node.seek + node.graphics_elapsed) {
            Ok(duration) if duration > 0 => duration,
            _ => {
                warn!(channel = channel; "Playlist graphics omitted: invalid clip duration");
                return None;
            }
        };
        let data = data.clone();
        let root = root.to_path_buf();
        let (pending, completion) = ClipGraphics::deferred();
        let preparation = self.preparation.clone();
        let started = self.spawn(move |cancellation| {
            if cancellation.is_cancelled() || completion.is_cancelled() {
                return;
            }
            let result = prepare_scene_budgeted(
                &data,
                &root,
                canvas,
                duration_us,
                channel,
                || cancellation.is_cancelled() || completion.is_cancelled(),
                Some(preparation),
            );
            if cancellation.is_cancelled() || completion.is_cancelled() {
                return;
            }

            match result {
                Ok((renderer, schedule)) => {
                    completion.publish(ClipGraphics::new(renderer, schedule));
                }
                Err(error) => {
                    warn!(channel = channel; "Playlist graphics omitted: {error:#}");
                }
            }
        });
        if !started {
            warn!(channel = channel; "Playlist graphics omitted: channel preparation is stopping");
            return None;
        }

        Some(pending.at_position(position_us))
    }
}

impl Drop for GraphicsPreparer {
    fn drop(&mut self) {
        self.cancel();
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        // The dispatcher never owns a blocking operation outside the tracked jobs.
        // Abort only this idle/coordinating future; running jobs retain their leases.
        if let Some(dispatcher) = state.dispatcher.take() {
            dispatcher.abort();
        }
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Asset {
    path: PathBuf,
    size: Option<(u32, u32)>,
    source_repeat: u32,
    source_end: SourceEnd,
}

#[cfg(test)]
fn prepare_scene(
    data: &Value,
    root: &Path,
    canvas: (u32, u32),
    duration_us: i64,
    channel: i32,
    cancelled: impl Fn() -> bool,
) -> Result<(CpuScene, ClipSchedule)> {
    prepare_scene_budgeted(
        data,
        root,
        canvas,
        duration_us,
        channel,
        cancelled,
        Some(ff_graphics::preparation::PreparationService::default()),
    )
}

fn prepare_scene_budgeted(
    data: &Value,
    root: &Path,
    canvas: (u32, u32),
    duration_us: i64,
    channel: i32,
    cancelled: impl Fn() -> bool,
    preparation: Option<ff_graphics::preparation::PreparationService>,
) -> Result<(CpuScene, ClipSchedule)> {
    let array = data
        .as_array()
        .context("graphics must be an array of events")?;
    ensure!(
        array.len() <= Limits::default().events,
        "graphics event limit exceeded (256)"
    );
    let parsed: Vec<GraphicEvent> =
        serde_json::from_value(data.clone()).context("invalid graphics event")?;
    let mut renderer = CpuScene::new(64, 64 * 1024 * 1024);
    if let Some(preparation) = &preparation {
        renderer = renderer.with_preparation(preparation);
    }
    let mut assets: BTreeMap<Asset, Option<Arc<Resource>>> = BTreeMap::new();
    let mut layers: BTreeMap<u64, Option<LayerSpec>> = BTreeMap::new();
    let mut events = Vec::new();
    let mut last_at = 0;
    for (index, event) in parsed.into_iter().enumerate() {
        ensure!(!cancelled(), "graphics preparation cancelled");
        let at_us = microseconds(event.at).with_context(|| format!("graphics event {index}"))?;
        ensure!(at_us >= last_at, "graphics events must be ordered by at");
        last_at = at_us;
        if at_us >= duration_us {
            continue;
        }
        let action = match event.action {
            GraphicAction::Take {
                layer,
                source,
                size,
                position,
                opacity,
                z_index,
                fade_in,
                hold,
                duration,
                fade_out,
                pause,
                repeat,
                source_repeat,
                source_end,
            } => {
                ensure!(
                    duration.is_none() || hold.is_none(),
                    "use duration or hold, not both"
                );
                let mut transition = Transition {
                    fade_in_us: microseconds(fade_in)? as u64,
                    hold_us: hold
                        .map(microseconds)
                        .transpose()?
                        .map(|value| value as u64),
                    fade_out_us: microseconds(fade_out)? as u64,
                    pause_us: microseconds(pause)? as u64,
                    repeat: match repeat {
                        Repetitions::Count(count) => Repeat::Count(count),
                        Repetitions::Keyword(Forever::Forever) => Repeat::Forever,
                    },
                };
                if let Some(duration) = duration {
                    transition = transition.within(microseconds(duration)? as u64);
                }
                transition.validate()?;
                if transition.hold_us.is_none() {
                    transition = transition.within((duration_us - at_us) as u64);
                }
                let path = if source.contains("://") {
                    Err(anyhow!("remote graphic sources are unsupported"))
                } else {
                    norm_abs_path(root, &source)
                        .map(|(path, _, _)| path)
                        .map_err(|error| anyhow!("{error}"))
                };
                let resource = match path {
                    Ok(path) => {
                        let asset = Asset {
                            path,
                            size,
                            source_repeat: source_repeat.count(),
                            source_end,
                        };
                        if let Some(resource) = assets.get(&asset) {
                            resource.clone()
                        } else {
                            ensure!(assets.len() < 64, "graphics asset limit exceeded (64)");
                            let key = ResourceKey {
                                id: assets.len() as u64 + 1,
                                revision: assets.len() as u64 + 1,
                            };
                            let resource = match prepare_source(
                                &mut renderer,
                                key,
                                &asset,
                                source_repeat.playback(),
                                &cancelled,
                            ) {
                                Ok(resource) => Some(resource),
                                Err(error) => {
                                    warn!(channel = channel; "Playlist graphic {source:?} omitted: {error:#}");
                                    None
                                }
                            };
                            assets.insert(asset, resource.clone());

                            resource
                        }
                    }
                    Err(error) => {
                        warn!(channel = channel; "Playlist graphic {source:?} omitted: {error:#}");
                        None
                    }
                };
                let Some(resource) = resource else {
                    layers.entry(layer).or_insert(None);
                    continue;
                };
                let spec = LayerSpec {
                    id: LayerId(layer),
                    resource: resource.key,
                    z: z_index,
                    scope: Scope::default(),
                    canvas,
                    position,
                    opacity,
                };
                layers.insert(layer, Some(spec));

                Action::Take {
                    layer: spec,
                    transition,
                }
            }
            GraphicAction::Update {
                layer,
                position,
                opacity,
                z_index,
            } => {
                let Some(spec) = layers
                    .get_mut(&layer)
                    .context("graphics update refers to an unknown layer")?
                else {
                    continue;
                };
                if let Some(position) = position {
                    spec.position = position;
                }
                if let Some(opacity) = opacity {
                    spec.opacity = opacity;
                }
                if let Some(z_index) = z_index {
                    spec.z = z_index;
                }

                Action::Update(*spec)
            }
            GraphicAction::Stop { layer } => {
                if matches!(layers.get(&layer), Some(None)) {
                    continue;
                }
                Action::Stop(LayerId(layer))
            }
            GraphicAction::Visibility { layer, visible } => {
                if matches!(layers.get(&layer), Some(None)) {
                    continue;
                }
                Action::Visibility {
                    layer: LayerId(layer),
                    visible,
                }
            }
            GraphicAction::Clear { layer } => {
                if matches!(layers.remove(&layer), Some(None)) {
                    continue;
                }
                Action::Clear(LayerId(layer))
            }
            GraphicAction::ClearAll => {
                layers.clear();
                Action::ClearAll
            }
        };
        events.push(Event { at_us, action });
    }
    let resources = assets.into_values().flatten().collect::<Vec<_>>();
    let schedule = ClipSchedule::compile(duration_us, &events, &resources, Limits::default())?;

    Ok((renderer, schedule))
}

/// Opens regular local files through non-nesting demuxers with bounded decoded storage. The channel
/// storage adapter resolved and checked the path before reaching this worker.
fn prepare_source(
    renderer: &mut CpuScene,
    key: ResourceKey,
    asset: &Asset,
    repeat: Repeat,
    cancelled: &impl Fn() -> bool,
) -> Result<Arc<Resource>> {
    if asset
        .path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
    {
        return renderer.prepare_png(key, &asset.path, asset.size);
    }
    renderer.prepare_animation(
        key,
        &asset.path,
        AnimationOptions {
            size: asset.size,
            repeat,
            completion: match asset.source_end {
                SourceEnd::Hide => Completion::Hide,
                SourceEnd::HoldLast => Completion::HoldLast,
            },
        },
        cancelled,
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tokio::time::timeout;

    use super::*;

    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests_assets/storage/media_images")
    }

    fn compile(data: Value, duration_us: i64) -> Result<ClipSchedule> {
        let (_owner, completion) = ClipGraphics::deferred();
        let (_, schedule) = prepare_scene(&data, &root(), (1280, 720), duration_us, 1, || {
            completion.is_cancelled()
        })?;

        Ok(schedule)
    }

    fn opacity(schedule: &ClipSchedule, seconds: f64) -> Option<f64> {
        let frame = schedule.frame_at(Some(microseconds(seconds).unwrap()))?;

        frame
            .scene
            .evaluate(frame.position_us, Scope::default())
            .next()
            .map(|layer| layer.content.opacity)
    }

    #[test]
    fn total_duration_and_implicit_clip_end_include_both_png_fades() {
        let schedule = compile(json!([
            {"at":0,"action":"take","layer":1,"source":"lower-third.png","duration":4,"fade_in":1,"fade_out":1},
            {"at":4,"action":"take","layer":1,"source":"lower-third.png","fade_in":1,"fade_out":1}
        ]), 8_000_000).unwrap();
        assert_eq!(opacity(&schedule, 0.5), Some(0.5));
        assert_eq!(opacity(&schedule, 2.0), Some(1.0));
        assert_eq!(opacity(&schedule, 3.5), Some(0.5));
        assert_eq!(opacity(&schedule, 4.5), Some(0.5));
        assert_eq!(opacity(&schedule, 6.0), Some(1.0));
        assert_eq!(opacity(&schedule, 7.5), Some(0.5));
        assert!(schedule.frame_at(Some(8_000_000)).is_none());
        assert!(compile(json!([
            {"at":0,"action":"take","layer":1,"source":"lower-third.png","duration":4,"hold":2}
        ]), 8_000_000).is_err());
    }

    #[test]
    fn animated_sources_retake_reuse_and_source_repeat_are_independent_from_fades() {
        let schedule = compile(json!([
            {"at":0,"action":"take","layer":1,"source":"animated-logo.gif","size":[16,16],"source_repeat":2,"source_end":"hold_last","fade_in":0.1,"hold":0.2,"fade_out":0.1,"pause":0.1,"repeat":2},
            {"at":1,"action":"take","layer":1,"source":"animated-logo.gif","size":[16,16],"source_repeat":2,"source_end":"hold_last"},
            {"at":2,"action":"take","layer":2,"source":"animated-logo-video.mkv","size":[16,16],"source_repeat":"forever"}
        ]), 4_000_000).unwrap();
        assert!(opacity(&schedule, 0.2).is_some());
        assert_eq!(opacity(&schedule, 0.45), None);
        let first = schedule.frame_at(Some(200_000)).unwrap();
        let retake = schedule.frame_at(Some(1_100_000)).unwrap();
        assert!(Arc::ptr_eq(
            &first.scene.layers[0].resource,
            &retake.scene.layers[0].resource
        ));
        assert_eq!(
            retake.scene.layers[0].presentation.elapsed_us(1_100_000),
            100_000
        );
        let video = schedule.frame_at(Some(2_100_000)).unwrap();
        assert_eq!(video.scene.layers.len(), 2);
        assert!(compile(json!([{"at":0,"action":"take","layer":1,"source":"animated-logo.gif","source_repeat":0}]), 1_000_000).is_err());
        assert!(compile(json!([{"at":0,"action":"take","layer":1,"source":"animated-logo.gif","source_end":"invalid"}]), 1_000_000).is_err());
    }

    #[test]
    fn repeated_lower_third_and_retakes_reuse_the_asset_and_preserve_timing() {
        let schedule = compile(json!([
            {"at":0,"action":"take","layer":1,"source":"lower-third.png","size":[8,4],"opacity":0.5,"fade_in":1,"hold":1,"fade_out":1,"repeat":2},
            {"at":7,"action":"take","layer":1,"source":"lower-third.png","size":[8,4],"opacity":0.5,"fade_out":0.5},
            {"at":7.5,"action":"update","layer":1,"opacity":0.25,"z_index":5,"position":[20,30]},
            {"at":8,"action":"stop","layer":1}
        ]), 10_000_000).unwrap();
        assert_eq!(opacity(&schedule, 0.5), Some(0.25));
        assert_eq!(opacity(&schedule, 3.5), Some(0.25));
        assert_eq!(opacity(&schedule, 6.0), None);
        assert_eq!(opacity(&schedule, 7.0), Some(0.5));
        assert_eq!(opacity(&schedule, 8.25), Some(0.125));
        assert_eq!(opacity(&schedule, 8.5), None);
        let first = schedule.frame_at(Some(500_000)).unwrap();
        let second = schedule.frame_at(Some(7_500_000)).unwrap();
        assert!(Arc::ptr_eq(
            &first.scene.layers[0].resource,
            &second.scene.layers[0].resource
        ));
        assert_eq!(second.scene.layers[0].spec.position, (20, 30));
        assert_eq!(second.scene.layers[0].spec.z, 5);
    }

    #[test]
    fn forever_ends_at_clip_boundary_and_late_events_do_not_load_assets() {
        let schedule = compile(json!([
            {"at":0,"action":"take","layer":1,"source":"lower-third.png","size":[8,4],"fade_in":0.5,"hold":0.5,"repeat":"forever"},
            {"at":20,"action":"take","layer":2,"source":"does-not-exist.png"}
        ]), 10_000_000).unwrap();
        assert_eq!(opacity(&schedule, 9.25), Some(0.5));
        assert!(schedule.frame_at(Some(10_000_000)).is_none());
    }

    #[test]
    fn json_pause_leaves_gaps_and_allows_retaking_immediately_after_the_final_exit() {
        let schedule = compile(json!([
            {"at":0,"action":"take","layer":1,"source":"lower-third.png","size":[8,4],"fade_in":0.25,"hold":0.5,"fade_out":0.25,"pause":2,"repeat":2},
            {"at":2,"action":"update","layer":1,"z_index":3},
            {"at":4,"action":"take","layer":1,"source":"lower-third.png","size":[8,4]}
        ]), 8_000_000).unwrap();
        assert_eq!(opacity(&schedule, 0.125), Some(0.5));
        assert_eq!(opacity(&schedule, 1.0), None);
        assert_eq!(opacity(&schedule, 2.9), None);
        assert_eq!(opacity(&schedule, 3.125), Some(0.5));
        assert_eq!(opacity(&schedule, 4.0), Some(1.0));
        let forever = compile(json!([
            {"at":0,"action":"take","layer":1,"source":"lower-third.png","size":[8,4],"hold":1,"pause":2,"repeat":"forever"}
        ]), 10_000_000).unwrap();
        assert_eq!(opacity(&forever, 1.0), None);
        assert_eq!(opacity(&forever, 3.0), Some(1.0));
        assert!(
            compile(
                json!([{"at":0,"action":"take","layer":1,"source":"missing.png","pause":-1}]),
                1_000_000
            )
            .is_err()
        );
    }

    #[test]
    fn missing_assets_do_not_disable_other_layers_and_paths_stay_inside_storage() {
        let schedule = compile(
            json!([
                {"at":0,"action":"take","layer":1,"source":"missing.png"},
                {"at":0,"action":"take","layer":2,"source":"lower-third.png","size":[8,4]},
                {"at":1,"action":"stop","layer":1},
                {"at":1,"action":"update","layer":1,"opacity":0.5},
                {"at":1,"action":"take","layer":3,"source":"../lower-third.png"},
                {"at":1,"action":"take","layer":4,"source":"https://example.invalid/image.png"}
            ]),
            2_000_000,
        )
        .unwrap();
        let selected = schedule.frame_at(Some(1_500_000)).unwrap();
        assert_eq!(selected.scene.layers.len(), 1);
        assert_eq!(selected.scene.layers[0].spec.id, LayerId(2));
    }

    #[test]
    fn malformed_graphics_roundtrip_without_breaking_a_playlist_and_affect_equality() {
        let mut media: Media = serde_json::from_value(json!({
            "source":"clip.mp4","in":12,"out":20,"duration":30,
            "graphics":{"invalid":"preserved"}
        }))
        .unwrap();
        let before = media.clone();
        assert!(Arc::ptr_eq(
            media.graphics.as_ref().unwrap(),
            before.graphics.as_ref().unwrap()
        ));
        media.graphics_elapsed = 3.0;
        assert_eq!(media, before);
        let serialized = serde_json::to_value(&media).unwrap();
        assert_eq!(serialized["graphics"], json!({"invalid":"preserved"}));
        assert!(serialized.get("graphics_elapsed").is_none());
        assert!(compile(media.graphics.as_deref().unwrap().clone(), 8_000_000).is_err());
        media.graphics = None;
        assert_ne!(media, before);
        assert!(
            serde_json::to_value(media)
                .unwrap()
                .get("graphics")
                .is_none()
        );
    }

    #[cfg(unix)]
    #[test]
    fn graphic_symlinks_cannot_escape_channel_storage() {
        let storage =
            std::env::temp_dir().join(format!("ffplayout-graphic-path-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&storage).unwrap();
        let outside = root().join("lower-third.png").canonicalize().unwrap();
        std::os::unix::fs::symlink(outside, storage.join("escape.png")).unwrap();
        let (_owner, completion) = ClipGraphics::deferred();
        let (_, schedule) = prepare_scene(
            &json!([{"at":0,"action":"take","layer":1,"source":"escape.png"}]),
            &storage,
            (1280, 720),
            1_000_000,
            1,
            || completion.is_cancelled(),
        )
        .unwrap();
        assert!(schedule.frame_at(Some(0)).is_none());
        std::fs::remove_dir_all(storage).unwrap();
    }

    #[test]
    fn bad_times_repeats_and_event_budgets_are_rejected_before_loading() {
        assert!(microseconds(f64::NAN).is_err());
        assert!(microseconds(f64::INFINITY).is_err());
        assert!(microseconds(-1.0).is_err());
        assert!(microseconds(1e20).is_err());
        assert!(compile(json!([{"at":-1,"action":"clear_all"}]), 1_000_000).is_err());
        assert!(
            compile(
                json!([{"at":1,"action":"clear_all"},{"at":0,"action":"clear_all"}]),
                2_000_000
            )
            .is_err()
        );
        for repeat in [json!(0), json!(-1), json!("unknown"), json!("forever")] {
            assert!(compile(json!([{"at":0,"action":"take","layer":1,"source":"missing.png","repeat":repeat}]), 1_000_000).is_err());
        }
        assert!(
            compile(
                Value::Array(vec![json!({"at":0,"action":"clear_all"}); 257]),
                1_000_000
            )
            .is_err()
        );
        let (owner, completion) = ClipGraphics::deferred();
        drop(owner);
        assert!(
            prepare_scene(
                &json!([{"at":0,"action":"clear_all"}]),
                &root(),
                (1280, 720),
                1_000_000,
                1,
                || completion.is_cancelled()
            )
            .is_err()
        );
    }

    #[tokio::test]
    async fn no_graphics_starts_no_task_and_busy_preparation_queues_without_blocking() {
        let preparer = GraphicsPreparer::new();
        assert!(
            preparer
                .for_clip(&Media::default(), &root(), (1280, 720), 1)
                .is_none()
        );
        assert_eq!(preparer.active.available_permits(), 2);
        assert!(preparer.state.lock().unwrap().dispatcher.is_none());
        let _busy = preparer.active.clone().acquire_many_owned(2).await.unwrap();
        let node = Media {
            out: 10.0,
            graphics: Some(Arc::new(json!([{"at":0,"action":"clear_all"}]))),
            ..Media::default()
        };
        let pending = preparer.for_clip(&node, &root(), (1280, 720), 1);
        assert!(pending.is_some());
        assert!(preparer.state.lock().unwrap().pending.is_some());
        preparer.for_clip(&Media::default(), &root(), (1280, 720), 1);
        assert!(preparer.state.lock().unwrap().pending.is_none());
    }

    #[tokio::test]
    async fn two_workers_run_and_only_the_latest_waiting_clip_is_retained() {
        let preparer = GraphicsPreparer::new();
        let mut releases = Vec::new();
        let mut cancellations = Vec::new();

        for _ in 0..2 {
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            assert!(preparer.spawn(move |cancellation| {
                let _ = started_tx.send(cancellation);
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            }));
            cancellations.push(
                timeout(Duration::from_secs(3), started_rx)
                    .await
                    .unwrap()
                    .unwrap(),
            );
            releases.push(release_tx);
        }
        assert_eq!(preparer.active.available_permits(), 0);
        assert!(cancellations[0].is_cancelled());
        let dropped = Arc::new(());
        let obsolete = dropped.clone();
        assert!(preparer.spawn(move |_| {
            let _keep = obsolete;
            panic!("an obsolete queued clip must never run");
        }));
        assert_eq!(Arc::strong_count(&dropped), 2);
        let (latest_tx, mut latest_rx) = tokio::sync::oneshot::channel();
        assert!(preparer.spawn(move |cancellation| {
            assert!(!cancellation.is_cancelled());
            let _ = latest_tx.send(());
        }));
        assert_eq!(
            Arc::strong_count(&dropped),
            1,
            "replaced job releases captured resources"
        );
        assert!(cancellations.iter().all(CancellationToken::is_cancelled));
        tokio::task::yield_now().await;
        assert!(matches!(
            latest_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        releases.pop().unwrap().send(()).unwrap();
        timeout(Duration::from_secs(3), latest_rx)
            .await
            .unwrap()
            .unwrap();
        releases.pop().unwrap().send(()).unwrap();
        preparer.cancel();
        preparer.join(1).await;
        assert_eq!(preparer.active.available_permits(), 2);
        assert_eq!(preparer.preparation.budget().usage().2, 0);
    }

    #[tokio::test]
    async fn stop_discards_pending_work_and_keeps_both_blocked_workers_tracked() {
        let preparer = GraphicsPreparer::new();
        let mut releases = Vec::new();

        for _ in 0..2 {
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            assert!(preparer.spawn(move |cancellation| {
                let _ = started_tx.send(cancellation);
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            }));
            timeout(Duration::from_secs(3), started_rx)
                .await
                .unwrap()
                .unwrap();
            releases.push(release_tx);
        }
        let captured = Arc::new(());
        let pending = captured.clone();
        assert!(preparer.spawn(move |_| {
            drop(pending);
            panic!("stopped pending job ran");
        }));
        preparer.cancel();
        assert_eq!(Arc::strong_count(&captured), 1);
        preparer
            .join_with_timeout(1, Duration::from_millis(5))
            .await;
        assert_eq!(preparer.active.available_permits(), 0);
        assert_eq!(preparer.state.lock().unwrap().jobs.len(), 2);
        assert!(!preparer.spawn(|_| panic!("stopped channel accepted work")));
        preparer.resume();
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        assert!(preparer.spawn(move |_| {
            let _ = done_tx.send(());
        }));
        releases.pop().unwrap().send(()).unwrap();
        timeout(Duration::from_secs(3), done_rx)
            .await
            .unwrap()
            .unwrap();
        releases.pop().unwrap().send(()).unwrap();
        preparer.cancel();
        preparer.join(1).await;
        assert_eq!(preparer.active.available_permits(), 2);
        assert_eq!(preparer.preparation.budget().usage().2, 0);
    }

    #[tokio::test]
    async fn dropping_preparer_cancels_workers_and_drops_queued_captures() {
        let preparer = GraphicsPreparer::new();
        let shared = Arc::downgrade(&preparer.state);
        let active = preparer.active.clone();
        let budget = preparer.preparation.budget().clone();
        let mut releases = Vec::new();
        let mut cancellations = Vec::new();

        for _ in 0..2 {
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            assert!(preparer.spawn(move |cancellation| {
                let _ = started_tx.send(cancellation);
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            }));
            cancellations.push(
                timeout(Duration::from_secs(3), started_rx)
                    .await
                    .unwrap()
                    .unwrap(),
            );
            releases.push(release_tx);
        }
        let captured = Arc::new(());
        let pending = captured.clone();
        assert!(preparer.spawn(move |_| {
            drop(pending);
            panic!("dropped preparer ran pending work");
        }));
        drop(preparer);
        assert_eq!(Arc::strong_count(&captured), 1);
        assert!(cancellations.iter().all(CancellationToken::is_cancelled));

        for release in releases {
            release.send(()).unwrap();
        }
        timeout(Duration::from_secs(3), async {
            while shared.upgrade().is_some()
                || active.available_permits() != 2
                || budget.usage().2 != 0
            {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn idle_dispatcher_retires_and_new_work_restarts_it_after_a_worker_panic() {
        let preparer = GraphicsPreparer::new();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        assert!(preparer.spawn(move |_| {
            let _ = started_tx.send(());
            panic!("simulated preparation failure");
        }));
        timeout(Duration::from_secs(3), started_rx)
            .await
            .unwrap()
            .unwrap();
        timeout(Duration::from_secs(3), async {
            loop {
                let idle = {
                    let state = preparer.state.lock().unwrap();
                    state.jobs.is_empty() && state.dispatcher.is_none()
                };
                if idle {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(preparer.active.available_permits(), 2);
        assert_eq!(preparer.preparation.budget().usage().2, 0);
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        assert!(preparer.spawn(move |_| {
            let _ = done_tx.send(());
        }));
        timeout(Duration::from_secs(3), done_rx)
            .await
            .unwrap()
            .unwrap();
        preparer.cancel();
        preparer.join(1).await;
        assert_eq!(preparer.active.available_permits(), 2);
        assert_eq!(preparer.preparation.budget().usage().2, 0);
    }

    #[tokio::test]
    async fn restart_recovers_automatically_after_a_slow_text_worker_finishes() {
        let preparer = GraphicsPreparer::new();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let signals = Arc::new(StdMutex::new(Some((entered_tx, release_rx))));
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        let pending = loop {
            let signals = signals.clone();
            if let Ok(pending) = preparer.preparation.submit(move |_, _| {
                let (entered_tx, release_rx) = signals.lock().unwrap().take().unwrap();
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                Ok(())
            }) {
                break pending;
            }
            // Nonblocking admission may race the freshly started worker's
            // queue lock; preserve the operation and retry test setup.
            assert!(tokio::time::Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(1)).await;
        };
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        preparer.cancel();
        assert!(!preparer.preparation.join(Duration::ZERO));
        preparer.resume();
        let (ran_tx, mut ran_rx) = tokio::sync::oneshot::channel();
        assert!(preparer.spawn(move |_| {
            let _ = ran_tx.send(());
        }));
        tokio::task::yield_now().await;
        assert!(matches!(
            ran_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        release_tx.send(()).unwrap();
        timeout(Duration::from_secs(3), ran_rx)
            .await
            .unwrap()
            .unwrap();
        assert!(
            pending.result().is_none(),
            "old cancelled work must never publish into the restarted channel"
        );
        preparer.cancel();
        preparer.join(1).await;
        assert!(!preparer.spawn(|_| panic!("explicitly stopped channel must stay stopped")));
    }

    #[tokio::test]
    async fn channel_stop_tracks_old_job_and_restart_uses_only_the_spare_slot() {
        let (manager, pool) = crate::test_support::channel_manager().await;
        let storage = manager.config.read().await.storage.path.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
        assert!(manager.graphics.spawn(move |cancellation| {
            let _ = started_tx.send(cancellation);
            // Simulate an in-progress file read which cannot be interrupted.
            let _ = release_rx.recv_timeout(Duration::from_secs(10));
        }));
        let cancellation = timeout(Duration::from_secs(5), started_rx)
            .await
            .unwrap()
            .unwrap();
        manager.stop_all(false).await;
        assert!(cancellation.is_cancelled());
        assert_eq!(manager.graphics.active.available_permits(), 1);
        let job = manager.graphics.state.lock().unwrap().jobs[0].clone();
        assert!(
            job.task.lock().await.is_some(),
            "a timed-out blocking job must stay tracked"
        );
        let restarted = manager.clone();
        assert!(Arc::ptr_eq(&manager.graphics, &restarted.graphics));
        restarted.graphics.resume();
        let (spare_tx, spare_rx) = tokio::sync::oneshot::channel();
        assert!(restarted.graphics.spawn(move |_| {
            let _ = spare_tx.send(());
        }));
        timeout(Duration::from_secs(3), spare_rx)
            .await
            .unwrap()
            .unwrap();
        release_tx.send(()).unwrap();
        restarted.graphics.join(manager.id).await;
        assert_eq!(manager.graphics.active.available_permits(), 2);
        assert!(job.task.lock().await.is_none());
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        assert!(restarted.graphics.spawn(move |cancellation| {
            assert!(!cancellation.is_cancelled());
            let _ = done_tx.send(());
        }));
        timeout(Duration::from_secs(5), done_rx)
            .await
            .unwrap()
            .unwrap();
        restarted.graphics.join(manager.id).await;
        drop(restarted);
        drop(manager);
        pool.close().await;
        std::fs::remove_dir_all(storage).unwrap();
    }

    #[tokio::test]
    async fn playlist_save_load_and_graphics_only_edits_preserve_optional_data() {
        use crate::{
            player::utils::JsonPlaylist,
            utils::playlist::{read_playlist, write_playlist},
        };

        let (manager, pool) = crate::test_support::channel_manager().await;
        let config = manager.config.read().await.clone();
        let mut playlist = JsonPlaylist {
            date: "2026-11-02".into(),
            program: vec![Media {
                source: "clip.mp4".into(),
                out: 10.0,
                duration: 10.0,
                graphics: Some(Arc::new(
                    json!([{"at":1,"action":"take","layer":1,"source":"lower-third.png","opacity":0.5}]),
                )),
                ..Media::default()
            }],
            ..JsonPlaylist::default()
        };
        write_playlist(&config, playlist.clone()).await.unwrap();
        Arc::make_mut(playlist.program[0].graphics.as_mut().unwrap())[0]["opacity"] = json!(0.75);
        assert!(
            write_playlist(&config, playlist.clone())
                .await
                .unwrap()
                .contains("Update")
        );
        let loaded = read_playlist(&config, playlist.date.clone()).await.unwrap();
        assert_eq!(loaded.program[0].graphics, playlist.program[0].graphics);
        assert_eq!(loaded.program[0].graphics_elapsed, 0.0);
        drop(manager);
        pool.close().await;
        std::fs::remove_dir_all(config.storage.path).unwrap();
    }
}
