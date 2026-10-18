use anyhow::{Context, Result};
use ffmpeg_next::{
    Packet, Rational, codec, frame, software::resampling, util::channel_layout::ChannelLayout,
};

use crate::{
    HlsVariant, OutputConfig,
    utils::{
        config::{audio_encoder_context, engine_audio_sample_format},
        ffmpeg::reference_audio_frame,
    },
};

pub(in crate::output) struct AudioOutputStream {
    pub(in crate::output) stream_index: usize,
    pub(in crate::output) encoder: codec::encoder::audio::Encoder,
    pub(in crate::output) resampler: Option<resampling::Context>,
}

impl AudioOutputStream {
    pub(in crate::output) fn submit(&mut self, frame: &frame::Audio) -> Result<()> {
        if let Some(resampler) = &mut self.resampler {
            let mut converted = frame::Audio::empty();
            resampler.run(frame, &mut converted)?;
            converted.set_pts(frame.pts());
            self.encoder.send_frame(&converted)?;
        } else {
            self.encoder.send_frame(frame)?;
        }

        Ok(())
    }

    pub(in crate::output) fn drain(
        &mut self,
        mut emit: impl FnMut(Packet, usize, Rational) -> Result<()>,
    ) -> Result<()> {
        let mut packet = Packet::empty();

        while self.encoder.receive_packet(&mut packet).is_ok() {
            emit(packet, self.stream_index, self.encoder.time_base())?;
            packet = Packet::empty();
        }

        Ok(())
    }

    pub(in crate::output) fn flush_resampler(
        &mut self,
        mut emit: impl FnMut(Packet, usize, Rational) -> Result<()>,
    ) -> Result<()> {
        loop {
            let Some(resampler) = &mut self.resampler else {
                return Ok(());
            };
            let output = *resampler.output();
            let mut converted = frame::Audio::new(
                output.format,
                self.encoder.frame_size() as usize,
                output.channel_layout,
            );
            converted.set_rate(output.rate);
            let delay = resampler.flush(&mut converted)?;

            if converted.samples() > 0 {
                self.encoder.send_frame(&converted)?;
            }
            self.drain(&mut emit)?;

            if delay.is_none() {
                return Ok(());
            }
        }
    }
}

pub(in crate::output) fn open_audio_encoder(
    stream_index: usize,
    cfg: &OutputConfig,
    global_header: bool,
    variant: Option<&HlsVariant>,
) -> Result<AudioOutputStream> {
    let audio_codec = if cfg.audio_codec.trim().is_empty() {
        codec::encoder::find(codec::Id::AAC)
    } else {
        codec::encoder::find_by_name(cfg.audio_codec.trim())
    }
    .with_context(|| format!("audio encoder {:?} not found", cfg.audio_codec))?;
    let audio_ctx = audio_encoder_context(
        audio_codec,
        &cfg.audio_options,
        cfg.sample_rate,
        variant.map_or(cfg.audio_bitrate, |variant| variant.audio_bitrate),
        cfg.audio_time_base,
        global_header,
    )
    .map_err(anyhow::Error::msg)?;
    let input_sample_format = engine_audio_sample_format();
    let encoder_sample_format = audio_ctx.format();
    let audio_encoder = audio_ctx.open_as(audio_codec)?;
    let resampler = (encoder_sample_format != input_sample_format)
        .then(|| {
            resampling::Context::get(
                input_sample_format,
                ChannelLayout::STEREO,
                cfg.sample_rate,
                encoder_sample_format,
                ChannelLayout::STEREO,
                cfg.sample_rate,
            )
        })
        .transpose()?;

    Ok(AudioOutputStream {
        stream_index,
        encoder: audio_encoder,
        resampler,
    })
}

/// Converts processed stereo buffers into codec-sized frames. The same framing
/// and timestamp-discontinuity policy applies to every encoding profile.
pub(in crate::output) struct AudioFrameBuffer {
    pub(in crate::output) samples: [std::collections::VecDeque<f32>; 2],
    pts: Option<i64>,
    rate: u32,
    size: usize,
}

impl AudioFrameBuffer {
    pub(in crate::output) fn new(rate: u32, size: usize) -> Self {
        Self {
            samples: Default::default(),
            pts: None,
            rate,
            size,
        }
    }

    pub(in crate::output) fn push(&mut self, frame: &frame::Audio) -> Result<Vec<frame::Audio>> {
        let mut frames = Vec::new();

        if !self.samples[0].is_empty()
            && frame
                .pts()
                .zip(self.pts)
                .is_some_and(|(pts, start)| pts != start + self.samples[0].len() as i64)
        {
            frames.extend(self.finish()?);
        }

        if self.samples[0].is_empty() {
            self.pts = frame.pts();
        }

        // Processed AAC-sized frames usually already match the encoder. Keep
        // their FFmpeg buffers rather than copying through a second FIFO.
        if self.size > 0
            && self.samples[0].is_empty()
            && frame.samples() == self.size
            && frame.rate() == self.rate
            && (0..2).all(|channel| {
                frame
                    .plane::<f32>(channel)
                    .iter()
                    .all(|sample| sample.is_finite())
            })
        {
            frames.push(reference_audio_frame(frame)?);
            self.pts = frame.pts().map(|pts| pts + self.size as i64);

            return Ok(frames);
        }

        for (channel, samples) in self.samples.iter_mut().enumerate() {
            samples.extend(
                frame
                    .plane::<f32>(channel)
                    .iter()
                    .map(|sample| if sample.is_finite() { *sample } else { 0.0 }),
            );
        }
        frames.extend(self.drain()?);

        Ok(frames)
    }

