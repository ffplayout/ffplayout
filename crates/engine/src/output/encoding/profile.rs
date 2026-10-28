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
    audio_buffer: AudioFrameBuffer,
    timing: Option<crate::output::diagnostics::PrimaryTiming>,
    video_time_base: ffmpeg_next::Rational,
}

impl EncodingProfile {
    pub(in crate::output) fn open_shared(
        program: &OutputConfig,
        encode: &EncodingConfig,
        gop_size: u32,
    ) -> Result<(Self, codec::Parameters, codec::Parameters)> {
        Self::open_profile(program, encode, true, true, gop_size)
    }

    fn open_profile(
        program: &OutputConfig,
        encode: &EncodingConfig,
        global_header: bool,
        closed_gop: bool,
        gop_size: u32,
    ) -> Result<(Self, codec::Parameters, codec::Parameters)> {
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
        let audio = open_audio_encoder(1, &cfg, global_header, None)?;
        let video_parameters = codec::Parameters::from(&video.encoder);
        let audio_parameters = codec::Parameters::from(&audio.encoder);
        let workers = EncodingWorkers::start(vec![video], vec![audio])?;

        Ok((
            Self {
                timing: (program.output_timing_diagnostics && log::log_enabled!(log::Level::Trace))
                    .then(|| {
                        crate::output::diagnostics::PrimaryTiming::new(
                            program.channel_id.unwrap_or_default(),
                        )
                    }),
                video_time_base: program.video_time_base,
                audio_buffer: AudioFrameBuffer::new(cfg.sample_rate, workers.audio_frame_size),
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
        for frame in self.audio_buffer.push(frame)? {
            self.workers
                .submit(EncodingWorkers::audio_frame(&frame)?, &mut emit)?;
        }

        Ok(())
    }

    pub(in crate::output) fn finish(
        mut self,
        mut emit: impl FnMut(EncodedPacket) -> Result<()>,
    ) -> Result<()> {
        for frame in self.audio_buffer.finish()? {
            self.workers
                .submit(EncodingWorkers::audio_frame(&frame)?, &mut emit)?;
        }
        self.workers.finish(emit)
    }
}
