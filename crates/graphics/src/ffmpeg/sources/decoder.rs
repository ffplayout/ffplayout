//! Incremental FFmpeg image/video decoding. Own this adapter on a preparation worker.
use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use ffmpeg_next::{Error, Packet, Rational, Rescale, codec, format, frame, media};

use crate::{
    layout::logo_dimensions,
    source::{FrameTiming, LoopDuration},
    style::LogoStyle,
    surface::Alpha,
};

use super::{
    auxiliary::AuxiliaryDecoder,
    avif,
    logo::LogoOverlay,
    preparation::{self, SourcePreparation},
};

pub struct SourceFrame {
    pub timing: FrameTiming,
    pub loop_end: LoopDuration,
    pub surface: Arc<LogoOverlay>,
}

struct DecodedFrame {
    video: frame::Video,
    alpha: Option<frame::Video>,
}

pub struct LogoDecoder {
    input: format::context::Input,
    decoder: codec::decoder::Video,
    preparation: SourcePreparation,
    high_quality: bool,
    source_alpha: Option<Alpha>,
    container_alpha: Option<Alpha>,
    alpha: Option<AuxiliaryDecoder>,
    stream: usize,
    time_base: Rational,
    default_duration_us: i64,
    style: LogoStyle,
    canvas: (u32, u32),
    dimensions: (u32, u32),
    origin: Option<i64>,
    origin_ticks: Option<i64>,
    next_us: i64,
    eof: bool,
    pending: Option<(DecodedFrame, i64, i64, LoopDuration)>,
}

impl LogoDecoder {
    pub fn new(
        input: format::context::Input,
        style: LogoStyle,
        width: u32,
        height: u32,
    ) -> Result<Self> {
        let association = avif::streams(&input)?;
        let stream = association
            .and_then(|association| input.stream(association.color))
            .or_else(|| input.streams().best(media::Type::Video))
            .context("graphics source has no video stream")?;
        let decoder = open_graphics_decoder(&stream)?;
        let alpha = association
            .map(|association| AuxiliaryDecoder::new(&input, association.alpha))
            .transpose()?;
        let source_alpha = preparation::stream_alpha(&stream);
        let container_alpha = association.map(|association| association.mode);
        let fps = stream.avg_frame_rate();
        let default_duration_us = if fps.numerator() > 0 && fps.denominator() > 0 {
            i64::from(fps.denominator()).saturating_mul(1_000_000) / i64::from(fps.numerator())
        } else {
            100_000
        }
        .max(1);
        let dimensions = logo_dimensions(
            style.scale.as_deref(),
            decoder.width(),
            decoder.height(),
            width,
            height,
        )?;
        // Bound decoder/scaler allocations before preparing any surfaces.
        ensure!(
            decoder.width() <= 8192 && decoder.height() <= 8192,
            "graphics input dimensions exceed 8192 pixels"
        );
        ensure!(
            dimensions.0 > 0 && dimensions.1 > 0 && dimensions.0 <= 8192 && dimensions.1 <= 8192,
            "graphics output dimensions are invalid or exceed 8192 pixels"
        );
        let stream_index = stream.index();
        let time_base = stream.time_base();
        ensure!(
            time_base.numerator() > 0 && time_base.denominator() > 0,
            "graphics source has invalid time base"
        );

        Ok(Self {
            input,
            decoder,
            preparation: SourcePreparation::default(),
            high_quality: true,
            source_alpha,
            container_alpha,
            alpha,
            stream: stream_index,
            time_base,
            default_duration_us,
            style,
            canvas: (width, height),
            dimensions,
            origin: None,
            origin_ticks: None,
            next_us: 0,
            eof: false,
            pending: None,
        })
    }

    pub fn set_high_quality(&mut self, high_quality: bool) {
        self.high_quality = high_quality;
    }

