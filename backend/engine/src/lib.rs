use anyhow::{Context, Result, anyhow};

mod input;
mod output;
mod playout;
mod utils;

use input::live::LiveOverrideOutput;
pub use input::live::{LiveReceiver, spawn_rtmp_listener};
use output::{FrameOutput, Output, PlaybackStopped};
use playout::{Timeline, play_clip, write_fallback};
pub use utils::{
    clock,
    config::{HlsVariant, OutputConfig, OutputSize},
    logging,
    media_info::{MediaInfo, print_media_info, probe_media_info},
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

impl Playout {
    pub fn open(output_url: &str, config: OutputConfig, fallback_duration: f64) -> Result<Self> {
        Self::validate_fallback_duration(fallback_duration)?;
        init_ffmpeg()?;
        let output = Output::open(output_url, &config)?;

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
    ) -> Result<Self> {
        Self::validate_fallback_duration(fallback_duration)?;
        init_ffmpeg()?;
        let output = Output::open_hls(playlist, &config, hls_variants, hls_vtt_subtitles)?;

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
        self.play_with_live(path, seek_seconds, &mut None)
    }

    pub fn play_with_live(
        &mut self,
        path: &str,
        seek_seconds: Option<f64>,
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
                    )
                } else {
                    play_to_output(
                        &path,
                        &config,
                        &mut timeline,
                        output,
                        fallback_duration,
                        seek_seconds,
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
            )
        } else {
            play_to_output(
                path,
                &self.config,
                &mut self.timeline,
                &mut self.output,
                self.fallback_duration,
                seek_seconds,
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
) -> Result<ClipResult> {
    match play_clip(path, config, timeline, output, seek_seconds) {
        Ok(()) => Ok(ClipResult::Played),
        Err(error) if error.downcast_ref::<PlaybackStopped>().is_some() => Ok(ClipResult::Stopped),
        Err(error) => {
            let reason = format!("{error:#}");
            write_fallback(config, timeline, output, fallback_duration)
                .with_context(|| format!("failed to generate fallback for {path}"))?;
            Ok(ClipResult::Fallback { reason })
        }
    }
}
