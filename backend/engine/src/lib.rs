use anyhow::{Context, Result, anyhow};
#[cfg(feature = "tokio")]
use std::{
    sync::mpsc,
    thread::{self, JoinHandle},
};
#[cfg(feature = "tokio")]
use tokio::sync::oneshot;

mod input;
mod output;
mod playout;
mod utils;

use input::live::LiveOverrideOutput;
pub use input::live::{LiveReceiver, spawn_rtmp_listener};
pub use output::resolved_variant_playlist_path;
use output::{FrameOutput, Output, PlaybackStopped};
use playout::{Timeline, play_clip, write_fallback};
pub use utils::{
    clock,
    config::{HlsVariant, LogoConfig, OutputConfig, OutputSize},
    logging,
    media_info::{
        AudioStream as EngineAudioStream, MediaInfo, MediaProbe as EngineMediaProbe, ProbeFormat,
        VideoStream as EngineVideoStream, print_media_info, probe_media, probe_media_info,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipResult {
    Played,
    Fallback { reason: String },
    Stopped,
}

pub struct Playout {
    config: OutputConfig,
    output: Output,
    timeline: Timeline,
    fallback_duration: f64,
}

#[cfg(feature = "tokio")]
pub struct AsyncPlayout {
    commands: mpsc::Sender<AsyncCommand>,
    completion: WorkerCompletion,
}

/// How to wait for the playout worker to finish. Desktop-output sessions run
/// as a job on the shared, process-lifetime SDL thread (see
/// `output::sdl_thread`) instead of on their own dedicated `JoinHandle`, so
/// completion is signalled through a channel instead.
#[cfg(feature = "tokio")]
enum WorkerCompletion {
    Thread(JoinHandle<()>),
    #[cfg(feature = "desktop")]
    SdlThread(oneshot::Receiver<()>),
}

#[cfg(feature = "tokio")]
impl AsyncPlayout {
    pub async fn open(
        output_url: impl Into<String>,
        config: OutputConfig,
        fallback_duration: f64,
    ) -> Result<Self> {
        let output_url = output_url.into();
        Self::open_with(move || Playout::open(&output_url, config, fallback_duration)).await
    }

    pub async fn open_hls(
        playlist: impl Into<String>,
        config: OutputConfig,
        fallback_duration: f64,
        hls_variants: Vec<HlsVariant>,
        hls_vtt_subtitles: bool,
        hls_segment_seconds: u32,
        hls_list_size: u32,
    ) -> Result<Self> {
        let playlist = playlist.into();
        Self::open_with(move || {
            Playout::open_hls(
                &playlist,
                config,
                fallback_duration,
                &hls_variants,
                hls_vtt_subtitles,
                hls_segment_seconds,
                hls_list_size,
            )
        })
        .await
    }

    pub async fn open_null(config: OutputConfig, fallback_duration: f64) -> Result<Self> {
        Self::open_with(move || Playout::open_null(config, fallback_duration)).await
    }

    #[cfg(feature = "desktop")]
    pub async fn open_desktop(config: OutputConfig, fallback_duration: f64) -> Result<Self> {
        let (commands, command_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = oneshot::channel();
        let (done_tx, done_rx) = oneshot::channel();

        output::sdl_thread::spawn(move || {
            match Playout::open_desktop(config, fallback_duration) {
                Ok(playout) => {
                    let _ = ready_tx.send(Ok(()));
                    run_async_playout_worker(playout, command_rx);
                }
                Err(error) => {
                    let _ = ready_tx.send(Err(error));
                }
            }
            let _ = done_tx.send(());
        });

        ready_rx
            .await
            .context("playout worker stopped during open")??;

        Ok(Self {
            commands,
            completion: WorkerCompletion::SdlThread(done_rx),
        })
    }

    async fn open_with<F>(open: F) -> Result<Self>
    where
        F: FnOnce() -> Result<Playout> + Send + 'static,
    {
        let (commands, command_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = oneshot::channel();

        let worker = thread::spawn(move || match open() {
            Ok(playout) => {
                let _ = ready_tx.send(Ok(()));
                run_async_playout_worker(playout, command_rx);
            }
            Err(error) => {
                let _ = ready_tx.send(Err(error));
            }
        });

        ready_rx
            .await
            .context("playout worker stopped during open")??;

        Ok(Self {
            commands,
            completion: WorkerCompletion::Thread(worker),
        })
    }

    pub async fn play(&self, path: impl Into<String>) -> Result<ClipResult> {
        self.play_with_seek(path, None).await
    }

    pub async fn play_with_seek(
        &self,
        path: impl Into<String>,
        seek_seconds: Option<f64>,
    ) -> Result<ClipResult> {
        let path = path.into();
        let (response, result) = oneshot::channel();
        self.commands
            .send(AsyncCommand::Play {
                path,
                seek_seconds,
                duration_seconds: None,
                response,
            })
            .map_err(|_| anyhow!("playout worker stopped"))?;

        result.await.context("playout worker stopped during play")?
    }

    pub async fn play_with_timing(
        &self,
        path: impl Into<String>,
        seek_seconds: Option<f64>,
        duration_seconds: Option<f64>,
    ) -> Result<ClipResult> {
        let path = path.into();
        let (response, result) = oneshot::channel();
        self.commands
            .send(AsyncCommand::Play {
                path,
                seek_seconds,
                duration_seconds,
                response,
            })
            .map_err(|_| anyhow!("playout worker stopped"))?;

        result.await.context("playout worker stopped during play")?
    }

    pub async fn start_rtmp_live(
        &self,
        url: impl Into<String>,
        config: OutputConfig,
    ) -> Result<()> {
        let url = url.into();
        let (response, result) = oneshot::channel();
        self.commands
            .send(AsyncCommand::StartRtmpLive {
                url,
                config,
                response,
            })
            .map_err(|_| anyhow!("playout worker stopped"))?;

        result
            .await
            .context("playout worker stopped while starting RTMP live")?
    }

    pub async fn stop_live(&self) -> Result<()> {
        let (response, result) = oneshot::channel();
        self.commands
            .send(AsyncCommand::StopLive { response })
            .map_err(|_| anyhow!("playout worker stopped"))?;

        result
            .await
            .context("playout worker stopped while stopping live input")?
    }

    pub async fn finish(self) -> Result<()> {
        let (response, result) = oneshot::channel();
        self.commands
            .send(AsyncCommand::Finish { response })
            .map_err(|_| anyhow!("playout worker stopped"))?;

        let finish_result = result
            .await
            .context("playout worker stopped during finish")?;

        match self.completion {
            WorkerCompletion::Thread(worker) => {
                if worker.join().is_err() && finish_result.is_ok() {
                    return Err(anyhow!("playout worker panicked during finish"));
                }
            }
            #[cfg(feature = "desktop")]
            WorkerCompletion::SdlThread(done_rx) => {
                if done_rx.await.is_err() && finish_result.is_ok() {
                    return Err(anyhow!("desktop playout worker stopped unexpectedly"));
                }
            }
        }

        finish_result
    }
}

#[cfg(feature = "tokio")]
enum AsyncCommand {
    Play {
        path: String,
        seek_seconds: Option<f64>,
        duration_seconds: Option<f64>,
        response: oneshot::Sender<Result<ClipResult>>,
    },
    StartRtmpLive {
        url: String,
        config: OutputConfig,
        response: oneshot::Sender<Result<()>>,
    },
    StopLive {
        response: oneshot::Sender<Result<()>>,
    },
    Finish {
        response: oneshot::Sender<Result<()>>,
    },
}

#[cfg(feature = "tokio")]
fn run_async_playout_worker(mut playout: Playout, commands: mpsc::Receiver<AsyncCommand>) {
    let mut live = None;

    while let Ok(command) = commands.recv() {
        match command {
            AsyncCommand::Play {
                path,
                seek_seconds,
                duration_seconds,
                response,
            } => {
                let _ = response.send(playout.play_timed_with_live(
                    &path,
                    seek_seconds,
                    duration_seconds,
                    &mut live,
                ));
            }
            AsyncCommand::StartRtmpLive {
                url,
                config,
                response,
            } => {
                live = Some(spawn_rtmp_listener(url, config));
                let _ = response.send(Ok(()));
            }
            AsyncCommand::StopLive { response } => {
                live = None;
                let _ = response.send(Ok(()));
            }
            AsyncCommand::Finish { response } => {
                let _ = response.send(playout.finish());
                break;
            }
        }
    }
}

impl Playout {
    pub fn open(output_url: &str, config: OutputConfig, fallback_duration: f64) -> Result<Self> {
        Self::validate_fallback_duration(fallback_duration)?;
        init_ffmpeg()?;
        let output = Output::open(output_url, &config)?;

        Ok(Self::with_output(config, output, fallback_duration))
    }

    pub fn open_null(config: OutputConfig, fallback_duration: f64) -> Result<Self> {
        Self::validate_fallback_duration(fallback_duration)?;
        init_ffmpeg()?;
        let output = Output::open_null(&config)?;

        Ok(Self::with_output(config, output, fallback_duration))
    }

    #[cfg(feature = "desktop")]
    pub fn open_desktop(config: OutputConfig, fallback_duration: f64) -> Result<Self> {
        Self::validate_fallback_duration(fallback_duration)?;
        init_ffmpeg()?;
        let output = Output::open_desktop(&config)?;

        Ok(Self::with_output(config, output, fallback_duration))
    }

    pub fn open_hls(
        playlist: &str,
        config: OutputConfig,
        fallback_duration: f64,
        hls_variants: &[HlsVariant],
        hls_vtt_subtitles: bool,
        hls_segment_seconds: u32,
        hls_list_size: u32,
    ) -> Result<Self> {
        Self::validate_fallback_duration(fallback_duration)?;
        init_ffmpeg()?;
        let output = Output::open_hls(
            playlist,
            &config,
            hls_variants,
            hls_vtt_subtitles,
            hls_segment_seconds,
            hls_list_size,
        )?;

        Ok(Self::with_output(config, output, fallback_duration))
    }

    fn validate_fallback_duration(fallback_duration: f64) -> Result<()> {
        if !fallback_duration.is_finite() || fallback_duration <= 0.0 {
            return Err(anyhow!("fallback duration must be a positive number"));
        }
        Ok(())
    }

    fn with_output(config: OutputConfig, output: Output, fallback_duration: f64) -> Self {
        Self {
            config,
            output,
            timeline: Timeline::new(),
            fallback_duration,
        }
    }

    pub fn play(&mut self, path: &str) -> Result<ClipResult> {
        self.play_with_seek(path, None)
    }

    pub fn play_with_seek(&mut self, path: &str, seek_seconds: Option<f64>) -> Result<ClipResult> {
        self.play_timed_with_live(path, seek_seconds, None, &mut None)
    }

    pub fn play_with_live(
        &mut self,
        path: &str,
        seek_seconds: Option<f64>,
        live: &mut Option<LiveReceiver>,
    ) -> Result<ClipResult> {
        self.play_timed_with_live(path, seek_seconds, None, live)
    }

    pub fn play_with_timing(
        &mut self,
        path: &str,
        seek_seconds: Option<f64>,
        duration_seconds: Option<f64>,
    ) -> Result<ClipResult> {
        self.play_timed_with_live(path, seek_seconds, duration_seconds, &mut None)
    }

    pub fn play_timed_with_live(
        &mut self,
        path: &str,
        seek_seconds: Option<f64>,
        duration_seconds: Option<f64>,
        live: &mut Option<LiveReceiver>,
    ) -> Result<ClipResult> {
        #[cfg(feature = "desktop")]
        if self.output.is_desktop() {
            let config = self.config.clone();
            let fallback_duration = self.fallback_duration;
            let mut timeline = self.timeline;
            let path = path.to_string();
            let mut live_for_worker = live.take();
            let operation = self.output.run_desktop(move |output| {
                let result = if let Some(live) = live_for_worker.as_mut() {
                    let mut output = LiveOverrideOutput::new(output, live);
                    play_to_output(
                        &path,
                        &config,
                        &mut timeline,
                        &mut output,
                        fallback_duration,
                        seek_seconds,
                        duration_seconds,
                    )
                } else {
                    play_to_output(
                        &path,
                        &config,
                        &mut timeline,
                        output,
                        fallback_duration,
                        seek_seconds,
                        duration_seconds,
                    )
                };
                (result, timeline, live_for_worker)
            });

            return match operation {
                Ok((result, timeline, live_for_worker)) => {
                    self.timeline = timeline;
                    *live = live_for_worker;
                    result
                }
                Err(error) if error.downcast_ref::<PlaybackStopped>().is_some() => {
                    Ok(ClipResult::Stopped)
                }
                Err(error) => Err(error),
            };
        }

        if let Some(live) = live.as_mut() {
            let mut output = LiveOverrideOutput::new(&mut self.output, live);
            play_to_output(
                path,
                &self.config,
                &mut self.timeline,
                &mut output,
                self.fallback_duration,
                seek_seconds,
                duration_seconds,
            )
        } else {
            play_to_output(
                path,
                &self.config,
                &mut self.timeline,
                &mut self.output,
                self.fallback_duration,
                seek_seconds,
                duration_seconds,
            )
        }
    }

    pub fn finish(self) -> Result<()> {
        self.output.finish()
    }
}

fn init_ffmpeg() -> Result<()> {
    ffmpeg_next::init().context("failed to initialize FFmpeg")?;
    logging::init();
    Ok(())
}

fn play_to_output<O: FrameOutput>(
    path: &str,
    config: &OutputConfig,
    timeline: &mut Timeline,
    output: &mut O,
    fallback_duration: f64,
    seek_seconds: Option<f64>,
    duration_seconds: Option<f64>,
) -> Result<ClipResult> {
    match play_clip(
        path,
        config,
        timeline,
        output,
        seek_seconds,
        duration_seconds,
    ) {
        Ok(()) => Ok(ClipResult::Played),
        Err(error) if error.downcast_ref::<PlaybackStopped>().is_some() => Ok(ClipResult::Stopped),
        Err(error) => {
            let reason = format!("{error:#}");
            let duration = duration_seconds
                .filter(|duration| duration.is_finite() && *duration > 0.0)
                .unwrap_or(fallback_duration);
            write_fallback(config, timeline, output, duration)
                .with_context(|| format!("failed to generate fallback for {path}"))?;
            Ok(ClipResult::Fallback { reason })
        }
    }
}
