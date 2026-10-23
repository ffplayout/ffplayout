use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock, PoisonError},
    time::Duration,
};

use anyhow::{Context, Result};
use hang::moq_net;
use tokio::time::{MissedTickBehavior, interval};
use tokio_util::sync::CancellationToken;

use super::{packets::Publisher, relay::RUNTIME};

#[derive(Clone)]
pub struct PublicationStatus {
    pub phase: &'static str,
    pub error: Option<String>,
}

#[derive(Clone, Default)]
pub(crate) struct AudioMeterSource {
    level: Arc<Mutex<Option<ff_engine::AudioLevel>>>,
    loudness: ff_engine::LoudnessMeterControl,
}

impl AudioMeterSource {
    pub(crate) fn new(
        level: Arc<Mutex<Option<ff_engine::AudioLevel>>>,
        loudness: ff_engine::LoudnessMeterControl,
    ) -> Self {
        Self { level, loudness }
    }
}

// Loudness analysis runs only while its MoQ track has subscribers.
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
pub(super) type StatusSlot = Arc<Mutex<PublicationStatus>>;
static STATUSES: OnceLock<Mutex<BTreeMap<i32, StatusSlot>>> = OnceLock::new();

pub fn status(channel: i32) -> Option<PublicationStatus> {
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

/// The packet worker owns media; this handle owns only the auxiliary meter task.
pub struct Attachment {
    stopped: CancellationToken,
    task: Option<tokio::task::JoinHandle<()>>,
    status: StatusSlot,
}

impl Attachment {
    pub async fn finish(mut self) -> Result<()> {
        self.stopped.cancel();
        if let Some(task) = self.task.take() {
            task.await.context("joining MoQ meter publisher")?;
        }
        Ok(())
    }
}

impl Drop for Attachment {
    fn drop(&mut self) {
        let mut status = self.status.lock().unwrap_or_else(PoisonError::into_inner);
        if status.phase != "failed" {
            status.phase = "stopped";
        }
        self.stopped.cancel();
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

#[derive(Clone)]
pub struct OutputAttachment(Arc<Endpoint>);

struct Endpoint {
    channel: i32,
    audio_level: AudioMeterSource,
    runtime: tokio::runtime::Handle,
    attached: Mutex<Option<Attachment>>,
}

impl std::fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MoqEndpoint")
            .field("channel", &self.channel)
            .finish_non_exhaustive()
    }
}

impl ff_engine::OutputEndpoint for Endpoint {
    fn connect_packets(
        &self,
        streams: &[ff_engine::EncodedStream],
    ) -> Result<Box<dyn ff_engine::PacketSink>> {
        let runtime = RUNTIME
            .get()
            .context("MoQ output requires ffplayout --listen")?;
        self.connect_to_origin(runtime.origin.clone(), streams)
    }

    fn disconnect(&self) {
        // Media has already been drained/joined by the engine. Stop its meter task.
        let attachment = self
            .attached
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(attachment) = attachment
            && let Err(error) = self.runtime.block_on(attachment.finish())
        {
            log::warn!(channel = self.channel; "MoQ meter shutdown: {error:#}");
        }
    }
}

impl Endpoint {
    fn connect_to_origin(
        &self,
        origin: moq_net::origin::Producer,
        streams: &[ff_engine::EncodedStream],
    ) -> Result<Box<dyn ff_engine::PacketSink>> {
        let mut attached = self.attached.lock().unwrap_or_else(PoisonError::into_inner);
        attached.take();
        let broadcast = origin
            .create_broadcast(format!("channels/channel-{}.hang", self.channel))
            .context("creating MoQ broadcast")?;
        let offset = Arc::new(Mutex::new(None));
        let status = Arc::new(Mutex::new(PublicationStatus {
            phase: "starting",
            error: None,
        }));
        // Construct/validate codecs before creating the auxiliary task. Failure
        // leaves no live publication or meter task and can be retried independently.
        let stopped = CancellationToken::new();
        let publisher = Publisher::new(
            broadcast.clone(),
            streams,
            offset.clone(),
            status.clone(),
            stopped.clone(),
        )
        .context("initializing MoQ packet publisher")?;
        let mut levels = broadcast.create_track(
            AUDIO_LEVEL_TRACK,
            moq_net::track::Info::default().with_max_age(Duration::ZERO),
        )?;
        let mut loudness = broadcast.create_track(
            LOUDNESS_TRACK,
            moq_net::track::Info::default().with_max_age(Duration::ZERO),
        )?;
        broadcast
            .announce(Default::default())
            .context("announcing MoQ broadcast")?;
        STATUSES
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(self.channel, status.clone());
        let token = stopped.clone();
        let source = self.audio_level.clone();
        let task_status = status.clone();
        let task = self.runtime.spawn(async move {
            let mut demand = LoudnessSubscription {
                control: source.loudness.clone(),
                active: false,
            };
            let mut tick = interval(AUDIO_LEVEL_INTERVAL);
            tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = token.cancelled() => break,
                    _ = tick.tick() => {
                        demand.update(loudness.is_used());
                        let offset = *offset.lock().unwrap_or_else(PoisonError::into_inner);
                        // Meter errors must never interrupt media publication.
                        if let Err(error) = publish_audio_level(&mut levels, &source, offset) {
                            log::trace!("MoQ audio meter publication failed: {error:#}");
                        }
                        if let Err(error) = publish_loudness(&mut loudness, &source) {
                            log::trace!("MoQ loudness publication failed: {error:#}");
                        }
                    }
                }
            }
            let _ = levels.finish();
            let _ = loudness.finish();
            let mut state = task_status.lock().unwrap_or_else(PoisonError::into_inner);
            if state.phase != "failed" {
                state.phase = "stopped";
            }
        });
        *attached = Some(Attachment {
            stopped,
            task: Some(task),
            status,
        });
        Ok(Box::new(publisher))
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
        if let Some(attachment) = attachment {
            attachment.finish().await?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn failed_native_initialization_leaves_no_task_and_can_reconnect() {
        let attachment = OutputAttachment::new(-98766, Default::default());
        let endpoint = attachment.0.clone();
        let (origin, _driver) = moq_net::origin::Producer::new(Default::default());
        let mut streams = vec![
            ff_engine::EncodedStream {
                codec: "h264".into(),
                initialization: vec![],
            },
            ff_engine::EncodedStream {
                codec: "aac".into(),
                initialization: vec![],
            },
        ];
        assert!(
            endpoint
                .connect_to_origin(origin.clone(), &streams)
                .is_err()
        );
        assert!(endpoint.attached.lock().unwrap().is_none());
        streams[1].initialization = vec![0x11, 0x90];
        for _ in 0..2 {
            let mut publisher = endpoint
                .connect_to_origin(origin.clone(), &streams)
                .unwrap();
            publisher
                .write(ff_engine::EncodedFrame {
                    stream: 1,
                    data: &[1, 2],
                    pts_us: -21_333,
                    dts_us: -21_333,
                    duration_us: 21_333,
                })
                .unwrap();
            assert_eq!(status(-98766).unwrap().phase, "running");
            publisher.finish().unwrap();
            drop(publisher);
            let meter = endpoint.attached.lock().unwrap().take().unwrap();
            meter.finish().await.unwrap();
            assert_eq!(status(-98766).unwrap().phase, "stopped");
            assert!(!endpoint.audio_level.loudness.active());
        }
        attachment.finish().await.unwrap();
    }
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
        fn connect_packets(
            &self,
            streams: &[ff_engine::EncodedStream],
        ) -> Result<Box<dyn ff_engine::PacketSink>> {
            self.connections.fetch_add(1, Ordering::SeqCst);
            self.endpoint
                .connect_to_origin(self.origin.clone(), streams)
        }
        fn disconnect(&self) {
            self.disconnects.fetch_add(1, Ordering::SeqCst);
            ff_engine::OutputEndpoint::disconnect(&self.endpoint);
        }
    }

    #[test]
    fn loudness_demand_is_balanced_on_disconnect_and_publisher_drop() {
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
    async fn native_hevc_publishes_decoder_configuration_and_video_frames() {
        if !ff_engine::ffmpeg_capabilities()
            .encoders
            .iter()
            .any(|codec| codec.name == "libx265")
        {
            eprintln!("libx265 unavailable in this FFmpeg build");
            return;
        }
        native_video("libx265", "hev1", -98771).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn native_avc_b_frames_publish_reorder_hint_and_presentation_timestamps() {
        native_video("libx264", "avc3", -98772).await;
    }

    async fn native_video(encoder: &str, codec: &str, channel: i32) {
        let (origin, driver) = moq_net::origin::Producer::new(Default::default());
        let observer = origin.consume();
        let driver = tokio::spawn(moq_net::time::run(driver));
        let endpoint = Arc::new(LocalEndpoint {
            endpoint: Endpoint {
                channel,
                audio_level: Default::default(),
                runtime: tokio::runtime::Handle::current(),
                attached: Mutex::new(None),
            },
            origin,
            connections: AtomicUsize::new(0),
            disconnects: AtomicUsize::new(0),
        });
        let mut config = ff_engine::OutputConfig::new(160, 90, 25, 48_000);
        config.video_codec = encoder.into();
        config.video_options.insert(
            "preset".into(),
            if encoder == "libx264" {
                "superfast"
            } else {
                "ultrafast"
            }
            .into(),
        );
        if encoder == "libx264" {
            config.video_options.insert("tune".into(), "none".into());
        }
        config.stream_endpoint = Some(endpoint.clone());
        config.destinations.push(ff_engine::OutputDestination {
            id: 5,
            config: config.clone(),
            target: ff_engine::OutputTarget::Stream { url: String::new() },
        });
        let worker = tokio::task::spawn_blocking(move || {
            let status = config.output_status.clone();
            let mut playout = ff_engine::Playout::open_parallel(config, 1.0).unwrap();
            let media = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests_assets/storage/media_mix/with_audio.mp4");
            assert_eq!(
                playout
                    .play_timed_with_live(
                        media.to_str().unwrap(),
                        None,
                        Some(5.0),
                        None,
                        None,
                        Default::default(),
                        1.0,
                        &mut None
                    )
                    .unwrap(),
                ff_engine::ClipResult::Played
            );
            assert_eq!(
                status.snapshot()[&5].phase,
                "running",
                "{:?}",
                status.snapshot()[&5]
            );
            playout.finish().unwrap();
        });
        tokio::time::timeout(Duration::from_secs(5), async {
            let broadcast = loop {
                if let Ok(broadcast) = observer
                    .request_broadcast(format!("channels/channel-{channel}.hang"))
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
            let name = loop {
                let mut group = catalogs.recv_group().await.unwrap().unwrap();
                let frame = group.read_frame().await.unwrap().unwrap();
                let value: serde_json::Value = serde_json::from_slice(&frame.payload).unwrap();
                if let Some((name, video)) = value["video"]["renditions"]
                    .as_object()
                    .and_then(|renditions| renditions.iter().next())
                {
                    assert!(video["codec"].as_str().unwrap().starts_with(codec));
                    assert_eq!(video["codedWidth"], 160);
                    assert_eq!(video["codedHeight"], 90);
                    assert_eq!(video["optimizeForLatency"], true);
                    if encoder == "libx264"
                        && !video["jitter"].as_f64().is_some_and(|jitter| jitter > 0.0)
                    {
                        continue;
                    }
                    break name.clone();
                }
            };
            let mut video = broadcast
                .track(&name)
                .unwrap()
                .subscribe(None)
                .await
                .unwrap();
            let mut group = video.recv_group().await.unwrap().unwrap();
            let frame = group.read_frame().await.unwrap().unwrap();
            let media = hang::container::Frame::decode(frame.payload).unwrap();
            assert!(media.payload.len() > 100);
            if encoder == "libx264" {
                let mut times = vec![media.timestamp.as_micros()];
                for _ in 0..4 {
                    let frame = group.read_frame().await.unwrap().unwrap();
                    times.push(
                        hang::container::Frame::decode(frame.payload)
                            .unwrap()
                            .timestamp
                            .as_micros(),
                    );
                }
                assert!(
                    times.windows(2).any(|pair| pair[0] > pair[1]),
                    "B-frame PTS ordering was lost: {times:?}"
                );
            }
        })
        .await
        .expect("native video publication missing");
        worker.await.unwrap();
        assert_eq!(endpoint.connections.load(Ordering::SeqCst), 1);
        assert_eq!(endpoint.disconnects.load(Ordering::SeqCst), 1);
        driver.abort();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn hls_and_moq_can_toggle_independently_with_a_shared_encoder() {
        use ff_engine::{
            HlsSubtitle, LogoFade, OutputConfig, OutputDestination, OutputTarget, Playout,
            StreamType,
        };

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
                    if round == 2 { "stopped" } else { "running" },
                    "MoQ status error: {:?}",
                    status.snapshot()[&5].error
                );
            }
            playout.finish().unwrap();
            assert_eq!(endpoint.disconnects.load(Ordering::SeqCst), 2);
        });
        // Exercise the actual encoder -> paced delivery -> native MoQ tracks mapping.
        // The meter must name the same timeline as the imported AAC, not process uptime.
        tokio::time::timeout(Duration::from_secs(6), async {
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
                let Some(video) = value["video"]["renditions"]
                    .as_object()
                    .and_then(|renditions| renditions.values().next())
                else {
                    continue;
                };
                assert_eq!(video["optimizeForLatency"], true);
                assert_eq!(video["codedWidth"], 160);
                assert_eq!(video["codedHeight"], 90);
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
        let (origin, driver) = moq_net::origin::Producer::new(Default::default());
        let driver = tokio::spawn(moq_net::time::run(driver));
        let endpoint = Arc::new(LocalEndpoint {
            endpoint: Endpoint {
                channel: -98768,
                audio_level: Default::default(),
                runtime: tokio::runtime::Handle::current(),
                attached: Mutex::new(None),
            },
            origin,
            connections: AtomicUsize::new(0),
            disconnects: AtomicUsize::new(0),
        });
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
        borrowed.stream_endpoint = Some(endpoint.clone());
        config.destinations.push(OutputDestination {
            id: 5,
            config: borrowed,
            target: OutputTarget::Stream { url: String::new() },
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
        assert_eq!(endpoint.connections.load(Ordering::SeqCst), 1);
        assert_eq!(endpoint.disconnects.load(Ordering::SeqCst), 1);
        driver.abort();
        std::fs::remove_dir_all(directory).unwrap();
    }
}
