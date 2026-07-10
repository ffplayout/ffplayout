use anyhow::{Context, Result, anyhow};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[cfg(feature = "tokio")]
use std::{
    sync::mpsc,
    thread::{self, JoinHandle},
};
#[cfg(feature = "tokio")]
use tokio::sync::oneshot;

mod analysis;
mod audio_mixer;
mod compositor;
mod input;
mod output;
mod playout;
mod utils;

pub use analysis::audio_level::{AudioLevel, AudioLevelCallback};
pub use audio_mixer::AudioEffectsControl;
use input::live::{LiveEnded, LiveOverrideOutput};
pub use input::live::{LiveReceiver, spawn_rtmp_listener};
pub use output::resolved_variant_playlist_path;
use output::{FrameOutput, Output, PlaybackStopped};
use playout::{PlaybackSkipped, Timeline, play_clip, write_fallback};
pub use utils::{
    clock,
    config::{
        HlsSubtitle, HlsVariant, LogLevel, LogoConfig, OutputConfig, OutputSize, RateControl,
        RgbaColor, StreamType, TextBackgroundConfig, TextConfig, TextOverlayState, TextPosition,
        TextScroll, TextWeight,
    },
    ffmpeg_capabilities::{
        FfmpegCapabilities, FfmpegCodec, FfmpegFeatureSet, FfmpegMediaType, FfmpegMuxer,
        FfmpegOutputTarget, ffmpeg_capabilities,
    },
    logging,
    media_info::{
        AudioStream as EngineAudioStream, MediaInfo, MediaProbe as EngineMediaProbe, ProbeFormat,
        SilenceDetection, VideoStream as EngineVideoStream, detect_audio_silence, print_media_info,
        probe_media, probe_media_info,
    },
};

