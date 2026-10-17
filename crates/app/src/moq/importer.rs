use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock, PoisonError},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use hang::moq_net;
use moq_mux::{catalog, container::ts};
use tokio::{io::AsyncReadExt, net::TcpListener};
use tokio_util::sync::CancellationToken;

use super::relay::{RETENTION, RUNTIME};

#[derive(Clone)]
pub struct ImportStatus {
    pub phase: &'static str,
    pub error: Option<String>,
}

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
        self.connect_with(|| self.runtime.block_on(attach_output(self.channel)))
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
    pub fn new(channel: i32) -> Self {
        Self(Arc::new(Endpoint {
            channel,
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
pub async fn attach_output(id: i32) -> Result<(String, Attachment)> {
    let runtime = RUNTIME
        .get()
        .context("MoQ output requires ffplayout --listen")?;
    attach_to_origin(id, runtime.origin.clone()).await
}

async fn attach_to_origin(
    id: i32,
    origin: moq_net::origin::Producer,
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
                        update.commit()?;
                        drop(reserved);
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

    #[tokio::test]
    async fn failed_importer_initialization_can_be_retried_and_recreated() {
        let attachment = OutputAttachment::new(-98766);
        let endpoint = attachment.0.clone();
        assert!(
            endpoint
                .connect_with(|| Err(anyhow::anyhow!("temporary bind failure")))
                .is_err()
        );
        assert!(endpoint.attached.lock().unwrap().is_none());
        let (origin, _driver) = moq_net::origin::Producer::new(Default::default());
        let first = attach_to_origin(-98766, origin.clone()).await.unwrap();
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
        let second = attach_to_origin(-98766, origin).await.unwrap();
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
        let (url, attachment) = attach_to_origin(-98768, origin).await.unwrap();
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
        let (url, attachment) = attach_to_origin(channel, origin).await.unwrap();
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
