//! Incremental FFmpeg image/video decoding. Own this adapter on a preparation worker.
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, ensure};
use ffmpeg_next::{
    Error, Packet, Rational, Rescale, codec, format, frame, media, software::scaling,
    util::format::pixel::Pixel,
};

use super::logo::LogoOverlay;
use crate::{
    layout::logo_dimensions,
    source::{FrameTiming, LoopDuration},
    style::LogoStyle,
};

pub struct SourceFrame {
    pub timing: FrameTiming,
    pub loop_end: LoopDuration,
    pub surface: Arc<LogoOverlay>,
}

pub struct LogoDecoder {
    input: format::context::Input,
    decoder: codec::decoder::Video,
    scaler: Option<scaling::Context>,
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
    pending: Option<(frame::Video, i64, i64, LoopDuration)>,
}

impl LogoDecoder {
    pub fn new(
        input: format::context::Input,
        style: LogoStyle,
        width: u32,
        height: u32,
    ) -> Result<Self> {
        let stream = input
            .streams()
            .best(media::Type::Video)
            .context("graphics source has no video stream")?;
        let mut context = codec::context::Context::from_parameters(stream.parameters())?;
        let mut threading = codec::threading::Config::kind(codec::threading::Type::Frame);
        threading.count = 1;
        context.set_threading(threading);
        let decoder = context.decoder().video()?;
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
            scaler: None,
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

    fn decode(
        &mut self,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Option<(frame::Video, i64, i64, LoopDuration)>> {
        loop {
            ensure!(!cancelled(), "graphics decoding cancelled");
            let mut decoded = frame::Video::empty();
            match self.decoder.receive_frame(&mut decoded) {
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

                    return Ok(Some((decoded, start, duration, loop_end)));
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
        ensure!(
            decoded.width() <= 8192 && decoded.height() <= 8192,
            "graphics frame dimensions exceed limit"
        );
        let changed = self.scaler.as_ref().is_none_or(|scaler| {
            let input = scaler.input();
            input.width != decoded.width()
                || input.height != decoded.height()
                || input.format != decoded.format()
        });
        if changed {
            self.scaler = Some(scaling::Context::get(
                decoded.format(),
                decoded.width(),
                decoded.height(),
                Pixel::YUVA420P,
                self.dimensions.0,
                self.dimensions.1,
                scaling::flag::Flags::BILINEAR,
            )?);
        }
        let mut prepared = frame::Video::empty();
        self.scaler
            .as_mut()
            .ok_or_else(|| anyhow!("graphics scaler unavailable"))?
            .run(&decoded, &mut prepared)?;
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

    pub fn rewind(&mut self) -> Result<()> {
        self.input.seek(0, ..)?;
        self.decoder.flush();
        self.eof = false;
        self.pending = None;
        self.origin = None;
        self.origin_ticks = None;
        self.next_us = 0;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
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
}
