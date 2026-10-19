use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock, PoisonError},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use futures_util::FutureExt;
use hang::moq_net;
use moq_mux::{catalog, container::ts};
use tokio::{
    io::AsyncReadExt,
    net::TcpListener,
    time::{MissedTickBehavior, interval},
};
use tokio_util::sync::CancellationToken;

use super::relay::{RETENTION, RUNTIME};

#[derive(Clone)]
pub struct ImportStatus {
    pub phase: &'static str,
    pub error: Option<String>,
}

#[derive(Clone, Default)]
pub(crate) struct AudioMeterSource {
    level: Arc<Mutex<Option<ff_engine::AudioLevel>>>,
    loudness: ff_engine::LoudnessMeterControl,
    first_audio_pts: Arc<Mutex<Option<i64>>>,
}

impl AudioMeterSource {
    pub(crate) fn new(
        level: Arc<Mutex<Option<ff_engine::AudioLevel>>>,
        loudness: ff_engine::LoudnessMeterControl,
    ) -> Self {
        Self {
            level,
            loudness,
            first_audio_pts: Default::default(),
        }
    }
}

// Loudness analysis only runs while its MoQ track has subscribers. Drop also
// releases demand when the importer task is cancelled or a connection fails.
struct LoudnessSubscription {
    control: ff_engine::LoudnessMeterControl,
    active: bool,
}

impl LoudnessSubscription {
    fn update(&mut self, active: bool) {
        if active == self.active {
            return;
        }
        if active {
            self.control.subscribe();
        } else {
            self.control.unsubscribe();
        }
        self.active = active;
    }
}

impl Drop for LoudnessSubscription {
    fn drop(&mut self) {
        self.update(false);
    }
}

const AUDIO_LEVEL_INTERVAL: Duration = Duration::from_millis(250);
const AUDIO_LEVEL_TRACK: &str = "audio-level.json";
const LOUDNESS_TRACK: &str = "audio-loudness.json";

type StatusSlot = Arc<Mutex<ImportStatus>>;
static STATUSES: OnceLock<Mutex<BTreeMap<i32, StatusSlot>>> = OnceLock::new();

pub fn status(channel: i32) -> Option<ImportStatus> {
    let statuses = STATUSES
        .get()?
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let state = statuses
        .get(&channel)?
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();

    Some(state)
}

/// Keeps a channel's importer alive only as long as its playout instance.
pub struct Attachment {
    stopped: CancellationToken,
    draining: CancellationToken,
    task: Option<tokio::task::JoinHandle<()>>,
    status: Option<StatusSlot>,
}

impl Attachment {
    /// Normal shutdown waits for the engine's drained TCP stream to reach EOF.
    pub async fn finish(mut self) -> Result<()> {
        self.draining.cancel();
        let Some(mut task) = self.task.take() else {
            return Ok(());
        };

        match tokio::time::timeout(Duration::from_secs(5), &mut task).await {
            Ok(result) => result.context("joining MoQ importer"),
            Err(_) => {
                self.stopped.cancel();
                task.abort();
                let _ = task.await;
                Err(anyhow::anyhow!(
                    "MoQ importer did not drain within five seconds"
                ))
            }
        }
    }
}

impl Drop for Attachment {
    fn drop(&mut self) {
        if let Some(status) = &self.status {
            let mut status = status.lock().unwrap_or_else(PoisonError::into_inner);
            if status.phase != "failed" {
                status.phase = "stopped";
            }
        }
        self.stopped.cancel();

        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

/// Lazily prepares the local importer on the delivery worker's retry schedule.
/// The playout task retains this handle until all encoded output has drained.
#[derive(Clone)]
pub struct OutputAttachment(Arc<Endpoint>);

struct Endpoint {
    channel: i32,
    audio_level: AudioMeterSource,
    runtime: tokio::runtime::Handle,
    attached: Mutex<Option<(String, Attachment)>>,
}

impl std::fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MoqEndpoint")
            .field("channel", &self.channel)
            .finish_non_exhaustive()
    }
}

impl ff_engine::OutputEndpoint for Endpoint {
    fn connect_url(&self) -> Result<String> {
        self.connect_with(|| {
            self.audio_level
                .first_audio_pts
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take();
            self.runtime
                .block_on(attach_output(self.channel, self.audio_level.clone()))
        })
    }

    fn audio_started(&self, timestamp_us: i64) {
        *self
            .audio_level
            .first_audio_pts
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(timestamp_us);
    }

