use std::{error::Error, fmt, sync::Arc};

use anyhow::Result;
#[cfg(feature = "desktop-base")]
use anyhow::anyhow;
use ffmpeg_next::frame;

mod composition;
mod delivery;
#[cfg(feature = "desktop-base")]
pub(crate) mod desktop;
mod diagnostics;
mod encoded;
mod encoding;
mod hls;
mod mux;
pub(crate) mod parallel;
mod recording;
mod shared;
mod vtt;

#[cfg(feature = "desktop-base")]
use crate::benchmark::BenchHandle;
use crate::{
    HlsHealth,
    clock::SharedPlayoutClock,
    compositor::logo::{LogoOverlay, blend_logo},
    input::diagnostics::LiveInputDiagnostics,
    utils::config::{HlsSubtitle, HlsVariant, OutputConfig},
};

#[cfg(feature = "desktop-base")]
use desktop::{DesktopFrameSender, DesktopOutput};
use encoded::{EncodedFormat, EncodedOutput};
pub use hls::resolved_variant_playlist_path;

#[derive(Debug)]
pub(crate) struct PlaybackStopped;

impl fmt::Display for PlaybackStopped {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("desktop playback stopped by user")
    }
}

impl Error for PlaybackStopped {}

pub(crate) trait FrameOutput {
    fn set_frame_timeline(&mut self, _timeline: &crate::playout::Timeline) {}

    fn playout_clock(&self) -> Option<SharedPlayoutClock> {
        None
    }

    fn input_diagnostics_handle(&self) -> Option<std::sync::Arc<LiveInputDiagnostics>> {
        None
    }

    fn input_diagnostics(&self) -> Option<&LiveInputDiagnostics> {
        None
    }

    fn audio_frame_size(&self) -> usize;

    fn encode_video(&mut self, frame: &frame::Video) -> Result<()>;

    /// Realtime outputs can decline a full queue so live ingest can pad audio
    /// and observe cancellation before retrying the same video frame.
    fn try_encode_video(&mut self, frame: &frame::Video) -> Result<bool> {
        self.encode_video(frame)?;

        Ok(true)
    }

    fn encode_audio(&mut self, frame: &frame::Audio) -> Result<()>;

    /// Volume has already been applied; meters and encoding still run normally.
    fn encode_processed_audio(&mut self, frame: &frame::Audio) -> Result<()> {
        self.encode_audio(frame)
    }

    fn handles_loudness(&self) -> bool {
        false
    }

    fn encode_live_audio(&mut self, frame: &frame::Audio) -> Result<()> {
        self.encode_audio(frame)
    }

    fn encode_audio_with_gain_hold(&mut self, frame: &frame::Audio, _hold: bool) -> Result<()> {
        self.encode_audio(frame)
    }

    /// Discard output buffered past a manual clip skip and re-anchor at the
    /// supplied synchronized timeline position. Encoded outputs cannot
    /// retract muxed packets and therefore use the default padding path.
    fn reset_after_skip(&mut self, _video_pts: i64, _audio_pts: i64) -> Result<bool> {
        Ok(false)
    }

    fn apply_logo_overlay(
        &mut self,
        frame: &mut frame::Video,
        logo: &LogoOverlay,
        opacity_factor: f64,
    ) {
        blend_logo(frame, logo, opacity_factor);
    }

    /// Keep an existing logo reference when composition is deferred.
    fn apply_shared_logo_overlay(
        &mut self,
        frame: &mut frame::Video,
        logo: &Arc<LogoOverlay>,
        opacity_factor: f64,
    ) {
        self.apply_logo_overlay(frame, logo, opacity_factor);
    }

    fn clear_logo_overlay(&mut self) {}

    fn benchmarks_logo_overlay(&self) -> bool {
        true
    }

    fn set_video_end(&mut self, _video_end_pts: Option<i64>) -> Result<()> {
        Ok(())
    }

    /// Signals that all decoded video frames have been queued. Realtime
    /// outputs can start short clips without ending their timeline early.
    fn video_decoded(&mut self) -> Result<()> {
        Ok(())
    }

    fn video_finished(&mut self) -> Result<()> {
        Ok(())
    }

    /// Lets realtime outputs account for silence that already elapsed while
    /// decode was blocked behind video backpressure.
    fn pad_audio(&mut self, _samples: i64) -> Result<bool> {
        Ok(false)
    }

    fn write_vtt_subtitles(
        &mut self,
        _media_path: &str,
        _output_start_ms: i64,
        _source_start_ms: i64,
    ) -> Result<()> {
        Ok(())
    }

