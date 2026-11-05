//! Decode associated AVIF planes together without retaining an unbounded track backlog.
use std::collections::VecDeque;

use anyhow::{Context, Result, ensure};
use ffmpeg_next::{Error, Packet, Rational, Rescale, codec, format, frame};

use super::decoder::open_graphics_decoder;

const FRAME_LIMIT: usize = 32;
const BYTE_LIMIT: usize = 256 * 1024 * 1024;

pub(super) struct AuxiliaryDecoder {
    decoder: codec::decoder::Video,
    stream: usize,
    time_base: Rational,
    colors: VecDeque<frame::Video>,
    alphas: VecDeque<frame::Video>,
    eof: bool,
}

impl AuxiliaryDecoder {
    pub fn new(input: &format::context::Input, index: usize) -> Result<Self> {
        let stream = input
            .stream(index)
            .context("AVIF alpha stream is missing")?;
        ensure!(
            stream.parameters().id() == codec::Id::AV1,
            "AVIF alpha must be AV1"
        );
        let decoder = open_graphics_decoder(&stream)?;
        ensure!(
            decoder.width() <= 8192 && decoder.height() <= 8192,
            "AVIF alpha dimensions exceed limit"
        );
        let time_base = stream.time_base();
        ensure!(
            time_base.numerator() > 0 && time_base.denominator() > 0,
            "AVIF alpha has invalid time base"
        );

        Ok(Self {
            decoder,
            stream: index,
            time_base,
            colors: VecDeque::new(),
            alphas: VecDeque::new(),
            eof: false,
        })
    }

    pub fn next_frame(
        &mut self,
        input: &mut format::context::Input,
        color: &mut codec::decoder::Video,
        color_stream: usize,
        color_time_base: Rational,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Option<(frame::Video, frame::Video)>> {
        loop {
            ensure!(!cancelled(), "graphics decoding cancelled");
            receive_frames(color, &mut self.colors, &self.alphas)?;
            receive_frames(&mut self.decoder, &mut self.alphas, &self.colors)?;

            if let (Some(video), Some(alpha)) = (self.colors.front(), self.alphas.front()) {
                ensure!(
                    timestamps_match(video, color_time_base, alpha, self.time_base),
                    "AVIF color and alpha timestamps do not match"
                );
                ensure!(
                    video.width() == alpha.width() && video.height() == alpha.height(),
                    "AVIF color and alpha dimensions do not match"
                );

                return Ok(self.colors.pop_front().zip(self.alphas.pop_front()));
            }

            if self.eof {
                ensure!(
                    self.colors.is_empty() && self.alphas.is_empty(),
                    "AVIF is missing a color or alpha frame"
                );

                return Ok(None);
            }

            let mut packet = Packet::empty();

            match packet.read(input) {
                Ok(()) if packet.stream() == color_stream => color.send_packet(&packet)?,
                Ok(()) if packet.stream() == self.stream => self.decoder.send_packet(&packet)?,
                Ok(()) => {}
                Err(Error::Eof) => {
                    color.send_eof()?;
                    self.decoder.send_eof()?;
                    self.eof = true;
                }
                Err(error) => return Err(error).context("reading AVIF color/alpha source"),
            }
        }
    }

    pub fn flush(&mut self) {
        self.decoder.flush();
        self.colors.clear();
        self.alphas.clear();
        self.eof = false;
    }
}

fn receive_frames(
    decoder: &mut codec::decoder::Video,
    frames: &mut VecDeque<frame::Video>,
    other: &VecDeque<frame::Video>,
) -> Result<()> {
    loop {
        let mut frame = frame::Video::empty();

        match decoder.receive_frame(&mut frame) {
            Ok(()) => {
                ensure!(
                    frame.width() <= 8192 && frame.height() <= 8192,
                    "AVIF frame dimensions exceed limit"
                );
                let bytes: usize = frames
                    .iter()
                    .chain(other.iter())
                    .chain(std::iter::once(&frame))
                    .map(|frame| {
                        (0..frame.planes())
                            .map(|plane| frame.data(plane).len())
                            .sum::<usize>()
                    })
                    .sum();
                ensure!(
                    frames.len() + other.len() < FRAME_LIMIT && bytes <= BYTE_LIMIT,
                    "AVIF color/alpha buffering exceeded its bounded capacity"
                );
                frames.push_back(frame);
            }
            Err(Error::Eof) => return Ok(()),
            Err(Error::Other { errno }) if errno == ffmpeg_next::ffi::EAGAIN => return Ok(()),
            Err(error) => return Err(error).context("decoding AVIF plane"),
        }
    }
}

fn timestamps_match(
    color: &frame::Video,
    color_base: Rational,
    alpha: &frame::Video,
    alpha_base: Rational,
) -> bool {
    match (
        color.timestamp().or(color.pts()),
        alpha.timestamp().or(alpha.pts()),
    ) {
        (Some(color), Some(alpha)) => {
            color
                .rescale(color_base, Rational(1, 1_000_000))
                .abs_diff(alpha.rescale(alpha_base, Rational(1, 1_000_000)))
                <= 1
        }
        (None, None) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_and_alpha_must_match_in_media_time_rather_than_raw_ticks() {
        let mut color = frame::Video::empty();
        color.set_pts(Some(42));
        let mut alpha = frame::Video::empty();
        alpha.set_pts(Some(1680));
        assert!(timestamps_match(
            &color,
            Rational(1, 25),
            &alpha,
            Rational(1, 1000)
        ));
        alpha.set_pts(Some(1720));
        assert!(!timestamps_match(
            &color,
            Rational(1, 25),
            &alpha,
            Rational(1, 1000)
        ));
        alpha.set_pts(None);
        assert!(!timestamps_match(
            &color,
            Rational(1, 25),
            &alpha,
            Rational(1, 1000)
        ));
        color.set_pts(None);
        assert!(timestamps_match(
            &color,
            Rational(1, 25),
            &alpha,
            Rational(1, 1000)
        ));
    }
}