    fn disconnect(&self) {
        let attachment = self
            .attached
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some((_, attachment)) = attachment
            && let Err(error) = self.runtime.block_on(attachment.finish())
        {
            log::warn!(channel = self.channel; "MoQ output shutdown: {error}");
        }
    }
}

impl Endpoint {
    fn connect_with(
        &self,
        attach: impl FnOnce() -> Result<(String, Attachment)>,
    ) -> Result<String> {
        let mut current = self.attached.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((url, attachment)) = current.as_ref()
            && attachment
                .task
                .as_ref()
                .is_some_and(|task| !task.is_finished())
        {
            return Ok(url.clone());
        }
        // Drop a terminated importer before reusing its broadcast identity.
        current.take();
        let (url, attachment) = attach()?;
        *current = Some((url.clone(), attachment));

        Ok(url)
    }
}

impl OutputAttachment {
    pub(crate) fn new(channel: i32, audio_level: AudioMeterSource) -> Self {
        Self(Arc::new(Endpoint {
            channel,
            audio_level,
            runtime: tokio::runtime::Handle::current(),
            attached: Mutex::new(None),
        }))
    }

    pub fn endpoint(&self) -> Arc<dyn ff_engine::OutputEndpoint> {
        self.0.clone()
    }

    pub async fn finish(self) -> Result<()> {
        let attachment = self
            .0
            .attached
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some((_, attachment)) = attachment {
            attachment.finish().await?;
        }

        Ok(())
    }
}

/// Attach a regular MoQ destination to the channel's browser broadcast.
async fn attach_output(id: i32, audio_level: AudioMeterSource) -> Result<(String, Attachment)> {
    let runtime = RUNTIME
        .get()
        .context("MoQ output requires ffplayout --listen")?;
    attach_to_origin(id, runtime.origin.clone(), audio_level).await
}