    pub(in crate::output) fn finish(&mut self) -> Result<Vec<frame::Audio>> {
        if self.samples[0].is_empty() {
            return Ok(Vec::new());
        }

        if self.size == 0 {
            return Err(anyhow::anyhow!(
                "audio encoder reported a frame size of zero"
            ));
        }

        for samples in &mut self.samples {
            samples.resize(self.size, 0.0);
        }
        self.drain()
    }

    fn drain(&mut self) -> Result<Vec<frame::Audio>> {
        if self.size == 0 {
            return Err(anyhow::anyhow!(
                "audio encoder reported a frame size of zero"
            ));
        }

        let mut frames = Vec::new();

        while self
            .samples
            .iter()
            .all(|samples| samples.len() >= self.size)
        {
            let mut frame = frame::Audio::new(
                ffmpeg_next::format::Sample::F32(ffmpeg_next::format::sample::Type::Planar),
                self.size,
                ChannelLayout::STEREO,
            );
            frame.set_rate(self.rate);
            frame.set_pts(self.pts);

            for (channel, samples) in self.samples.iter_mut().enumerate() {
                let plane = frame.plane_mut::<f32>(channel);
                let (front, back) = samples.as_slices();
                let first = front.len().min(self.size);
                plane[..first].copy_from_slice(&front[..first]);
                plane[first..self.size].copy_from_slice(&back[..self.size - first]);
                samples.drain(..self.size);
            }
            self.pts = self.pts.map(|pts| pts + self.size as i64);
            frames.push(frame);
        }

        Ok(frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audio(samples: &[f32], pts: i64) -> frame::Audio {
        let mut frame = frame::Audio::new(
            engine_audio_sample_format(),
            samples.len(),
            ChannelLayout::STEREO,
        );
        frame.set_rate(48_000);
        frame.set_pts(Some(pts));
        for channel in 0..2 {
            frame.plane_mut::<f32>(channel).copy_from_slice(samples);
        }

        frame
    }

    #[test]
    fn matching_frames_share_storage_without_changing_samples_or_timestamps() {
        let input = audio(&[0.1, 0.2, 0.3, 0.4], 123);
        let mut buffer = AudioFrameBuffer::new(48_000, 4);
        let output = buffer.push(&input).unwrap();
        assert_eq!(output.len(), 1);
        assert_eq!(output[0].pts(), input.pts());
        for channel in 0..2 {
            assert_eq!(output[0].plane::<f32>(channel), input.plane::<f32>(channel));
            assert_eq!(
                output[0].data(channel).as_ptr(),
                input.data(channel).as_ptr()
            );
        }
        assert!(buffer.finish().unwrap().is_empty());
    }

    #[test]
    fn partial_frames_preserve_order_across_fifo_wrap_and_pad_only_the_tail() {
        let mut buffer = AudioFrameBuffer::new(48_000, 4);
        assert!(
            buffer
                .push(&audio(&[0.0, 1.0, 2.0], 100))
                .unwrap()
                .is_empty()
        );
        let first = buffer.push(&audio(&[3.0, 4.0, 5.0], 103)).unwrap();
        let second = buffer.push(&audio(&[6.0, 7.0, 8.0], 106)).unwrap();
        let tail = buffer.finish().unwrap();
        for (output, expected, pts) in [
            (&first[0], [0.0, 1.0, 2.0, 3.0], 100),
            (&second[0], [4.0, 5.0, 6.0, 7.0], 104),
            (&tail[0], [8.0, 0.0, 0.0, 0.0], 108),
        ] {
            assert_eq!(output.pts(), Some(pts));
            for channel in 0..2 {
                assert_eq!(output.plane::<f32>(channel), expected);
            }
        }
    }

    #[test]
    fn matching_frames_still_sanitize_nonfinite_samples_and_flush_discontinuities() {
        let mut buffer = AudioFrameBuffer::new(48_000, 4);
        assert!(buffer.push(&audio(&[1.0, 2.0], 10)).unwrap().is_empty());
        let output = buffer
            .push(&audio(&[3.0, f32::NAN, f32::INFINITY, 4.0], 100))
            .unwrap();
        assert_eq!(output.len(), 2);
        assert_eq!(output[0].pts(), Some(10));
        assert_eq!(output[0].plane::<f32>(0), [1.0, 2.0, 0.0, 0.0]);
        assert_eq!(output[1].pts(), Some(100));
        assert_eq!(output[1].plane::<f32>(0), [3.0, 0.0, 0.0, 4.0]);
    }
}