    fn decode(
        &mut self,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Option<(DecodedFrame, i64, i64, LoopDuration)>> {
        loop {
            ensure!(!cancelled(), "graphics decoding cancelled");
            let mut decoded = frame::Video::empty();
            let (received, alpha) = if let Some(alpha) = &mut self.alpha {
                match alpha.next_frame(
                    &mut self.input,
                    &mut self.decoder,
                    self.stream,
                    self.time_base,
                    cancelled,
                )? {
                    Some((video, alpha)) => {
                        decoded = video;
                        (Ok(()), Some(alpha))
                    }
                    None => return Ok(None),
                }
            } else {
                (self.decoder.receive_frame(&mut decoded), None)
            };

            match received {
                Ok(()) => {
                    let source_ticks = decoded.timestamp().or(decoded.pts());
                    let absolute = decoded
                        .timestamp()
                        .or(decoded.pts())
                        .map(|pts| pts.rescale(self.time_base, Rational(1, 1_000_000)));
                    let start = absolute
                        .map(|pts| pts.saturating_sub(*self.origin.get_or_insert(pts)))
                        .unwrap_or(self.next_us)
                        .max(0);
                    let duration = decoded
                        .packet()
                        .duration
                        .rescale(self.time_base, Rational(1, 1_000_000));
                    let duration = if duration > 0 {
                        duration
                    } else {
                        self.default_duration_us
                    };
                    self.next_us = start.saturating_add(duration);

                    let frame_ticks = decoded.packet().duration;
                    let loop_end = if let Some(ticks) = source_ticks {
                        let origin = *self.origin_ticks.get_or_insert(ticks);
                        let duration_ticks = if frame_ticks > 0 {
                            frame_ticks
                        } else {
                            duration
                                .rescale(Rational(1, 1_000_000), self.time_base)
                                .max(1)
                        };
                        LoopDuration::from_ticks(
                            ticks.saturating_sub(origin).saturating_add(duration_ticks),
                            self.time_base.numerator(),
                            self.time_base.denominator(),
                        )
                    } else {
                        LoopDuration::from_ticks(start.saturating_add(duration), 1, 1_000_000)
                    };

                    return Ok(Some((
                        DecodedFrame {
                            video: decoded,
                            alpha,
                        },
                        start,
                        duration,
                        loop_end,
                    )));
                }
                Err(Error::Eof) => return Ok(None),
                Err(Error::Other { errno }) if errno == ffmpeg_next::ffi::EAGAIN => {}
                Err(error) => return Err(error.into()),
            }
            if self.eof {
                return Ok(None);
            }
            let mut packet = Packet::empty();
            match packet.read(&mut self.input) {
                Ok(()) if packet.stream() == self.stream => self.decoder.send_packet(&packet)?,
                Ok(()) => {}
                Err(Error::Eof) => {
                    self.eof = true;
                    self.decoder.send_eof()?;
                }
                Err(error) => return Err(error).context("reading graphics source"),
            }
        }
    }

    pub fn next_frame(&mut self) -> Result<Option<SourceFrame>> {
        self.next_frame_with_cancel(|| false)
    }

    pub fn next_frame_with_cancel(
        &mut self,
        cancelled: impl Fn() -> bool,
    ) -> Result<Option<SourceFrame>> {
        let Some((decoded, start, fallback_duration, loop_end)) = self
            .pending
            .take()
            .map(|frame| Ok(Some(frame)))
            .unwrap_or_else(|| self.decode(&cancelled))?
        else {
            return Ok(None);
        };
        self.pending = self.decode(&cancelled)?;
        let duration = self
            .pending
            .as_ref()
            .map(|(_, next, _, _)| next.saturating_sub(start))
            .filter(|duration| *duration > 0)
            .unwrap_or(fallback_duration);
        let alpha = decoded.alpha;
        let decoded = decoded.video;

        ensure!(
            decoded.width() <= 8192 && decoded.height() <= 8192,
            "graphics frame dimensions exceed limit"
        );
        let mode = self
            .container_alpha
            .or_else(|| preparation::frame_alpha(&decoded))
            .or(self.source_alpha)
            .unwrap_or(Alpha::Straight);
        let prepared = self.preparation.prepare_with_quality(
            &decoded,
            alpha.as_ref(),
            mode,
            self.dimensions,
            self.high_quality,
        )?;

        // A format without alpha must become a fully opaque graphics surface.
        let surface = LogoOverlay::from_yuva(&self.style, prepared, self.canvas.0, self.canvas.1)?;

        Ok(Some(SourceFrame {
            loop_end,
            timing: FrameTiming {
                start_us: start,
                duration_us: duration,
            },
            surface: Arc::new(surface),
        }))
    }

    pub(in crate::ffmpeg) fn initialize_origin(
        &mut self,
        cancelled: impl Fn() -> bool,
    ) -> Result<()> {
        if self.origin.is_none() && self.pending.is_none() {
            self.pending = self.decode(&cancelled)?;
        }

        Ok(())
    }

    /// Seek on the original source timeline, preserving its timestamp origin.
    /// If a demuxer cannot seek, the caller may rewind and decode forward.
    pub fn seek_to(&mut self, source_us: i64) -> Result<()> {
        self.input.seek(
            self.origin.unwrap_or(0).saturating_add(source_us.max(0)),
            ..,
        )?;
        self.decoder.flush();

        if let Some(alpha) = &mut self.alpha {
            alpha.flush();
        }

        self.eof = false;
        self.pending = None;
        self.next_us = source_us.max(0);

        Ok(())
    }

    pub fn duration_hint_us(&self) -> i64 {
        self.input.duration().max(0)
    }

    pub fn rewind(&mut self) -> Result<()> {
        self.input.seek(0, ..)?;
        self.decoder.flush();

        if let Some(alpha) = &mut self.alpha {
            alpha.flush();
        }

        self.eof = false;
        self.pending = None;
        self.origin = None;
        self.origin_ticks = None;
        self.next_us = 0;

        Ok(())
    }
}

pub(super) fn open_graphics_decoder(
    stream: &format::stream::Stream<'_>,
) -> Result<codec::decoder::Video> {
    let mut context = codec::context::Context::from_parameters(stream.parameters())?;
    let mut threading = codec::threading::Config::kind(codec::threading::Type::Frame);
    threading.count = 1;
    context.set_threading(threading);
    let alpha_decoder = match context.id() {
        codec::Id::VP8 => Some("libvpx"),
        codec::Id::VP9 => Some("libvpx-vp9"),
        _ => None,
    }
    .filter(|_| stream.metadata().get("alpha_mode") == Some("1"));

    if let Some(name) = alpha_decoder {
        // WebM alpha is a separate BlockAdditional bitstream. Native VPx
        // decoders ignore it even though the demuxer advertises alpha_mode=1.
        let decoder = codec::decoder::find_by_name(name).with_context(|| {
            format!("transparent WebM graphics require FFmpeg's {name} decoder")
        })?;

        return context
            .decoder()
            .open_as(decoder)
            .and_then(codec::decoder::Opened::video)
            .with_context(|| format!("opening {name} for transparent WebM graphics"));
    }

    context
        .decoder()
        .video()
        .context("opening graphics decoder")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use ffmpeg_next::util::format::pixel::Pixel;

    use crate::ffmpeg::sources::logo::blend_logo;

    use super::*;

    fn decoder(name: &str) -> LogoDecoder {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests_assets/storage/media_images")
            .join(name);
        let input = format::input(&path).unwrap();
        LogoDecoder::new(
            input,
            LogoStyle {
                scale: None,
                opacity: 1.0,
                position: "0:0".into(),
            },
            64,
            64,
        )
        .unwrap()
    }

    #[test]
    fn gif_preserves_variable_frame_durations_transparency_and_rewinds() {
        let mut decoder = decoder("animated-logo.gif");
        let mut frames = Vec::new();
        while let Some(frame) = decoder.next_frame().unwrap() {
            frames.push(frame);
            assert!(
                frames.len() <= 3,
                "embedded GIF loop count must not loop the demuxer"
            );
        }
        assert_eq!(
            frames.iter().map(|frame| frame.timing).collect::<Vec<_>>(),
            vec![
                FrameTiming {
                    start_us: 0,
                    duration_us: 100_000
                },
                FrameTiming {
                    start_us: 100_000,
                    duration_us: 300_000
                },
                FrameTiming {
                    start_us: 400_000,
                    duration_us: 200_000
                },
            ]
        );
        for frame in &frames {
            let video = &frame.surface.frame;
            assert_eq!(video.data(3)[0], 0);
            assert_eq!(video.data(3)[16 * video.stride(3) + 16], 255);
        }
        decoder.rewind().unwrap();
        let first = decoder.next_frame().unwrap().unwrap();
        assert_eq!(first.timing, frames[0].timing);
        assert_eq!(first.surface.frame.data(0), frames[0].surface.frame.data(0));
    }

    #[test]
    fn fractional_video_period_is_retained_without_rounding_each_loop() {
        let mut decoder = decoder("animated-logo-fractional.mov");
        decoder.next_frame().unwrap().unwrap();
        let last = decoder.next_frame().unwrap().unwrap();
        assert!(decoder.next_frame().unwrap().is_none());
        assert_eq!(last.loop_end.position_us(2_002_000_000), 0);
        assert_eq!(last.loop_end.position_us(2_002_000_001), 1);
    }

    #[test]
    fn alpha_video_and_opaque_video_have_correct_alpha_planes() {
        for (name, alpha, count) in [
            ("animated-logo-alpha.mov", 0, 3),
            ("animated-logo-video.mkv", 255, 80),
        ] {
            let mut decoder = decoder(name);
            let mut frames = 0;
            while let Some(frame) = decoder.next_frame().unwrap() {
                assert_eq!(frame.surface.frame.data(3)[0], alpha);
                assert_eq!(frame.timing.start_us, frames * 100_000);
                assert_eq!(frame.timing.duration_us, 100_000);
                frames += 1;
            }
            assert_eq!(frames, count);
        }
    }

    #[test]
    fn transparent_webm_preserves_alpha_through_animation_still_loading_and_blending() {
        for (file, codec) in [
            ("animated-logo-alpha-vp8.webm", "libvpx"),
            ("animated-logo-alpha-vp9.webm", "libvpx-vp9"),
        ] {
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests_assets/storage/media_images")
                .join(file);
            let input = format::input(&path).unwrap();
            let stream = input.streams().best(media::Type::Video).unwrap();
            assert_eq!(stream.metadata().get("alpha_mode"), Some("1"));
            let style = LogoStyle {
                scale: None,
                opacity: 1.0,
                position: "0:0".into(),
            };

            if codec::decoder::find_by_name(codec).is_none() {
                let error = LogoDecoder::new(input, style, 64, 64).err().unwrap();
                assert!(error.to_string().contains(codec));
                continue;
            }

            let mut decoder = LogoDecoder::new(input, style.clone(), 64, 64).unwrap();
            assert_eq!(decoder.decoder.codec().unwrap().name(), codec);

            for index in 0..3 {
                let source = decoder.next_frame().unwrap().unwrap();
                assert_eq!(source.timing.start_us, index * 100_000);
                assert_eq!(source.timing.duration_us, 100_000);
                let alpha = source.surface.frame.data(3);
                let expected = if index % 2 == 0 {
                    [0, 128, 255]
                } else {
                    [255, 128, 0]
                };

                for (x, expected) in [4, 16, 28].into_iter().zip(expected) {
                    assert!(alpha[x].abs_diff(expected) <= 2, "{file}: alpha at {x}");
                }

                if index == 0 {
                    let mut background = frame::Video::new(Pixel::YUV420P, 32, 32);
                    background.data_mut(0).fill(40);
                    background.data_mut(1).fill(128);
                    background.data_mut(2).fill(128);
                    blend_logo(&mut background, &source.surface, 1.0);
                    assert_eq!(
                        background.data(0)[4],
                        40,
                        "transparent pixels must retain video"
                    );
                    assert!((105..=115).contains(&background.data(0)[16]));
                    assert!(background.data(0)[28] > 170);
                }
            }

            assert!(decoder.next_frame().unwrap().is_none());
            decoder.rewind().unwrap();
            assert_eq!(
                decoder.next_frame().unwrap().unwrap().surface.frame.data(3)[4],
                0
            );
            let still =
                LogoOverlay::from_input(&style, format::input(&path).unwrap(), file, 64, 64)
                    .unwrap();
            assert_eq!(still.frame.data(3)[4], 0);
            assert!(still.frame.data(3)[16].abs_diff(128) <= 2);
            assert_eq!(still.frame.data(3)[28], 255);
        }
    }

    #[test]
    fn webm_without_alpha_metadata_keeps_the_default_decoder() {
        for (file, id) in [
            ("animated-logo-alpha-vp8.webm", codec::Id::VP8),
            ("animated-logo-alpha-vp9.webm", codec::Id::VP9),
        ] {
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests_assets/storage/media_images")
                .join(file);
            let mut input = format::input(&path).unwrap();
            let index = input.streams().best(media::Type::Video).unwrap().index();
            input
                .stream_mut(index)
                .unwrap()
                .set_metadata(ffmpeg_next::Dictionary::new());
            let stream = input.stream(index).unwrap();
            let decoder = open_graphics_decoder(&stream).unwrap();
            assert_eq!(
                decoder.codec().unwrap().name(),
                codec::decoder::find(id).unwrap().name()
            );
        }
    }

    #[test]
    fn prores_4444_and_ffv1_preserve_transparent_and_semitransparent_pixels() {
        for file in [
            "animated-logo-alpha-prores.mov",
            "animated-logo-alpha-ffv1.mkv",
        ] {
            let mut decoder = decoder(file);

            for index in 0..3 {
                let source = decoder.next_frame().unwrap().unwrap();
                let alpha = source.surface.frame.data(3);
                let expected = if index % 2 == 0 {
                    [0, 128, 255]
                } else {
                    [255, 128, 0]
                };

                for (x, expected) in [4, 16, 28].into_iter().zip(expected) {
                    assert!(alpha[x].abs_diff(expected) <= 2, "{file}: alpha at {x}");
                }
            }

            assert!(decoder.next_frame().unwrap().is_none());
        }
    }
}

#[cfg(test)]
mod avif_tests;