async fn attach_to_origin(
    id: i32,
    origin: moq_net::origin::Producer,
    audio_level: AudioMeterSource,
) -> Result<(String, Attachment)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("tcp://{}?tcp_nodelay=1", listener.local_addr()?);
    let stopped = CancellationToken::new();
    let token = stopped.clone();
    let draining = CancellationToken::new();
    let stop_accepting = draining.clone();
    let status = Arc::new(Mutex::new(ImportStatus {
        phase: "starting",
        error: None,
    }));
    STATUSES
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(id, status.clone());
    let task_status = status.clone();
    let task = tokio::spawn(async move {
        let result = async {
            loop {
                let accepted = tokio::select! {
                    result = listener.accept() => Some(result?),
                    _ = token.cancelled() => None,
                    _ = stop_accepting.cancelled() => None,
                };

                let Some((mut stream, _)) = accepted else {
                    break;
                };
                let connection = async {
                let mut broadcast =
                    origin.create_broadcast(format!("channels/channel-{id}.hang"))?;
                let config = catalog::Config::default()
                    .with_max_age(RETENTION)
                    .with_catalog(catalog::hang::Catalog::<ts::Ext>::default());
                let mut catalog = catalog::Producer::new(&mut broadcast, config)?;
                let mut import = ts::Import::new(broadcast.clone(), catalog.reserve()).live();
                let mut audio_levels = broadcast.create_track(
                    AUDIO_LEVEL_TRACK,
                    moq_net::track::Info::default().with_max_age(Duration::ZERO),
                )?;
                let mut loudness_track = broadcast.create_track(
                    LOUDNESS_TRACK,
                    moq_net::track::Info::default().with_max_age(Duration::ZERO),
                )?;
                let mut loudness_subscription = LoudnessSubscription {
                    control: audio_level.loudness.clone(),
                    active: false,
                };
                let mut audio_clock = AudioClock::default();
                let mut meter_tick = interval(AUDIO_LEVEL_INTERVAL);
                meter_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
                broadcast.announce(Default::default())?;
                {
                    *task_status.lock().unwrap_or_else(PoisonError::into_inner) = ImportStatus {
                        phase: "starting",
                        error: None,
                    };
                    let mut buffer = [0_u8; 32768];
                    let measure_timing = log::log_enabled!(log::Level::Trace);
                    let mut reported = Instant::now();
                    let mut previous_read: Option<Instant> = None;
                    let mut read_gap_max = Duration::ZERO;
                    let mut import_max = Duration::ZERO;
                    let mut bytes = 0_u64;
                    let mut reads = 0_u64;
                    let mut received_media = false;

                    loop {
                        let count = tokio::select! {
                            result = stream.read(&mut buffer) => result?,
                            _ = meter_tick.tick() => {
                                publish_audio_level(&mut audio_levels, &audio_level, audio_clock.offset_us)?;
                                loudness_subscription.update(loudness_track.is_used());
                                publish_loudness(&mut loudness_track, &audio_level)?;
                                continue;
                            },
                            _ = token.cancelled() => break,
                        };

                        if count == 0 {
                            break;
                        }

                        let read_at = measure_timing.then(Instant::now);
                        // Publish track discovery and the decoder hint together, so
                        // browsers do not open a throughput-oriented decoder first.
                        let reserved = catalog.reserve();
                        import.decode(&buffer[..count])?;
                        let mut update = catalog.modify()?;

                        if update
                            .video
                            .renditions
                            .values()
                            .any(|video| video.optimize_for_latency != Some(true))
                        {
                            configure_catalog(&mut update);
                        }
                        let audio_track = if audio_clock.offset_us.is_none() {
                            update.audio.renditions.keys().next().cloned()
                        } else {
                            None
                        };
                        update.commit()?;
                        drop(reserved);
                        if let Some(name) = audio_track {
                            // Meter synchronization must never stall or fail media delivery.
                            if audio_clock.observe(&broadcast, &name, &audio_level).is_err() {
                                audio_clock = AudioClock::default();
                            }
                        }
                        // Report the actual importer receiving media, for the configured output.
                        if !received_media {
                            task_status
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner)
                                .phase = "running";
                            received_media = true;
                        }

                        if let Some(read_at) = read_at {
                            if let Some(previous) = previous_read {
                                read_gap_max =
                                    read_gap_max.max(read_at.saturating_duration_since(previous));
                            }
                            previous_read = Some(read_at);
                            let now = Instant::now();
                            import_max = import_max.max(now.saturating_duration_since(read_at));
                            bytes += count as u64;
                            reads += 1;
                            let window = now.saturating_duration_since(reported);

                            if window >= Duration::from_secs(5) {
                                log::trace!(channel = id;
                                    "[Preview Timing] stage=importer window_s={:.3} reads={reads} bytes={bytes} tcp_read_gap_max_ms={:.3} import_call_max_ms={:.3}",
                                    window.as_secs_f64(), read_gap_max.as_secs_f64() * 1000.0, import_max.as_secs_f64() * 1000.0,
                                );
                                reported = now;
                                read_gap_max = Duration::ZERO;
                                import_max = Duration::ZERO;
                                bytes = 0;
                                reads = 0;
                            }
                        }
                    }
                }
                loudness_track.finish()?;
                audio_levels.finish()?;
                import.finish()?;
                catalog.finish()?;
                Ok::<(), anyhow::Error>(())
                }.await;
                let mut state = task_status.lock().unwrap_or_else(PoisonError::into_inner);
                match connection {
                    Ok(()) => state.phase = "starting",
                    Err(error) => {
                        state.phase = "failed";
                        state.error = Some(format!("{error:#}"));
                        log::warn!(channel = id; "MoQ import connection failed: {error:#}");
                    }
                }
            }
            Ok::<(), anyhow::Error>(())
        };

        let result = result.await;
        let mut status = task_status.lock().unwrap_or_else(PoisonError::into_inner);
        match result {
            Ok(()) => status.phase = "stopped",
            Err(error) => {
                status.phase = "failed";
                status.error = Some(format!("{error:#}"));
                log::warn!(channel = id; "MoQ importer stopped: {error}");
            }
        }
    });

    Ok((
        url,
        Attachment {
            stopped,
            draining,
            task: Some(task),
            status: Some(status),
        },
    ))
}

/// Resolve the program-to-MoQ offset once per connection from the exact first
/// encoded audio packet and its first imported frame. This includes encoder
/// priming, MPEG-TS mux shifts and the importer's live clock re-anchoring.
#[derive(Default)]
struct AudioClock {
    subscriber: Option<moq_net::track::Subscriber>,
    group: Option<moq_net::group::Consumer>,
    offset_us: Option<i64>,
}

