//! A destination-free video/audio profile for output destinations.

use std::time::{Duration, Instant};

use anyhow::Result;
use ffmpeg_next::{codec, frame};

use crate::{EncodingConfig, OutputConfig};

use super::{
    EncodedPacket, EncodingWorkers,
    audio::{AudioFrameBuffer, open_audio_encoder},
    video::open_video_encoder,
};

pub(in crate::output) struct EncodingProfile {
    workers: EncodingWorkers,
    audio_buffer: Option<AudioFrameBuffer>,
    timing: Option<crate::output::diagnostics::PrimaryTiming>,
    video_time_base: ffmpeg_next::Rational,
}

impl EncodingProfile {
    pub(in crate::output) fn open_shared(
        program: &OutputConfig,
        encode: &EncodingConfig,
        gop_size: u32,
        shared_audio: Option<codec::Parameters>,
    ) -> Result<(Self, codec::Parameters, codec::Parameters)> {
        let global_header = true;
        let closed_gop = true;
        let mut cfg = program.clone();
        cfg.width = encode.width;
        cfg.height = encode.height;
        cfg.video_codec = encode.video_codec.clone();
        cfg.video_options = encode.video_options.clone();
        cfg.audio_codec = encode.audio_codec.clone();
        cfg.audio_options = encode.audio_options.clone();
        cfg.audio_bitrate = encode.audio_bitrate;
        let video = open_video_encoder(
            0,
            &cfg,
            global_header,
            None,
            (program.width, program.height),
            Some(gop_size),
            closed_gop,
        )?;
        let video_parameters = codec::Parameters::from(&video.encoder);
        let (audio_parameters, audio_streams) = if let Some(parameters) = shared_audio {
            (parameters, Vec::new())
        } else {
            let audio = open_audio_encoder(1, &cfg, global_header, None)?;
            let parameters = codec::Parameters::from(&audio.encoder);

            (parameters, vec![audio])
        };
        let has_audio = !audio_streams.is_empty();
        let workers = EncodingWorkers::start(vec![video], audio_streams)?;

        Ok((
            Self {
                timing: (program.output_timing_diagnostics && log::log_enabled!(log::Level::Trace))
                    .then(|| {
                        crate::output::diagnostics::PrimaryTiming::new(
                            program.channel_id.unwrap_or_default(),
                        )
                    }),
                video_time_base: program.video_time_base,
                audio_buffer: has_audio
                    .then(|| AudioFrameBuffer::new(cfg.sample_rate, workers.audio_frame_size)),
                workers,
            },
            video_parameters,
            audio_parameters,
        ))
    }

    pub(in crate::output) fn video_frame(
        &mut self,
        frame: &frame::Video,
        mut emit: impl FnMut(EncodedPacket) -> Result<()>,
    ) -> Result<()> {
        let entered = self.timing.as_mut().map(|timing| {
            timing.begin();
            Instant::now()
        });
        let timing = &mut self.timing;
        let result = self
            .workers
            .submit(EncodingWorkers::video_frame(frame)?, |packet| {
                let queued = entered.map(|_| Instant::now());
                let result = emit(packet);
                if let (Some(timing), Some(queued)) = (timing.as_mut(), queued) {
                    timing.packet(queued.elapsed());
                }
                result
            });
        if let (Some(timing), Some(entered)) = (self.timing.as_mut(), entered) {
            timing.finish(
                frame.pts(),
                self.video_time_base,
                entered,
                entered.elapsed(),
                Duration::ZERO,
            );
        }

        result
    }

    pub(in crate::output) fn audio(
        &mut self,
        frame: &frame::Audio,
        mut emit: impl FnMut(EncodedPacket) -> Result<()>,
    ) -> Result<()> {
        let Some(buffer) = &mut self.audio_buffer else {
            return Ok(());
        };

        for frame in buffer.push(frame)? {
            self.workers
                .submit(EncodingWorkers::audio_frame(&frame)?, &mut emit)?;
        }

        Ok(())
    }

    pub(in crate::output) fn finish(
        mut self,
        mut emit: impl FnMut(EncodedPacket) -> Result<()>,
    ) -> Result<()> {
        if let Some(buffer) = &mut self.audio_buffer {
            for frame in buffer.finish()? {
                self.workers
                    .submit(EncodingWorkers::audio_frame(&frame)?, &mut emit)?;
            }
        }

        self.workers.finish(emit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn borrowed_audio_does_not_open_an_encoder_or_allocate_a_sample_buffer() {
        ffmpeg_next::init().unwrap();

        let program = OutputConfig::new(64, 64, 25, 48_000);
        let audio = open_audio_encoder(1, &program, true, None).unwrap();
        let parameters = codec::Parameters::from(&audio.encoder);
        let encode = EncodingConfig {
            width: 64,
            height: 64,
            video_codec: "libx264".into(),
            video_options: [("preset".into(), "ultrafast".into())].into(),
            // Opening this encoder would fail: the borrowed parameters must suffice.
            audio_codec: "nonexistent-audio-encoder".into(),
            audio_options: Default::default(),
            audio_bitrate: program.audio_bitrate,
        };
        let (profile, _, returned) =
            EncodingProfile::open_shared(&program, &encode, 25, Some(parameters)).unwrap();

        assert!(profile.audio_buffer.is_none());
        assert_eq!(profile.workers.audio_frame_size, 0);
        assert_eq!(returned.id(), codec::Id::AAC);
        profile.finish(|_| Ok(())).unwrap();
    }
}
