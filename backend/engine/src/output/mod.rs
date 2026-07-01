use std::{error::Error, fmt};

#[cfg(feature = "desktop")]
mod desktop;
mod encoded;
mod hls;
#[cfg(feature = "desktop")]
pub(crate) mod sdl_thread;
mod vtt;

pub use hls::resolved_variant_playlist_path;

use anyhow::Result;
#[cfg(feature = "desktop")]
use anyhow::anyhow;
#[cfg(feature = "desktop")]
use desktop::{DesktopFrameSender, DesktopOutput};
use encoded::{EncodedFormat, EncodedOutput};
use ffmpeg_next::frame;

use crate::utils::config::{HlsVariant, OutputConfig};

#[derive(Debug)]
pub(crate) struct PlaybackStopped;

impl fmt::Display for PlaybackStopped {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("desktop playback stopped by user")
    }
}

impl Error for PlaybackStopped {}

pub(crate) trait FrameOutput {
    fn audio_frame_size(&self) -> usize;
    fn encode_video(&mut self, frame: &frame::Video) -> Result<()>;
    fn encode_audio(&mut self, frame: &frame::Audio) -> Result<()>;
    fn set_video_end(&mut self, _video_end_pts: Option<i64>) -> Result<()> {
        Ok(())
    }
    fn video_finished(&mut self) -> Result<()> {
        Ok(())
    }
    fn write_vtt_subtitles(
        &mut self,
        _media_path: &str,
        _output_start_ms: i64,
        _source_start_ms: i64,
    ) -> Result<()> {
        Ok(())
    }
}

pub(crate) struct Output {
    kind: OutputKind,
}

enum OutputKind {
    Encoded(EncodedOutput),
    #[cfg(feature = "desktop")]
    Desktop(DesktopOutput),
}

impl Output {
    pub(crate) fn open(path: &str, cfg: &OutputConfig) -> Result<Self> {
        Ok(Self {
            kind: OutputKind::Encoded(EncodedOutput::open(path, cfg, EncodedFormat::Auto)?),
        })
    }

    pub(crate) fn open_null(cfg: &OutputConfig) -> Result<Self> {
        Ok(Self {
            kind: OutputKind::Encoded(EncodedOutput::open("-", cfg, EncodedFormat::Null)?),
        })
    }

    pub(crate) fn open_hls(
        path: &str,
        cfg: &OutputConfig,
        hls_variants: &[HlsVariant],
        hls_vtt_subtitles: bool,
        hls_segment_seconds: u32,
        hls_list_size: u32,
    ) -> Result<Self> {
        Ok(Self {
            kind: OutputKind::Encoded(EncodedOutput::open(
                path,
                cfg,
                EncodedFormat::Hls {
                    variants: hls_variants.to_vec(),
                    vtt_subtitles: hls_vtt_subtitles,
                    segment_seconds: hls_segment_seconds,
                    list_size: hls_list_size,
                },
            )?),
        })
    }

    #[cfg(feature = "desktop")]
    pub(crate) fn open_desktop(cfg: &OutputConfig) -> Result<Self> {
        Ok(Self {
            kind: OutputKind::Desktop(DesktopOutput::open(cfg)?),
        })
    }

    pub(crate) fn audio_frame_size(&self) -> usize {
        match &self.kind {
            OutputKind::Encoded(output) => output.audio_frame_size(),
            #[cfg(feature = "desktop")]
            OutputKind::Desktop(output) => output.audio_frame_size(),
        }
    }

    pub(crate) fn encode_video(&mut self, frame: &frame::Video) -> Result<()> {
        match &mut self.kind {
            OutputKind::Encoded(output) => output.encode_video(frame),
            #[cfg(feature = "desktop")]
            OutputKind::Desktop(output) => output.encode_video(frame),
        }
    }

    pub(crate) fn encode_audio(&mut self, frame: &frame::Audio) -> Result<()> {
        match &mut self.kind {
            OutputKind::Encoded(output) => output.encode_audio(frame),
            #[cfg(feature = "desktop")]
            OutputKind::Desktop(output) => output.encode_audio(frame),
        }
    }

    pub(crate) fn finish(self) -> Result<()> {
        match self.kind {
            OutputKind::Encoded(output) => output.finish(),
            #[cfg(feature = "desktop")]
            OutputKind::Desktop(output) => output.finish(),
        }
    }

    #[cfg(feature = "desktop")]
    pub(crate) fn is_desktop(&self) -> bool {
        matches!(self.kind, OutputKind::Desktop(_))
    }

    #[cfg(feature = "desktop")]
    pub(crate) fn run_desktop<T, F>(&mut self, operation: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut DesktopFrameSender) -> T + Send + 'static,
    {
        match &mut self.kind {
            OutputKind::Desktop(output) => output.run_operation(operation),
            OutputKind::Encoded(_) => Err(anyhow!("output is not in desktop mode")),
        }
    }
}

impl FrameOutput for Output {
    fn audio_frame_size(&self) -> usize {
        Self::audio_frame_size(self)
    }

    fn encode_video(&mut self, frame: &frame::Video) -> Result<()> {
        Self::encode_video(self, frame)
    }

    fn encode_audio(&mut self, frame: &frame::Audio) -> Result<()> {
        Self::encode_audio(self, frame)
    }

    fn write_vtt_subtitles(
        &mut self,
        media_path: &str,
        output_start_ms: i64,
        source_start_ms: i64,
    ) -> Result<()> {
        match &mut self.kind {
            OutputKind::Encoded(output) => {
                output.write_vtt_subtitles(media_path, output_start_ms, source_start_ms)
            }
            #[cfg(feature = "desktop")]
            OutputKind::Desktop(_) => Ok(()),
        }
    }
}