    /// Discard subtitles queued for the previous clip. Encoded outputs keep
    /// cues pending until file playback reaches their timestamp.
    fn clear_vtt_subtitles(&mut self) -> Result<()> {
        Ok(())
    }

    /// Release subtitle cues whose output timestamp has been reached by an
    /// actually emitted file video frame.
    fn advance_vtt_subtitles(&mut self, _output_position_ms: i64) -> Result<()> {
        Ok(())
    }
}

pub(crate) struct Output {
    deferred_logo: Option<(Arc<LogoOverlay>, f64)>,
    kind: OutputKind,
}

enum OutputKind {
    Encoded(Box<EncodedOutput>),
    Parallel(Box<parallel::ParallelOutput>),
    #[cfg(feature = "desktop-base")]
    Desktop(Box<DesktopOutput>),
}

impl Output {
    pub(crate) fn open_parallel(cfg: &OutputConfig) -> Result<Self> {
        Ok(Self {
            deferred_logo: None,
            kind: OutputKind::Parallel(Box::new(parallel::ParallelOutput::new(cfg)?)),
        })
    }

    pub(crate) fn open(path: &str, cfg: &OutputConfig) -> Result<Self> {
        Ok(Self {
            deferred_logo: None,
            kind: OutputKind::Encoded(Box::new(
                EncodedOutput::open(path, cfg, EncodedFormat::Auto)?.threaded()?,
            )),
        })
    }

    pub(crate) fn open_stream(path: &str, cfg: &OutputConfig) -> Result<Self> {
        Ok(Self {
            deferred_logo: None,
            kind: OutputKind::Encoded(Box::new(
                EncodedOutput::open(
                    path,
                    cfg,
                    EncodedFormat::Stream {
                        muxer: cfg.stream_type.muxer(&cfg.stream_format).to_string(),
                    },
                )?
                .threaded()?,
            )),
        })
    }

    pub(crate) fn open_hls(
        path: &str,
        cfg: &OutputConfig,
        hls_variants: &[HlsVariant],
        hls_subtitle: Option<HlsSubtitle>,
        hls_segment_seconds: u32,
        hls_list_size: u32,
        hls_health: HlsHealth,
    ) -> Result<Self> {
        Ok(Self {
            deferred_logo: None,
            kind: OutputKind::Encoded(Box::new(
                EncodedOutput::open_with_hls_health(
                    path,
                    cfg,
                    EncodedFormat::Hls {
                        variants: hls_variants.to_vec(),
                        subtitle: hls_subtitle,
                        segment_seconds: hls_segment_seconds,
                        list_size: hls_list_size,
                    },
                    Some(hls_health),
                )?
                .threaded()?,
            )),
        })
    }

    #[cfg(feature = "desktop-base")]
    pub(crate) fn open_desktop(cfg: &OutputConfig) -> Result<Self> {
        Ok(Self {
            deferred_logo: None,
            kind: OutputKind::Desktop(Box::new(DesktopOutput::open(cfg)?)),
        })
    }

    pub(crate) fn audio_frame_size(&self) -> usize {
        match &self.kind {
            OutputKind::Encoded(output) => output.audio_frame_size(),
            OutputKind::Parallel(_) => 1024,
            #[cfg(feature = "desktop-base")]
            OutputKind::Desktop(output) => output.audio_frame_size(),
        }
    }

    pub(crate) fn encode_video(&mut self, frame: &frame::Video) -> Result<()> {
        match &mut self.kind {
            OutputKind::Encoded(output) => output.encode_video(frame),
            OutputKind::Parallel(output) => match &self.deferred_logo {
                Some((logo, opacity)) => {
                    output.video_with_logo(frame, Some(logo.clone()), *opacity)
                }
                None => output.encode_video(frame),
            },
            #[cfg(feature = "desktop-base")]
            OutputKind::Desktop(output) => output.encode_video(frame),
        }
    }

    pub(crate) fn encode_audio(&mut self, frame: &frame::Audio) -> Result<()> {
        match &mut self.kind {
            OutputKind::Encoded(output) => output.encode_audio(frame),
            OutputKind::Parallel(output) => output.encode_audio(frame),
            #[cfg(feature = "desktop-base")]
            OutputKind::Desktop(output) => output.encode_audio(frame),
        }
    }

    pub(crate) fn set_playout_rate(&mut self, rate: f64) {
        match &mut self.kind {
            OutputKind::Encoded(output) => output.set_playout_rate(rate),
            OutputKind::Parallel(output) => output.set_playout_rate(rate),
            #[cfg(feature = "desktop-base")]
            OutputKind::Desktop(_) => {}
        }
    }

