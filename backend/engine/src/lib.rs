use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[cfg(feature = "tokio")]
use std::{
    sync::mpsc,
    thread::{self, JoinHandle},
};

use anyhow::{Context, Result, anyhow};

#[cfg(feature = "tokio")]
use tokio::sync::oneshot;

mod analysis;
mod audio_mixer;
mod benchmark;
mod compositor;
mod input;
mod output;
mod playback_control;
mod playout;
mod utils;

pub use analysis::audio_level::{AudioFrameCallback, AudioLevel, AudioLevelCallback};
pub use analysis::loudness::{LoudnessMeterControl, LoudnessMetrics};
pub use audio_mixer::{
    AudioEffectsControl, LiveLoudnessConfig, LiveLoudnessControl, LiveLoudnessMeasurement,
    LiveLoudnessMetrics, LiveLoudnessProcessor,
};
use input::live::{LiveEnded, LiveOverrideOutput};
pub use input::live::{LiveReceiver, spawn_rtmp_listener};
#[cfg(all(feature = "desktop-base", feature = "tokio"))]
pub use output::desktop::thread::run_on_main_thread as run_desktop_on_main_thread;
pub use output::resolved_variant_playlist_path;
use output::{FrameOutput, Output, PlaybackStopped};
pub use playback_control::{LiveSession, NavigationBlocked, PlaybackControl, PlaylistNavigation};
use playout::{PlaybackRestart, PlaybackSkipped, Timeline, write_fallback};
pub use utils::{
    clock,
    config::{
        AudioOptions, DesktopControlCallback, DesktopControlCommand, HlsSubtitle, HlsVariant,
        LogLevel, LogoConfig, OutputConfig, OutputSize, RecordingConfig, RecordingEncodeConfig,
        RgbaColor, StreamType, TextBackgroundConfig, TextConfig, TextOverlayState, TextPosition,
        TextScroll, TextWeight, VideoOptionChoice, VideoOptionKind, VideoOptionSpec,
        VideoOptionVisibility, VideoOptions, audio_codec_uses_bitrate, validate_audio_options,
        validate_output_protocol_options, validate_video_options, video_codec_uses_bitrate,
        video_option_defaults, video_option_specs,
    },
    ffmpeg_capabilities::{
        FfmpegCapabilities, FfmpegCodec, FfmpegFeatureSet, FfmpegMediaType, FfmpegMuxer,
        FfmpegOutputTarget, ffmpeg_capabilities, validate_muxer_options,
    },
    helper::is_live_input,
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

/// Sets the report interval for the optional `processing-bench` feature.
/// Has no effect when that feature is disabled.
pub fn set_processing_bench_interval(interval: Duration) {
    benchmark::set_report_interval(interval);
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

#[derive(Clone)]
pub struct HlsHealth {
    last_muxed_at: Arc<std::sync::Mutex<Instant>>,
}

impl HlsHealth {
    pub(crate) fn new() -> Self {
        Self {
            last_muxed_at: Arc::new(std::sync::Mutex::new(Instant::now())),
        }
    }

    pub fn last_muxed_age(&self) -> Duration {
        self.last_muxed_at
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .elapsed()
    }

    pub(crate) fn mark_muxed(&self) {
        *self
            .last_muxed_at
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Instant::now();
    }
}

#[derive(Clone, Copy)]
struct PlayOptions<'a> {
    seek_seconds: Option<f64>,
    duration_seconds: Option<f64>,
    external_audio_path: Option<&'a str>,
    subtitles_media_path: Option<&'a str>,
    logo_fade: LogoFade,
}

#[cfg(feature = "tokio")]
pub struct AsyncPlayout {
    commands: mpsc::Sender<AsyncCommand>,
    completion: Option<WorkerCompletion>,
    playback_control: PlaybackControl,
    hls_health: Option<HlsHealth>,
}

/// How to wait for the playout worker to finish.
#[cfg(feature = "tokio")]
enum WorkerCompletion {
    Thread(JoinHandle<()>),
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
        let hls_health = HlsHealth::new();
        let worker_health = hls_health.clone();
        let mut playout = Self::open_with(move || {
            Playout::open_hls_with_health(
                &playlist,
                config,
                fallback_duration,
                &hls_variants,
                hls_subtitle,
                hls_segment_seconds,
                hls_list_size,
                worker_health,
            )
        })
        .await?;
        playout.hls_health = Some(hls_health);
        Ok(playout)
    }

    #[cfg(feature = "desktop-base")]
    pub async fn open_desktop(config: OutputConfig, fallback_duration: f64) -> Result<Self> {
        Self::open_with(move || Playout::open_desktop(config, fallback_duration)).await
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

        // Own the cancellation guard before the await: opening can itself be cancelled.
        let playout = Self {
            commands,
            completion: Some(WorkerCompletion::Thread(worker)),
            playback_control,
            hls_health: None,
        };
        ready_rx
            .await
            .context("playout worker stopped during open")??;
        Ok(playout)
    }

    pub fn playback_control(&self) -> PlaybackControl {
        self.playback_control.clone()
    }

    pub fn hls_health(&self) -> Option<HlsHealth> {
        self.hls_health.clone()
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
                external_audio_path: None,
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
        self.play_with_timing_logo_fade_rate_and_audio(
            path,
            seek_seconds,
            duration_seconds,
            None,
            subtitles_media_path,
            logo_fade,
            playout_rate,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn play_with_timing_logo_fade_rate_and_audio(
        &self,
        path: impl Into<String>,
        seek_seconds: Option<f64>,
        duration_seconds: Option<f64>,
        external_audio_path: Option<String>,
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
                external_audio_path,
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

    pub async fn finish(mut self) -> Result<()> {
        self.playback_control.request_shutdown();
        let (response, result) = oneshot::channel();
        self.commands
            .send(AsyncCommand::Finish { response })
            .map_err(|_| anyhow!("playout worker stopped"))?;

        let finish_result = result
            .await
            .context("playout worker stopped during finish")?;

        if let Some(WorkerCompletion::Thread(worker)) = self.completion.take()
            && worker.join().is_err()
            && finish_result.is_ok()
        {
            return Err(anyhow!("playout worker panicked during finish"));
        }

        finish_result
    }
}

#[cfg(feature = "tokio")]
impl Drop for AsyncPlayout {
    fn drop(&mut self) {
        if self.completion.is_some() {
            self.playback_control.request_shutdown();
            let (response, _) = oneshot::channel();
            let _ = self.commands.send(AsyncCommand::Finish { response });
        }
    }
}

#[cfg(feature = "tokio")]
enum AsyncCommand {
    Play {
        path: String,
        seek_seconds: Option<f64>,
        duration_seconds: Option<f64>,
        external_audio_path: Option<String>,
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
    let mut finish_response = None;

    while let Ok(command) = commands.recv() {
        if playout.playback_control.is_shutdown() && !matches!(command, AsyncCommand::Finish { .. })
        {
            continue;
        }
        match command {
            AsyncCommand::Play {
                path,
                seek_seconds,
                duration_seconds,
                external_audio_path,
                subtitles_media_path,
                logo_fade,
                playout_rate,
                response,
            } => {
                let result = playout.play_timed_with_live(
                    &path,
                    seek_seconds,
                    duration_seconds,
                    external_audio_path.as_deref(),
                    subtitles_media_path.as_deref(),
                    logo_fade,
                    playout_rate,
                    &mut live,
                );
                let _ = response.send(result);
                // A closed desktop window reports `Stopped`, but the owner
                // still sends `Finish` immediately afterwards. Keep this
                // worker alive so that command can explicitly release the
                // window and its WGPU resources before process shutdown.
            }
            AsyncCommand::StartRtmpLive {
                url,
                config,
                response,
            } => {
                let receiver = spawn_rtmp_listener(url, *config);
                receiver.set_benchmark(benchmark::current());
                live = Some(receiver);
                let _ = response.send(Ok(()));
            }
            AsyncCommand::Finish { response } => {
                finish_response = Some(response);
                break;
            }
        }
    }
    // Also finalize after cancellation or channel disconnection, and abort the
    // listener before flushing output so it cannot retain more decoded frames.
    drop(live);
    let channel_id = playout.config.channel_id.unwrap_or_default();
    let result = playout.finish();
    if let Some(response) = finish_response {
        let _ = response.send(result);
    } else if let Err(error) = result {
        log::warn!(channel = channel_id; "failed to finalize abandoned playout: {error:#}");
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

    #[cfg(feature = "desktop-base")]
    pub fn open_desktop(config: OutputConfig, fallback_duration: f64) -> Result<Self> {
        Self::validate_fallback_duration(fallback_duration)?;
        init_ffmpeg(&config)?;
        let output = Output::open_desktop(&config)?;

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
        Self::open_hls_with_health(
            playlist,
            config,
            fallback_duration,
            hls_variants,
            hls_subtitle,
            hls_segment_seconds,
            hls_list_size,
            HlsHealth::new(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn open_hls_with_health(
        playlist: &str,
        config: OutputConfig,
        fallback_duration: f64,
        hls_variants: &[HlsVariant],
        hls_subtitle: Option<HlsSubtitle>,
        hls_segment_seconds: u32,
        hls_list_size: u32,
        hls_health: HlsHealth,
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
            hls_health,
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
        #[cfg(feature = "desktop-base")]
        if !output.is_desktop() {
            benchmark::start(config.channel_id);
        }
        #[cfg(not(feature = "desktop-base"))]
        benchmark::start(config.channel_id);

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
            None,
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
        external_audio_path: Option<&str>,
        subtitles_media_path: Option<&str>,
        logo_fade: LogoFade,
        playout_rate: f64,
        live: &mut Option<LiveReceiver>,
    ) -> Result<ClipResult> {
        let external_audio_path = external_audio_path.map(str::to_string);
        let subtitles_media_path = subtitles_media_path.map(str::to_string);
        self.output.set_playout_rate(playout_rate);

        #[cfg(feature = "desktop-base")]
        if self.output.is_desktop() {
            let config = self.config.clone();
            let fallback_duration = self.fallback_duration;
            let playback_control = self.playback_control.clone();
            let mut timeline = self.timeline;
            let path = path.to_string();
            let mut live_for_worker = live.take();
            let benchmark = benchmark::start(config.channel_id);
            if let Some(live) = live_for_worker.as_ref() {
                live.set_benchmark(Some(benchmark.clone()));
            }
            let operation = self.output.run_desktop(benchmark, move |output| {
                let result = if let Some(live) = live_for_worker.as_mut() {
                    let mut output = LiveOverrideOutput::new(output, live, &playback_control);
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
                            external_audio_path: external_audio_path.as_deref(),
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
                            external_audio_path: external_audio_path.as_deref(),
                            subtitles_media_path: subtitles_media_path.as_deref(),
                            logo_fade,
                        },
                    )
                };
                if matches!(&result, Ok(ClipResult::LiveEnded))
                    && let Some(live) = live_for_worker.as_ref()
                {
                    live.reanchor_timeline(&mut timeline);
                }
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
            let result = {
                let mut output =
                    LiveOverrideOutput::new(&mut self.output, live, &self.playback_control);
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
                        external_audio_path: external_audio_path.as_deref(),
                        subtitles_media_path: subtitles_media_path.as_deref(),
                        logo_fade,
                    },
                )
            };
            if matches!(&result, Ok(ClipResult::LiveEnded)) {
                live.reanchor_timeline(&mut self.timeline);
            }
            result
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
                    external_audio_path: external_audio_path.as_deref(),
                    subtitles_media_path: subtitles_media_path.as_deref(),
                    logo_fade,
                },
            )
        }
    }

    pub fn finish(self) -> Result<()> {
        let result = self.output.finish();
        benchmark::finish();
        result
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
    match playout::play_clip_with_external_audio(
        path,
        options.external_audio_path,
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
        Err(error) if error.downcast_ref::<PlaybackRestart>().is_some() => Err(error),
        Err(error) if error.downcast_ref::<PlaybackSkipped>().is_some() => Ok(ClipResult::Skipped),
        Err(error) if error.downcast_ref::<LiveEnded>().is_some() => Ok(ClipResult::LiveEnded),
        Err(error) if error.downcast_ref::<PlaybackStopped>().is_some() => Ok(ClipResult::Stopped),
        Err(error) => {
            let reason = format!("{error:#}");
            let duration = options
                .duration_seconds
                .filter(|duration| duration.is_finite() && *duration > 0.0)
                .unwrap_or(fallback_duration);
            match write_fallback(path, config, timeline, output, duration, playback_control) {
                Ok(()) => {
                    timeline.finish_logo_fade(options.logo_fade);
                    Ok(ClipResult::Fallback { reason })
                }
                Err(error) => classify_fallback_error(error, path),
            }
        }
    }
}

fn classify_fallback_error(error: anyhow::Error, path: &str) -> Result<ClipResult> {
    if error.downcast_ref::<PlaybackRestart>().is_some() {
        Err(error)
    } else if error.downcast_ref::<PlaybackSkipped>().is_some() {
        Ok(ClipResult::Skipped)
    } else if error.downcast_ref::<LiveEnded>().is_some() {
        Ok(ClipResult::LiveEnded)
    } else if error.downcast_ref::<PlaybackStopped>().is_some() {
        Ok(ClipResult::Stopped)
    } else {
        Err(error.context(format!("failed to generate fallback for {path}")))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ClipResult, PlaybackControl, PlaybackRestart, PlaybackSkipped, classify_fallback_error,
    };

    #[test]
    fn restart_during_fallback_remains_a_playout_restart() {
        let result =
            classify_fallback_error(anyhow::Error::new(PlaybackRestart), "unavailable.mp4");

        assert!(result.unwrap_err().is::<PlaybackRestart>());
    }

    #[test]
    fn playlist_reset_during_fallback_reinitializes_playout() {
        let result =
            classify_fallback_error(anyhow::Error::new(PlaybackSkipped), "unavailable.mp4");

        assert_eq!(result.unwrap(), ClipResult::Skipped);
    }

    #[test]
    fn restart_request_is_independent_from_skip_request() {
        let control = PlaybackControl::default();
        control.restart_playout();

        assert!(control.take_restart());
        assert!(!control.take_skip_current());
        assert!(!control.take_restart());
    }

    #[cfg(feature = "tokio")]
    #[test]
    fn dropping_async_playout_discards_queued_work_and_finishes_worker() {
        use super::*;
        let directory =
            std::env::temp_dir().join(format!("ffplayout-drop-worker-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("output.mkv");
        let playback_control = PlaybackControl::default();
        let worker_control = playback_control.clone();
        let (commands, command_rx) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let (done, finished) = mpsc::channel();
        let worker = thread::spawn(move || {
            // Wait until the caller has dropped its owner with queued work.
            gate.recv().unwrap();
            let mut playout = Playout::open(
                path.to_str().unwrap(),
                OutputConfig::new(32, 32, 25, 48_000),
                1.0,
            )
            .unwrap();
            playout.playback_control = worker_control;
            run_async_playout_worker(playout, command_rx);
            done.send(()).unwrap();
        });
        let owner = AsyncPlayout {
            commands,
            completion: Some(WorkerCompletion::Thread(worker)),
            playback_control: playback_control.clone(),
            hls_health: None,
        };
        let mut responses = Vec::new();
        for _ in 0..2 {
            let (response, result) = oneshot::channel();
            owner
                .commands
                .send(AsyncCommand::Play {
                    path: String::new(),
                    seek_seconds: None,
                    duration_seconds: Some(3600.0),
                    external_audio_path: None,
                    subtitles_media_path: None,
                    logo_fade: LogoFade::default(),
                    playout_rate: 1.0,
                    response,
                })
                .unwrap();
            responses.push(result);
        }
        drop(owner);
        assert!(playback_control.is_shutdown());
        release.send(()).unwrap();
        finished.recv_timeout(Duration::from_secs(5)).unwrap();
        for mut response in responses {
            assert!(matches!(
                response.try_recv(),
                Err(oneshot::error::TryRecvError::Closed)
            ));
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}