impl AudioClock {
    fn observe(
        &mut self,
        broadcast: &moq_net::broadcast::Producer,
        name: &str,
        source: &AudioMeterSource,
    ) -> Result<()> {
        let Some(source_pts) = *source
            .first_audio_pts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
        else {
            return Ok(());
        };
        if self.subscriber.is_none() {
            let track = broadcast.consume().track(name)?;
            // Include the initial group even if a read delivered several AAC frames.
            let subscription = moq_net::track::Subscription::default().with_max_age(RETENTION);
            let Some(subscriber) = track.subscribe(subscription).now_or_never() else {
                return Ok(());
            };
            let mut subscriber = subscriber?;
            // Never derive an offset from a later group if the first was evicted.
            subscriber.set_groups(0..=0);
            self.subscriber = Some(subscriber);
        }
        let subscriber = self
            .subscriber
            .as_mut()
            .expect("audio subscriber was just initialized");
        if self.group.is_none() {
            let Some(group) = subscriber.recv_group().now_or_never() else {
                return Ok(());
            };
            self.group = group?;
        }
        let Some(group) = self.group.as_mut() else {
            return Ok(());
        };
        if let Some(frame) = group.read_frame().now_or_never() {
            if let Some(frame) = frame? {
                self.offset_us = i64::try_from(frame.timestamp.as_micros())
                    .ok()
                    .and_then(|timestamp| timestamp.checked_sub(source_pts));
            }
            self.group = None;
            self.subscriber = None;
        }

        Ok(())
    }
}

fn publish_audio_level(
    track: &mut moq_net::track::Producer,
    source: &AudioMeterSource,
    offset_us: Option<i64>,
) -> Result<()> {
    // A public player does not subscribe. Do not serialize or retain samples
    // unless a player-page meter is actually consuming this track.
    if !track.is_used() {
        return Ok(());
    }
    let level = *source.level.lock().unwrap_or_else(PoisonError::into_inner);
    let payload = level.map(|level| {
        serde_json::json!({
            "rms_db": level.rms_db,
            "peak_db": level.peak_db,
            "timestamp_us": level.timestamp_us.zip(offset_us).and_then(|(pts, offset)| pts.checked_add(offset)),
        })
    });
    track.write_frame(moq_net::Timestamp::now(), serde_json::to_vec(&payload)?)?;

    Ok(())
}

fn publish_loudness(track: &mut moq_net::track::Producer, source: &AudioMeterSource) -> Result<()> {
    if !track.is_used() {
        return Ok(());
    }
    let payload = source.loudness.metrics().map(|metrics| {
        serde_json::json!({
            "momentary_lufs": metrics.momentary_lufs,
            "short_term_lufs": metrics.short_term_lufs,
            "integrated_lufs": metrics.integrated_lufs,
            "true_peak_dbtp": metrics.true_peak_dbtp,
        })
    });
    track.write_frame(moq_net::Timestamp::now(), serde_json::to_vec(&payload)?)?;

    Ok(())
}