    pub(crate) fn finish(self) -> Result<()> {
        match self.kind {
            OutputKind::Encoded(output) => output.finish(),
            OutputKind::Parallel(output) => output.finish(),
            #[cfg(feature = "desktop-base")]
            OutputKind::Desktop(output) => output.finish(),
        }
    }

    #[cfg(feature = "desktop-base")]
    pub(crate) fn is_desktop(&self) -> bool {
        matches!(self.kind, OutputKind::Desktop(_))
    }

    #[cfg(feature = "desktop-base")]
    pub(crate) fn run_desktop<T, F>(&mut self, benchmark: BenchHandle, operation: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut DesktopFrameSender) -> T + Send + 'static,
    {
        match &mut self.kind {
            OutputKind::Desktop(output) => output.run_operation(benchmark, operation),
            OutputKind::Encoded(_) | OutputKind::Parallel(_) => {
                Err(anyhow!("output is not in desktop mode"))
            }
        }
    }
}

impl FrameOutput for Output {
    fn playout_clock(&self) -> Option<SharedPlayoutClock> {
        match &self.kind {
            OutputKind::Encoded(output) => output.playout_clock(),
            OutputKind::Parallel(output) => Some(output.clock()),
            #[cfg(feature = "desktop-base")]
            OutputKind::Desktop(_) => None,
        }
    }

    fn audio_frame_size(&self) -> usize {
        Self::audio_frame_size(self)
    }

    fn encode_video(&mut self, frame: &frame::Video) -> Result<()> {
        Self::encode_video(self, frame)
    }

    fn encode_audio(&mut self, frame: &frame::Audio) -> Result<()> {
        Self::encode_audio(self, frame)
    }

    fn encode_processed_audio(&mut self, frame: &frame::Audio) -> Result<()> {
        match &mut self.kind {
            OutputKind::Encoded(output) => output.encode_processed_audio(frame),
            OutputKind::Parallel(output) => output.encode_processed_audio(frame),
            #[cfg(feature = "desktop-base")]
            OutputKind::Desktop(output) => output.encode_audio(frame),
        }
    }

    fn apply_logo_overlay(
        &mut self,
        frame: &mut frame::Video,
        logo: &LogoOverlay,
        opacity_factor: f64,
    ) {
        match &mut self.kind {
            OutputKind::Encoded(_) => blend_logo(frame, logo, opacity_factor),
            OutputKind::Parallel(_) => {
                self.deferred_logo = Some((Arc::new(logo.clone()), opacity_factor));
            }
            #[cfg(feature = "desktop-base")]
            OutputKind::Desktop(_) => {}
        }
    }

    fn apply_shared_logo_overlay(
        &mut self,
        frame: &mut frame::Video,
        logo: &Arc<LogoOverlay>,
        opacity_factor: f64,
    ) {
        if matches!(self.kind, OutputKind::Parallel(_)) {
            self.deferred_logo = Some((logo.clone(), opacity_factor));
        } else {
            self.apply_logo_overlay(frame, logo, opacity_factor);
        }
    }

    fn clear_logo_overlay(&mut self) {
        self.deferred_logo = None;
    }

    fn benchmarks_logo_overlay(&self) -> bool {
        match self.kind {
            OutputKind::Encoded(_) => true,
            OutputKind::Parallel(_) => false,
            #[cfg(feature = "desktop-base")]
            OutputKind::Desktop(_) => false,
        }
    }

    fn write_vtt_subtitles(
        &mut self,
        media_path: &str,
        output_start_ms: i64,
        source_start_ms: i64,
    ) -> Result<()> {
        match &mut self.kind {
            OutputKind::Parallel(output) => {
                output.write_vtt_subtitles(media_path, output_start_ms, source_start_ms)
            }
            OutputKind::Encoded(output) => {
                output.write_vtt_subtitles(media_path, output_start_ms, source_start_ms)
            }
            #[cfg(feature = "desktop-base")]
            OutputKind::Desktop(_) => Ok(()),
        }
    }

    fn clear_vtt_subtitles(&mut self) -> Result<()> {
        match &mut self.kind {
            OutputKind::Parallel(output) => output.clear_vtt_subtitles(),
            OutputKind::Encoded(output) => {
                output.clear_vtt_subtitles();

                Ok(())
            }
            #[cfg(feature = "desktop-base")]
            OutputKind::Desktop(_) => Ok(()),
        }
    }