pub fn available_font_families() -> Vec<String> {
    compositor::text::available_font_families()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipResult {
    Played,
    Skipped,
    LiveEnded,
    Fallback { reason: String },
    Stopped,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LogoFade {
    pub fade_in: bool,
    pub fade_out: bool,
}

pub struct Playout {
    config: OutputConfig,
    output: Output,
    timeline: Timeline,
    fallback_duration: f64,
    playback_control: PlaybackControl,
}

#[derive(Debug, Clone, Default)]
pub struct PlaybackControl {
    skip_current: Arc<AtomicBool>,
}

impl PlaybackControl {
    pub fn skip_current(&self) {
        self.skip_current.store(true, Ordering::SeqCst);
    }

    pub(crate) fn take_skip_current(&self) -> bool {
        self.skip_current.swap(false, Ordering::SeqCst)
    }
}

#[derive(Clone, Copy)]
struct PlayOptions<'a> {
    seek_seconds: Option<f64>,
    duration_seconds: Option<f64>,
    subtitles_media_path: Option<&'a str>,
    logo_fade: LogoFade,
}

#[cfg(feature = "tokio")]
pub struct AsyncPlayout {
    commands: mpsc::Sender<AsyncCommand>,
    completion: WorkerCompletion,
    playback_control: PlaybackControl,
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

    pub async fn open_stream(
        output_url: impl Into<String>,
        config: OutputConfig,
        fallback_duration: f64,
    ) -> Result<Self> {
        let output_url = output_url.into();
        Self::open_with(move || Playout::open_stream(&output_url, config, fallback_duration)).await
    }

    pub async fn open_hls(
        playlist: impl Into<String>,
        config: OutputConfig,
        fallback_duration: f64,
        hls_variants: Vec<HlsVariant>,
        hls_subtitle: Option<HlsSubtitle>,
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
                hls_subtitle,
                hls_segment_seconds,
                hls_list_size,
            )
        })
        .await
    }

    #[cfg(feature = "desktop")]
    pub async fn open_desktop(config: OutputConfig, fallback_duration: f64) -> Result<Self> {
        let (commands, command_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = oneshot::channel();
        let (done_tx, done_rx) = oneshot::channel();
        let playback_control = PlaybackControl::default();
        let worker_playback_control = playback_control.clone();

        output::sdl_thread::spawn(move || {
            match Playout::open_desktop(config, fallback_duration) {
                Ok(mut playout) => {
                    playout.playback_control = worker_playback_control;
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
            playback_control,
        })
    }

    async fn open_with<F>(open: F) -> Result<Self>
    where
        F: FnOnce() -> Result<Playout> + Send + 'static,
    {
        let (commands, command_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = oneshot::channel();
        let playback_control = PlaybackControl::default();
        let worker_playback_control = playback_control.clone();

        let worker = thread::spawn(move || match open() {
            Ok(mut playout) => {
                playout.playback_control = worker_playback_control;
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
            playback_control,
        })
    }

    pub fn playback_control(&self) -> PlaybackControl {
        self.playback_control.clone()
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
                subtitles_media_path: None,
                logo_fade: LogoFade::default(),
                playout_rate: 1.0,
                response,
            })
            .map_err(|_| anyhow!("playout worker stopped"))?;

        result.await.context("playout worker stopped during play")?
    }

    pub async fn play_with_timing_and_logo_fade(
        &self,
        path: impl Into<String>,
        seek_seconds: Option<f64>,
        duration_seconds: Option<f64>,
        subtitles_media_path: Option<String>,
        logo_fade: LogoFade,
    ) -> Result<ClipResult> {
        self.play_with_timing_logo_fade_and_rate(
            path,
            seek_seconds,
            duration_seconds,
            subtitles_media_path,
            logo_fade,
            1.0,
        )
        .await
    }

    pub async fn play_with_timing_logo_fade_and_rate(
        &self,
        path: impl Into<String>,
        seek_seconds: Option<f64>,
        duration_seconds: Option<f64>,
        subtitles_media_path: Option<String>,
        logo_fade: LogoFade,
        playout_rate: f64,
    ) -> Result<ClipResult> {
        let path = path.into();
        let (response, result) = oneshot::channel();
        self.commands
            .send(AsyncCommand::Play {
                path,
                seek_seconds,
                duration_seconds,
                subtitles_media_path,
                logo_fade,
                playout_rate,
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
                config: Box::new(config),
                response,
            })
            .map_err(|_| anyhow!("playout worker stopped"))?;

        result
            .await
            .context("playout worker stopped while starting RTMP live")?
    }

    pub async fn finish(self) -> Result<()> {
        self.playback_control.skip_current();
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
        subtitles_media_path: Option<String>,
        logo_fade: LogoFade,
        playout_rate: f64,
        response: oneshot::Sender<Result<ClipResult>>,
    },
    StartRtmpLive {
        url: String,
        config: Box<OutputConfig>,
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
                subtitles_media_path,
                logo_fade,
                playout_rate,
                response,
            } => {
                let result = playout.play_timed_with_live(
                    &path,
                    seek_seconds,
                    duration_seconds,
                    subtitles_media_path.as_deref(),
                    logo_fade,
                    playout_rate,
                    &mut live,
                );
                let stopped = matches!(result, Ok(ClipResult::Stopped));
                let _ = response.send(result);
                if stopped {
                    break;
                }
            }
            AsyncCommand::StartRtmpLive {
                url,
                config,
                response,
            } => {
                live = Some(spawn_rtmp_listener(url, *config));
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
        init_ffmpeg(&config)?;
        let output = Output::open(output_url, &config)?;

        Ok(Self::with_output(config, output, fallback_duration))
    }

    pub fn open_stream(
        output_url: &str,
        config: OutputConfig,
        fallback_duration: f64,
    ) -> Result<Self> {
        Self::validate_fallback_duration(fallback_duration)?;
        init_ffmpeg(&config)?;
        let output = Output::open_stream(output_url, &config)?;

        Ok(Self::with_output(config, output, fallback_duration))
    }

    #[cfg(feature = "desktop")]
    pub fn open_desktop(config: OutputConfig, fallback_duration: f64) -> Result<Self> {
        Self::validate_fallback_duration(fallback_duration)?;
        init_ffmpeg(&config)?;
        let sdl = output::init_desktop_sdl()?;
        let config = output::desktop_config_for_primary_display(config, &sdl);
        let output = Output::open_desktop(&config, sdl)?;

        Ok(Self::with_output(config, output, fallback_duration))
    }

    pub fn open_hls(
        playlist: &str,
        config: OutputConfig,
        fallback_duration: f64,
        hls_variants: &[HlsVariant],
        hls_subtitle: Option<HlsSubtitle>,
        hls_segment_seconds: u32,
        hls_list_size: u32,
    ) -> Result<Self> {
        Self::validate_fallback_duration(fallback_duration)?;
        init_ffmpeg(&config)?;
        let output = Output::open_hls(
            playlist,
            &config,
            hls_variants,
            hls_subtitle,
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
            playback_control: PlaybackControl::default(),
        }
    }

    pub fn play(&mut self, path: &str) -> Result<ClipResult> {
        self.play_with_seek(path, None)
    }

    pub fn play_with_seek(&mut self, path: &str, seek_seconds: Option<f64>) -> Result<ClipResult> {
        self.play_timed_with_live(
            path,
            seek_seconds,
            None,
            Some(path),
            LogoFade::default(),
            1.0,
            &mut None,
        )
    }

    pub fn play_with_live(
        &mut self,
        path: &str,
        seek_seconds: Option<f64>,
        live: &mut Option<LiveReceiver>,
    ) -> Result<ClipResult> {
        self.play_timed_with_live(
            path,
            seek_seconds,
            None,
            Some(path),
            LogoFade::default(),
            1.0,
            live,
        )
    }

    pub fn play_with_timing_and_logo_fade(
        &mut self,
        path: &str,
        seek_seconds: Option<f64>,
        duration_seconds: Option<f64>,
        logo_fade: LogoFade,
    ) -> Result<ClipResult> {
        self.play_timed_with_live(
            path,
            seek_seconds,
            duration_seconds,
            Some(path),
            logo_fade,
            1.0,
            &mut None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn play_timed_with_live(
        &mut self,
        path: &str,
        seek_seconds: Option<f64>,
        duration_seconds: Option<f64>,
        subtitles_media_path: Option<&str>,
        logo_fade: LogoFade,
        playout_rate: f64,
        live: &mut Option<LiveReceiver>,
    ) -> Result<ClipResult> {
        let subtitles_media_path = subtitles_media_path.map(str::to_string);
        self.output.set_playout_rate(playout_rate);

        #[cfg(feature = "desktop")]
        if self.output.is_desktop() {
            let config = self.config.clone();
            let fallback_duration = self.fallback_duration;
            let playback_control = self.playback_control.clone();
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
                        &playback_control,
                        PlayOptions {
                            seek_seconds,
                            duration_seconds,
                            subtitles_media_path: subtitles_media_path.as_deref(),
                            logo_fade,
                        },
                    )
                } else {
                    play_to_output(
                        &path,
                        &config,
                        &mut timeline,
                        output,
                        fallback_duration,
                        &playback_control,
                        PlayOptions {
                            seek_seconds,
                            duration_seconds,
                            subtitles_media_path: subtitles_media_path.as_deref(),
                            logo_fade,
                        },
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
                &self.playback_control,
                PlayOptions {
                    seek_seconds,
                    duration_seconds,
                    subtitles_media_path: subtitles_media_path.as_deref(),
                    logo_fade,
                },
            )
        } else {
            play_to_output(
                path,
                &self.config,
                &mut self.timeline,
                &mut self.output,
                self.fallback_duration,
                &self.playback_control,
                PlayOptions {
                    seek_seconds,
                    duration_seconds,
                    subtitles_media_path: subtitles_media_path.as_deref(),
                    logo_fade,
                },
            )
        }
    }

    pub fn finish(self) -> Result<()> {
        self.output.finish()
    }
}

fn init_ffmpeg(config: &OutputConfig) -> Result<()> {
    compositor::text::init();
    ffmpeg_next::init().context("failed to initialize FFmpeg")?;
    logging::init(
        config.ffmpeg_log_level,
        config.ingest_log_level,
        &config.ffmpeg_ignore_lines,
        config.channel_id,
    );
    Ok(())
}

fn play_to_output<O: FrameOutput>(
    path: &str,
    config: &OutputConfig,
    timeline: &mut Timeline,
    output: &mut O,
    fallback_duration: f64,
    playback_control: &PlaybackControl,
    options: PlayOptions<'_>,
) -> Result<ClipResult> {
    match play_clip(
        path,
        config,
        timeline,
        output,
        options.seek_seconds,
        options.duration_seconds,
        options.subtitles_media_path,
        options.logo_fade,
        playback_control,
    ) {
        Ok(()) => Ok(ClipResult::Played),
        Err(error) if error.downcast_ref::<PlaybackSkipped>().is_some() => Ok(ClipResult::Skipped),
        Err(error) if error.downcast_ref::<LiveEnded>().is_some() => Ok(ClipResult::LiveEnded),
        Err(error) if error.downcast_ref::<PlaybackStopped>().is_some() => Ok(ClipResult::Stopped),
        Err(error) => {
            let reason = format!("{error:#}");
            let duration = options
                .duration_seconds
                .filter(|duration| duration.is_finite() && *duration > 0.0)
                .unwrap_or(fallback_duration);
            write_fallback(path, config, timeline, output, duration, playback_control)
                .with_context(|| format!("failed to generate fallback for {path}"))?;
            timeline.finish_logo_fade(options.logo_fade);
            Ok(ClipResult::Fallback { reason })
        }
    }
}