pub(super) fn configure_catalog(catalog: &mut catalog::hang::Catalog<ts::Ext>) {
    for video in catalog.video.renditions.values_mut() {
        video.optimize_for_latency = Some(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use tokio::sync::oneshot;

    #[test]
    fn audio_clock_maps_first_codec_pts_to_imported_pts_without_retaining_media() {
        let broadcast = moq_net::broadcast::Producer::new(Default::default());
        let mut audio = broadcast
            .create_track("1.aac", hang::container::track_info(80))
            .unwrap();
        audio
            .write_frame(
                moq_net::Timestamp::from_micros(10_000_000).unwrap(),
                vec![1],
            )
            .unwrap();
        audio
            .write_frame(
                moq_net::Timestamp::from_micros(10_021_333).unwrap(),
                vec![2],
            )
            .unwrap();
        let source = AudioMeterSource::default();
        *source.first_audio_pts.lock().unwrap() = Some(-21_333);
        let mut clock = AudioClock::default();
        clock.observe(&broadcast, "1.aac", &source).unwrap();
        assert_eq!(clock.offset_us, Some(10_021_333));
        assert!(clock.subscriber.is_none());
        assert!(clock.group.is_none());
        assert!(!audio.is_used());
    }

    #[test]
    fn loudness_demand_is_balanced_on_disconnect_and_importer_drop() {
        let control = ff_engine::LoudnessMeterControl::default();
        {
            let mut demand = LoudnessSubscription {
                control: control.clone(),
                active: false,
            };
            assert!(!control.active());
            demand.update(true);
            demand.update(true);
            assert!(control.active());
            demand.update(false);
            assert!(!control.active());
            demand.update(true);
        }
        assert!(!control.active());
        // Ending a MoQ subscription must not remove a concurrent SSE subscriber.
        control.subscribe();
        {
            let mut demand = LoudnessSubscription {
                control: control.clone(),
                active: false,
            };
            demand.update(true);
        }
        assert!(control.active());
        control.unsubscribe();
        assert!(!control.active());
    }

    #[tokio::test]
    async fn audio_level_track_is_on_demand_and_carries_current_program_levels() {
        let broadcast = moq_net::broadcast::Producer::new(Default::default());
        let mut track = broadcast
            .create_track(
                AUDIO_LEVEL_TRACK,
                moq_net::track::Info::default().with_max_age(Duration::ZERO),
            )
            .unwrap();
        let source = AudioMeterSource::new(
            Arc::new(Mutex::new(Some(ff_engine::AudioLevel {
                rms_db: -24.5,
                peak_db: -6.25,
                timestamp_us: Some(250_000),
            }))),
            Default::default(),
        );
        publish_audio_level(&mut track, &source, Some(10_000_000)).unwrap();
        let mut subscriber = track.subscribe(None);
        assert_eq!(subscriber.latest(), None);
        publish_audio_level(&mut track, &source, Some(10_000_000)).unwrap();
        let mut group = subscriber.recv_group().await.unwrap().unwrap();
        let frame = group.read_frame().await.unwrap().unwrap();
        let value: serde_json::Value = serde_json::from_slice(&frame.payload).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"rms_db": -24.5, "peak_db": -6.25, "timestamp_us": 10_250_000})
        );
        assert!(group.read_frame().await.unwrap().is_none());
        *source.level.lock().unwrap() = None;
        publish_audio_level(&mut track, &source, Some(10_000_000)).unwrap();
        let mut group = subscriber.recv_group().await.unwrap().unwrap();
        let frame = group.read_frame().await.unwrap().unwrap();
        assert_eq!(&frame.payload[..], b"null");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn hls_and_moq_can_toggle_independently_with_a_shared_encoder() {
        use ff_engine::{
            HlsSubtitle, LogoFade, OutputConfig, OutputDestination, OutputTarget, Playout,
            StreamType,
        };
        use std::sync::atomic::AtomicUsize;

        struct LocalEndpoint {
            endpoint: Endpoint,
            origin: moq_net::origin::Producer,
            connections: AtomicUsize,
            disconnects: AtomicUsize,
        }
        impl std::fmt::Debug for LocalEndpoint {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_struct("LocalEndpoint").finish_non_exhaustive()
            }
        }
        impl ff_engine::OutputEndpoint for LocalEndpoint {
            fn connect_url(&self) -> Result<String> {
                self.connections.fetch_add(1, Ordering::SeqCst);
                self.endpoint.connect_with(|| {
                    self.endpoint.runtime.block_on(attach_to_origin(
                        -98770,
                        self.origin.clone(),
                        self.endpoint.audio_level.clone(),
                    ))
                })
            }

            fn audio_started(&self, timestamp_us: i64) {
                self.endpoint.audio_started(timestamp_us);
            }
            fn disconnect(&self) {
                self.disconnects.fetch_add(1, Ordering::SeqCst);
                ff_engine::OutputEndpoint::disconnect(&self.endpoint);
            }
        }
        let directory =
            std::env::temp_dir().join(format!("moq-live-output-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let media = directory.join("clip.mp4");
        std::fs::copy(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests_assets/storage/media_mix/with_audio.mp4"),
            &media,
        )
        .unwrap();
        std::fs::write(
            media.with_extension("vtt"),
            "WEBVTT\n\n00:00:00.000 --> 00:00:20.000\nShared encoder remains active\n",
        )
        .unwrap();
        let (origin, driver) = moq_net::origin::Producer::new(Default::default());
        let driver = tokio::spawn(moq_net::time::run(driver));
        let endpoint = Arc::new(LocalEndpoint {
            endpoint: Endpoint {
                channel: -98770,
                audio_level: Default::default(),
                runtime: tokio::runtime::Handle::current(),
                attached: Mutex::new(None),
            },
            origin,
            connections: AtomicUsize::new(0),
            disconnects: AtomicUsize::new(0),
        });
        let observer = endpoint.origin.consume();
        let measured_level = endpoint.endpoint.audio_level.level.clone();
        let mut config = OutputConfig::new(160, 90, 25, 48_000);
        config.loudness_meter_control = endpoint.endpoint.audio_level.loudness.clone();
        config.audio_level_callback = Some(ff_engine::AudioLevelCallback::new(move |level| {
            *measured_level.lock().unwrap() = Some(level);
        }));
        config.stream_type = StreamType::Custom;
        config.stream_format = "mpegts".into();
        config
            .video_options
            .insert("preset".into(), "ultrafast".into());
        config
            .video_options
            .insert("tune".into(), "zerolatency".into());
        config.encoding_revision = "hls-owner".into();
        let mut hls = OutputDestination {
            id: 1,
            config: config.clone(),
            target: OutputTarget::Hls {
                playlist: directory.join("index.m3u8").to_string_lossy().into_owned(),
                variants: vec![],
                subtitle: Some(HlsSubtitle {
                    name: "Subtitles".into(),
                    language: "en".into(),
                    default: false,
                }),
                segment_seconds: 1,
                list_size: 6,
            },
        };
        hls.config.delivery_enabled = false;
        let mut borrowed = config.clone();
        borrowed.encoding_source = Some((1, 0));
        borrowed.stream_endpoint = Some(endpoint.clone());
        let moq = OutputDestination {
            id: 5,
            config: borrowed,
            target: OutputTarget::Stream { url: String::new() },
        };
        config.destinations = vec![hls.clone(), moq.clone()];
        let worker_directory = directory.clone();
        let worker = tokio::task::spawn_blocking(move || {
            let status = config.output_status.clone();
            let mut playout = Playout::open_parallel(config, 1.0).unwrap();
            for round in 0..4 {
                if round == 1 {
                    hls.config.delivery_enabled = true;
                    status
                        .update_outputs(vec![hls.clone(), moq.clone()])
                        .unwrap();
                } else if round == 2 {
                    status.update_outputs(vec![hls.clone()]).unwrap();
                } else if round == 3 {
                    status
                        .update_outputs(vec![hls.clone(), moq.clone()])
                        .unwrap();
                }
                assert_eq!(
                    playout
                        .play_timed_with_live(
                            media.to_str().unwrap(),
                            Some(1.0),
                            Some(2.0),
                            None,
                            media.to_str(),
                            LogoFade::default(),
                            1.0,
                            &mut None
                        )
                        .unwrap(),
                    ff_engine::ClipResult::Played
                );
                assert_eq!(
                    endpoint.connections.load(Ordering::SeqCst),
                    if round == 3 { 2 } else { 1 }
                );
                if round >= 2 {
                    assert_eq!(endpoint.disconnects.load(Ordering::SeqCst), 1);
                }
                if round >= 1 {
                    assert_eq!(status.snapshot()[&1].phase, "running");
                    assert!(worker_directory.join("index.m3u8").exists());
                }
                assert_eq!(
                    status.snapshot()[&5].phase,
                    if round == 2 { "stopped" } else { "running" }
                );
            }
            playout.finish().unwrap();
            assert_eq!(endpoint.disconnects.load(Ordering::SeqCst), 2);
        });
        // Exercise the actual encoder -> delivery -> MPEG-TS -> live importer mapping.
        // The meter must name the same timeline as the imported AAC, not process uptime.
        tokio::time::timeout(Duration::from_secs(4), async {
            let broadcast = loop {
                if let Ok(broadcast) = observer
                    .request_broadcast("channels/channel--98770.hang")
                    .await
                {
                    break broadcast;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            };
            let mut catalogs = broadcast
                .track("catalog.json")
                .unwrap()
                .subscribe(None)
                .await
                .unwrap();
            let audio_name = loop {
                let mut group = catalogs.recv_group().await.unwrap().unwrap();
                let frame = group.read_frame().await.unwrap().unwrap();
                let value: serde_json::Value = serde_json::from_slice(&frame.payload).unwrap();
                if let Some(name) = value["audio"]["renditions"]
                    .as_object()
                    .and_then(|renditions| renditions.keys().next())
                {
                    break name.clone();
                }
            };
            let mut audio = broadcast
                .track(&audio_name)
                .unwrap()
                .subscribe(None)
                .await
                .unwrap();
            let mut audio_group = audio.recv_group().await.unwrap().unwrap();
            let first_audio = audio_group
                .read_frame()
                .await
                .unwrap()
                .unwrap()
                .timestamp
                .as_micros();
            let mut loudness = broadcast
                .track(LOUDNESS_TRACK)
                .unwrap()
                .subscribe(None)
                .await
                .unwrap();
            let mut meter = broadcast
                .track(AUDIO_LEVEL_TRACK)
                .unwrap()
                .subscribe(None)
                .await
                .unwrap();
            let timestamp = loop {
                let mut group = meter.recv_group().await.unwrap().unwrap();
                let frame = group.read_frame().await.unwrap().unwrap();
                let value: serde_json::Value = serde_json::from_slice(&frame.payload).unwrap();
                if let Some(timestamp) = value["timestamp_us"].as_u64() {
                    break timestamp;
                }
            };
            assert!(u128::from(timestamp) >= first_audio);
            assert!(u128::from(timestamp) - first_audio < 1_000_000);
            loop {
                let mut group = loudness.recv_group().await.unwrap().unwrap();
                let frame = group.read_frame().await.unwrap().unwrap();
                let value: serde_json::Value = serde_json::from_slice(&frame.payload).unwrap();
                if value["short_term_lufs"].as_f64().is_some() {
                    break;
                }
            }
        })
        .await
        .expect("synchronized audio meter was not published");
        worker.await.unwrap();
        driver.abort();
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn failed_importer_initialization_can_be_retried_and_recreated() {
        let attachment = OutputAttachment::new(-98766, Default::default());
        let endpoint = attachment.0.clone();
        assert!(
            endpoint
                .connect_with(|| Err(anyhow::anyhow!("temporary bind failure")))
                .is_err()
        );
        assert!(endpoint.attached.lock().unwrap().is_none());
        let (origin, _driver) = moq_net::origin::Producer::new(Default::default());
        let first = attach_to_origin(-98766, origin.clone(), Default::default())
            .await
            .unwrap();
        let url = endpoint.connect_with(|| Ok(first)).unwrap();
        assert_eq!(
            endpoint
                .connect_with(|| panic!("live importer must be reused"))
                .unwrap(),
            url
        );
        let (url, mut first) = endpoint.attached.lock().unwrap().take().unwrap();
        first.stopped.cancel();
        first.task.as_mut().unwrap().await.unwrap();
        *endpoint.attached.lock().unwrap() = Some((url, first));
        let second = attach_to_origin(-98766, origin, Default::default())
            .await
            .unwrap();
        let expected = second.0.clone();
        assert_eq!(endpoint.connect_with(|| Ok(second)).unwrap(), expected);
        attachment.finish().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shared_hls_and_moq_write_media_after_seeking_inside_a_subtitle() {
        use ff_engine::{
            HlsSubtitle, LogoFade, OutputConfig, OutputDestination, OutputTarget, Playout,
            StreamType,
        };
        use std::path::Path;

        let directory = std::env::temp_dir().join(format!("hls-moq-vtt-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let media = directory.join("clip.mp4");
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests_assets/storage/media_mix/av_sync.mp4");
        std::fs::copy(fixture, &media).unwrap();
        std::fs::write(
            media.with_extension("vtt"),
            "WEBVTT\n\n00:00:00.000 --> 00:00:20.000\nCue spanning the seek point\n",
        )
        .unwrap();
        let (origin, _driver) = moq_net::origin::Producer::new(Default::default());
        let (url, attachment) = attach_to_origin(-98768, origin, Default::default())
            .await
            .unwrap();
        let mut config = OutputConfig::new(320, 180, 25, 48000);
        config
            .video_options
            .insert("preset".into(), "ultrafast".into());
        config.stream_type = StreamType::Custom;
        config.stream_format = "mpegts".into();
        config.destinations.push(OutputDestination {
            id: 1,
            config: config.clone(),
            target: OutputTarget::Hls {
                playlist: directory.join("index.m3u8").to_string_lossy().into_owned(),
                variants: vec![],
                subtitle: Some(HlsSubtitle {
                    name: "Subtitles".into(),
                    language: "en".into(),
                    default: false,
                }),
                segment_seconds: 2,
                list_size: 6,
            },
        });
        let mut borrowed = config.clone();
        borrowed.destinations.clear();
        borrowed.encoding_source = Some((1, 0));
        config.destinations.push(OutputDestination {
            id: 5,
            config: borrowed,
            target: OutputTarget::Stream { url },
        });
        let status = config.output_status.clone();
        let output_directory = directory.clone();
        let worker = tokio::task::spawn_blocking(move || {
            let mut playout = Playout::open_parallel(config, 1.0).unwrap();
            let result = playout
                .play_timed_with_live(
                    media.to_str().unwrap(),
                    Some(12.0),
                    Some(6.0),
                    None,
                    media.to_str(),
                    LogoFade::default(),
                    1.0,
                    &mut None,
                )
                .unwrap();
            assert_eq!(result, ff_engine::ClipResult::Played);
            // Check before finish: flushing a broken mux on shutdown can conceal
            // that the live playlist never received any media during playback.
            assert!(
                std::fs::read_dir(&output_directory)
                    .unwrap()
                    .flatten()
                    .any(|entry| {
                        entry
                            .path()
                            .extension()
                            .is_some_and(|extension| extension == "ts")
                            && entry.metadata().unwrap().len() > 0
                    }),
                "HLS did not write live video segments"
            );
            for id in [1, 5] {
                assert_eq!(status.snapshot()[&id].phase, "running");
            }

            let playlist = output_directory.join("index.m3u8");
            let before = std::fs::read_to_string(&playlist).unwrap();
            std::fs::remove_file(media.with_extension("vtt")).unwrap();
            assert_eq!(
                playout
                    .play_timed_with_live(
                        media.to_str().unwrap(),
                        None,
                        Some(6.0),
                        None,
                        media.to_str(),
                        LogoFade::default(),
                        1.0,
                        &mut None,
                    )
                    .unwrap(),
                ff_engine::ClipResult::Played
            );
            assert_ne!(std::fs::read_to_string(playlist).unwrap(), before);
            for id in [1, 5] {
                assert_eq!(status.snapshot()[&id].phase, "running");
            }

            playout.finish().unwrap();
        });
        worker.await.unwrap();
        attachment.finish().await.unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn importer_accepts_another_connection_after_a_broken_stream() {
        use tokio::{io::AsyncWriteExt, net::TcpStream};
        let (origin, _driver) = moq_net::origin::Producer::new(Default::default());
        let channel = -98765;
        let (url, attachment) = attach_to_origin(channel, origin, Default::default())
            .await
            .unwrap();
        let address = url
            .strip_prefix("tcp://")
            .unwrap()
            .split('?')
            .next()
            .unwrap();
        for _ in 0..2 {
            let mut stream = TcpStream::connect(address).await.unwrap();
            stream.write_all(&[0_u8; 376]).await.unwrap();
            stream.shutdown().await.unwrap();
            // EOF from the server proves this import finished before the next connection.
            let mut response = Vec::new();
            tokio::time::timeout(Duration::from_secs(2), stream.read_to_end(&mut response))
                .await
                .unwrap()
                .unwrap();
        }
        attachment.finish().await.unwrap();
    }

    #[test]
    fn dropping_importer_updates_status_without_erasing_errors() {
        for phase in ["running", "failed"] {
            let status = Arc::new(Mutex::new(ImportStatus {
                phase,
                error: (phase == "failed").then(|| "broken stream".into()),
            }));
            let attachment = Attachment {
                draining: CancellationToken::new(),
                stopped: CancellationToken::new(),
                task: None,
                status: Some(status.clone()),
            };
            drop(attachment);
            let state = status.lock().unwrap();
            assert_eq!(
                state.phase,
                if phase == "failed" {
                    "failed"
                } else {
                    "stopped"
                }
            );
            assert_eq!(
                state.error.as_deref(),
                (phase == "failed").then_some("broken stream")
            );
        }
    }

    #[tokio::test]
    async fn normal_finish_waits_for_importer_eof_before_cancelling() {
        let stopped = CancellationToken::new();
        let token = stopped.clone();
        let (eof, reader) = oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            tokio::select! {
                result = reader => { result.unwrap(); },
                _ = token.cancelled() => panic!("normal finish must allow the importer to drain"),
            }
        });
        let attachment = Attachment {
            draining: CancellationToken::new(),
            stopped,
            task: Some(task),
            status: None,
        };
        let finish = tokio::spawn(attachment.finish());
        tokio::task::yield_now().await;
        assert!(!finish.is_finished());
        eof.send(()).unwrap();
        finish.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn dropping_attachment_aborts_its_importer_task() {
        struct Dropped(Arc<AtomicBool>);
        impl Drop for Dropped {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }

        let dropped = Arc::new(AtomicBool::new(false));
        let marker = Dropped(dropped.clone());
        let task = tokio::spawn(async move {
            let _marker = marker;
            std::future::pending::<()>().await;
        });
        let stopped = CancellationToken::new();
        let attachment = Attachment {
            draining: CancellationToken::new(),
            stopped: stopped.clone(),
            task: Some(task),
            status: None,
        };
        drop(attachment);
        tokio::task::yield_now().await;
        assert!(stopped.is_cancelled());
        assert!(dropped.load(Ordering::Acquire));
    }
}