    fn advance_vtt_subtitles(&mut self, output_position_ms: i64) -> Result<()> {
        match &mut self.kind {
            OutputKind::Encoded(output) => output.advance_vtt_subtitles(output_position_ms),
            OutputKind::Parallel(output) => output.advance_vtt_subtitles(output_position_ms),
            #[cfg(feature = "desktop-base")]
            OutputKind::Desktop(_) => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Playout, utils::config::LogoConfig};
    use ffmpeg_next::{ChannelLayout, codec, format::Pixel, media};

    #[test]
    fn server_pipeline_keeps_logo_pixels_at_each_output_resolution() {
        ffmpeg_next::init().unwrap();
        let directory = std::env::temp_dir().join(format!(
            "server-logo-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let mut config = OutputConfig::new(320, 180, 25, 48_000);
        config.stream_type = crate::StreamType::Custom;
        config.stream_format = "mpegts".into();
        for (key, value) in [
            ("preset", "ultrafast"),
            ("tune", "zerolatency"),
            ("quality", "0"),
        ] {
            config.video_options.insert(key.into(), value.into());
        }
        let source = directory.join("source.ts");
        let mut writer = Output::open_stream(source.to_str().unwrap(), &config).unwrap();
        for index in 0..5 {
            let mut video = frame::Video::new(Pixel::YUV420P, 320, 180);
            for plane in 0..video.planes() {
                video.data_mut(plane).fill(128);
            }
            video.set_pts(Some(index));
            writer.encode_video(&video).unwrap();
            let mut audio = frame::Audio::new(
                ffmpeg_next::format::Sample::F32(ffmpeg_next::format::sample::Type::Planar),
                1920,
                ChannelLayout::STEREO,
            );
            audio.set_rate(48_000);
            audio.set_pts(Some(index * 1920));
            for channel in 0..2 {
                audio.plane_mut::<f32>(channel).fill(0.0);
            }
            writer.encode_audio(&audio).unwrap();
        }
        writer.finish().unwrap();
        let logo_config = LogoConfig {
            path: std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests_assets/storage/logo.png")
                .to_string_lossy()
                .into_owned(),
            scale: Some("32:16".into()),
            position: "W-w-8:H-h-8".into(),
            opacity: 1.0,
        };
        config.logo = Some(logo_config.clone());
        let resolutions = [(320, 180), (160, 90)];
        for (index, (width, height)) in resolutions.into_iter().enumerate() {
            let mut settings = config.clone();
            settings.width = width;
            settings.height = height;
            config.destinations.push(parallel::OutputDestination {
                id: index as i32 + 1,
                config: settings,
                target: parallel::OutputTarget::Stream {
                    url: directory
                        .join(format!("output-{index}.ts"))
                        .to_string_lossy()
                        .into_owned(),
                },
            });
        }
        let mut playout = Playout::open_parallel(config, 1.0).unwrap();
        playout.play(source.to_str().unwrap()).unwrap();
        playout.finish().unwrap();
        let logo = LogoOverlay::load(&logo_config, 320, 180).unwrap();
        for (index, (width, height)) in resolutions.into_iter().enumerate() {
            let mut expected = frame::Video::new(Pixel::YUV420P, width, height);
            for plane in 0..expected.planes() {
                expected.data_mut(plane).fill(128);
            }
            let positioned = logo
                .positioned(Some(&logo_config.position), width, height)
                .unwrap();
            blend_logo(&mut expected, &positioned, 1.0);
            let mut input =
                ffmpeg_next::format::input(&directory.join(format!("output-{index}.ts"))).unwrap();
            let stream = input.streams().best(media::Type::Video).unwrap();
            let stream_index = stream.index();
            let mut decoder = codec::context::Context::from_parameters(stream.parameters())
                .unwrap()
                .decoder()
                .video()
                .unwrap();
            let mut checked = 0;
            let mut verify = |decoded: &frame::Video| {
                assert_eq!((decoded.width(), decoded.height()), (width, height));
                // MPEG-TS may prepend black frames for the initial stream timestamp.
                if decoded.data(0)[0] == 16 {
                    return;
                }

                for row in 0..height as usize {
                    assert_eq!(
                        &decoded.data(0)
                            [row * decoded.stride(0)..row * decoded.stride(0) + width as usize],
                        &expected.data(0)
                            [row * expected.stride(0)..row * expected.stride(0) + width as usize],
                        "logo pixels differ at resolution {width}x{height}, row {row}"
                    );
                }
                checked += 1;
            };
            for (stream, packet) in input.packets() {
                if stream.index() == stream_index {
                    decoder.send_packet(&packet).unwrap();
                    let mut decoded = frame::Video::empty();
                    while decoder.receive_frame(&mut decoded).is_ok() {
                        verify(&decoded);
                    }
                }
            }
            decoder.send_eof().unwrap();
            let mut decoded = frame::Video::empty();
            while decoder.receive_frame(&mut decoded).is_ok() {
                verify(&decoded);
            }
            assert!(checked >= 5, "missing output frames");
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}
