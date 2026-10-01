use std::{collections::VecDeque, time::Duration};

use anyhow::Result;
use ffmpeg_next::{Rational, Rescale, frame};

mod stats;

pub(crate) use stats::AudioStatsOverlay;

use crate::{
    BufferedLoudnessAnalysis, LiveDynamicsProcessor, LiveLoudnessMeasurement, LiveLoudnessMetrics,
    LiveLoudnessProcessor,
    output::FrameOutput,
    utils::ffmpeg::{make_audio_frame_writable, reference_audio_frame, reference_video_frame},
};

enum PendingFrame {
    Video(frame::Video),
    Audio(frame::Audio, Option<BufferedLoudnessAnalysis>),
}

/// Preserve decode order and source timestamps while holding a bounded A/V preview.
/// The desktop output supplies backpressure and its normal audio master clock.
pub(crate) struct LoudnessPreview<'a, O> {
    output: &'a mut O,
    processor: LiveLoudnessProcessor,
    dynamics: Option<LiveDynamicsProcessor>,
    stats: Option<AudioStatsOverlay>,
    lookahead_samples: usize,
    buffered_samples: usize,
    pending: VecDeque<PendingFrame>,
    video_finished: bool,
    video_time_base: Rational,
    sample_rate: u32,
    latest_video_sample: i64,
}

impl<'a, O: FrameOutput> LoudnessPreview<'a, O> {
    pub(crate) fn new(
        output: &'a mut O,
        mut processor: LiveLoudnessProcessor,
        measurement: LiveLoudnessMeasurement,
        lookahead: Duration,
        sample_rate: u32,
        video_time_base: Rational,
    ) -> Self {
        processor.set_measurement(measurement);

        Self {
            output,
            processor,
            dynamics: None,
            stats: None,
            lookahead_samples: (lookahead.as_secs_f64() * f64::from(sample_rate)) as usize,
            buffered_samples: 0,
            pending: VecDeque::new(),
            video_finished: false,
            video_time_base,
            sample_rate,
            latest_video_sample: 0,
        }
    }

    pub(crate) fn set_dynamics(&mut self, dynamics: Option<LiveDynamicsProcessor>) {
        self.dynamics = dynamics;
    }

    pub(crate) fn set_stats(&mut self, stats: Option<AudioStatsOverlay>) {
        self.stats = stats;
    }

    fn required_samples(&self) -> usize {
        self.dynamics.as_ref().map_or(
            self.processor.peak_lookahead_samples(),
            LiveDynamicsProcessor::lookahead_samples,
        )
    }

    pub(crate) fn metrics(&self) -> LiveLoudnessMetrics {
        self.dynamics
            .as_ref()
            .map_or_else(|| self.processor.metrics(), LiveDynamicsProcessor::metrics)
    }

    fn drain(&mut self, flush: bool) -> Result<()> {
        let preview = self.lookahead_samples.max(self.required_samples());

        while let Some(front) = self.pending.front() {
            let samples = match front {
                PendingFrame::Audio(audio, _) => audio.samples(),
                PendingFrame::Video(_) => 0,
            };

            let front_sample = match front {
                PendingFrame::Audio(audio, _) => audio.pts().unwrap_or_default() + samples as i64,
                PendingFrame::Video(video) => video
                    .pts()
                    .unwrap_or_default()
                    .rescale(self.video_time_base, Rational(1, self.sample_rate as i32)),
            };
            // Video can continue after audio EOF or through missing audio. Do not
            // retain that video indefinitely waiting for samples that never arrive.
            let video_advanced =
                self.latest_video_sample.saturating_sub(front_sample) >= preview as i64;

            if !flush && !video_advanced && self.buffered_samples.saturating_sub(samples) < preview
            {
                break;
            }

            match self.pending.pop_front() {
                Some(PendingFrame::Video(mut video)) => {
                    if let Some(stats) = self.stats.as_mut() {
                        stats.blend(&mut video, self.video_time_base)?;
                    }

                    self.output.encode_video(&video)?;
                }
                Some(PendingFrame::Audio(mut audio, analysis)) => {
                    self.buffered_samples -= audio.samples();
                    let limit = self.required_samples();
                    let future: Vec<[f32; 2]> = self
                        .pending
                        .iter()
                        .filter_map(|item| match item {
                            PendingFrame::Audio(audio, _) => Some(audio),
                            PendingFrame::Video(_) => None,
                        })
                        .flat_map(|audio| {
                            audio
                                .plane::<f32>(0)
                                .iter()
                                .zip(audio.plane::<f32>(1))
                                .map(|(&left, &right)| [left, right])
                        })
                        .take(limit)
                        .collect();

                    if let Some(stats) = self.stats.as_mut() {
                        stats.observe_input(&audio);
                    }

                    if let Some(dynamics) = self.dynamics.as_mut() {
                        dynamics.process(&mut audio, &future);
                    } else if let Some(analysis) = analysis {
                        self.processor
                            .apply_buffered_gain(&mut audio, &analysis, &future);
                    } else {
                        self.processor.process_with_lookahead(&mut audio, &future);
                    }

                    let metrics = self.metrics();
                    let compressor = self
                        .dynamics
                        .as_ref()
                        .map(LiveDynamicsProcessor::compressor_gain_reduction_db);
                    let pause = self
                        .dynamics
                        .as_ref()
                        .map(LiveDynamicsProcessor::pause_status);

                    if let Some(stats) = self.stats.as_mut() {
                        stats.observe_output(&audio, metrics, compressor, pause);
                    }

                    self.output.encode_audio(&audio)?;
                }
                None => break,
            }
        }

        Ok(())
    }

    pub(crate) fn finish(&mut self) -> Result<()> {
        self.drain(true)?;

        if self.video_finished {
            self.output.video_finished()?;
        }

        Ok(())
    }
}

impl<O: FrameOutput> FrameOutput for LoudnessPreview<'_, O> {
    fn audio_frame_size(&self) -> usize {
        self.output.audio_frame_size()
    }

    fn encode_video(&mut self, video: &frame::Video) -> Result<()> {
        self.latest_video_sample = self.latest_video_sample.max(
            video
                .pts()
                .unwrap_or_default()
                .rescale(self.video_time_base, Rational(1, self.sample_rate as i32)),
        );
        self.pending
            .push_back(PendingFrame::Video(reference_video_frame(video)?));
        self.drain(false)
    }

    fn encode_audio(&mut self, audio: &frame::Audio) -> Result<()> {
        let mut audio = reference_audio_frame(audio)?;
        make_audio_frame_writable(&mut audio)?;

        let analysis = (self.lookahead_samples > 0 && self.dynamics.is_none())
            .then(|| self.processor.analyze_buffered(&mut audio));

        self.buffered_samples += audio.samples();
        self.pending.push_back(PendingFrame::Audio(audio, analysis));
        self.drain(false)
    }

    fn set_video_end(&mut self, end: Option<i64>) -> Result<()> {
        self.output.set_video_end(end)
    }

    fn video_decoded(&mut self) -> Result<()> {
        self.output.video_decoded()
    }

    fn video_finished(&mut self) -> Result<()> {
        // Audio may still be decoded after video EOF. Finish only after the preview is drained.
        self.video_finished = true;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use ffmpeg_next::{
        ChannelLayout,
        format::{Sample, sample::Type},
    };

    use crate::LiveLoudnessConfig;

    use super::*;

    #[derive(Default)]
    struct Output {
        video_pts: Vec<i64>,
        audio_samples: usize,
        peak: f32,
        finished: bool,
        audio_rms: Vec<f64>,
    }

    impl FrameOutput for Output {
        fn audio_frame_size(&self) -> usize {
            4800
        }

        fn encode_video(&mut self, video: &frame::Video) -> Result<()> {
            self.video_pts.push(video.pts().unwrap());
            Ok(())
        }

        fn encode_audio(&mut self, audio: &frame::Audio) -> Result<()> {
            self.audio_samples += audio.samples();
            self.audio_rms.push(
                (audio
                    .plane::<f32>(0)
                    .iter()
                    .map(|&sample| f64::from(sample).powi(2))
                    .sum::<f64>()
                    / audio.samples() as f64)
                    .sqrt(),
            );

            for sample in audio.plane::<f32>(0) {
                self.peak = self.peak.max(sample.abs());
            }
            Ok(())
        }

        fn video_finished(&mut self) -> Result<()> {
            self.finished = true;
            Ok(())
        }
    }

    fn video(pts: i64) -> frame::Video {
        let mut video = frame::Video::new(ffmpeg_next::format::Pixel::YUV420P, 16, 16);
        video.set_pts(Some(pts));
        video
    }

    fn audio(pts: i64) -> frame::Audio {
        let mut audio = frame::Audio::new(Sample::F32(Type::Planar), 4800, ChannelLayout::STEREO);
        audio.set_rate(48000);
        audio.set_pts(Some(pts));
        audio.plane_mut::<f32>(0).fill(2.0);
        audio.plane_mut::<f32>(1).fill(2.0);
        audio
    }

    #[test]
    fn three_second_preview_plays_before_eof_and_flushes_audio_and_video() {
        ffmpeg_next::init().unwrap();
        let mut output = Output::default();
        let processor = LiveLoudnessProcessor::new(48000, LiveLoudnessConfig::default()).unwrap();
        let mut preview = LoudnessPreview::new(
            &mut output,
            processor,
            LiveLoudnessMeasurement::ShortTerm,
            Duration::from_secs(3),
            48000,
            Rational(1, 10),
        );

        for index in 0..30 {
            preview.encode_video(&video(index)).unwrap();
            preview.encode_audio(&audio(index * 4800)).unwrap();
        }

        assert_eq!(preview.output.audio_samples, 0);
        assert!(preview.output.video_pts.len() <= 1);
        assert!(preview.metrics().short_term_lufs.is_some());

        for index in 30..60 {
            preview.encode_video(&video(index)).unwrap();
            preview.encode_audio(&audio(index * 4800)).unwrap();
            assert!(preview.buffered_samples <= 148800);
        }

        assert!(preview.output.audio_samples > 0);
        assert!(preview.output.video_pts.len() > 1);
        preview.video_finished().unwrap();
        assert!(!preview.output.finished);
        preview.finish().unwrap();

        assert_eq!(preview.output.audio_samples, 60 * 4800);
        assert_eq!(preview.output.video_pts, (0..60).collect::<Vec<_>>());
        assert!(preview.output.peak <= 10_f32.powf(-1.0 / 20.0) + 0.00001);
        assert!(preview.output.finished);
        assert!(preview.pending.is_empty());
    }

    #[test]
    fn video_without_more_audio_continues_with_bounded_preview() {
        ffmpeg_next::init().unwrap();
        let mut output = Output::default();
        let processor = LiveLoudnessProcessor::new(48000, LiveLoudnessConfig::default()).unwrap();
        let mut preview = LoudnessPreview::new(
            &mut output,
            processor,
            LiveLoudnessMeasurement::ShortTerm,
            Duration::from_secs(3),
            48000,
            Rational(1, 10),
        );
        preview.encode_audio(&audio(0)).unwrap();

        for index in 0..100 {
            preview.encode_video(&video(index)).unwrap();
            assert!(preview.pending.len() <= 32);
        }

        assert_eq!(preview.output.audio_samples, 4800);
        assert!(!preview.output.video_pts.is_empty());
        preview.finish().unwrap();
        assert_eq!(preview.output.video_pts.len(), 100);
    }
    #[test]
    fn future_loud_section_does_not_attenuate_earlier_desktop_audio() {
        ffmpeg_next::init().unwrap();
        let mut output = Output::default();
        let processor = LiveLoudnessProcessor::new(48000, LiveLoudnessConfig::default()).unwrap();
        let mut preview = LoudnessPreview::new(
            &mut output,
            processor,
            LiveLoudnessMeasurement::ShortTerm,
            Duration::from_secs(3),
            48000,
            Rational(1, 10),
        );

        for index in 0..80 {
            let mut source = audio(index * 4800);
            let amplitude = if index < 40 { 0.01 } else { 0.4 };

            for channel in 0..2 {
                for (offset, sample) in source.plane_mut::<f32>(channel).iter_mut().enumerate() {
                    let position = index * 4800 + offset as i64;
                    *sample = (std::f64::consts::TAU * 997.0 * position as f64 / 48000.0).sin()
                        as f32
                        * amplitude;
                }
            }

            preview.encode_video(&video(index)).unwrap();
            preview.encode_audio(&source).unwrap();
        }

        preview.finish().unwrap();
        let quiet_rms = 0.01 / 2.0_f64.sqrt();

        for &level in &preview.output.audio_rms[10..39] {
            assert!(
                level >= quiet_rms * 0.99,
                "future loudness attenuated an earlier quiet frame: {level}"
            );
        }
    }
    #[test]
    fn live_dynamics_desktop_uses_short_preview_and_emits_audio_and_video() {
        ffmpeg_next::init().unwrap();
        let mut output = Output::default();
        let config = LiveLoudnessConfig::default();
        let processor = LiveLoudnessProcessor::new(48000, config).unwrap();
        let mut preview = LoudnessPreview::new(
            &mut output,
            processor,
            LiveLoudnessMeasurement::ShortTerm,
            Duration::ZERO,
            48000,
            Rational(1, 10),
        );
        preview.set_dynamics(Some(LiveDynamicsProcessor::new(48000, config).unwrap()));

        for index in 0..30 {
            let mut source = audio(index * 4800);
            let amplitude = if index < 20 { 0.01 } else { 0.4 };

            for channel in 0..2 {
                for (offset, sample) in source.plane_mut::<f32>(channel).iter_mut().enumerate() {
                    let position = index * 4800 + offset as i64;
                    *sample = (std::f64::consts::TAU * 997.0 * position as f64 / 48000.0).sin()
                        as f32
                        * amplitude;
                }
            }

            preview.encode_video(&video(index)).unwrap();
            preview.encode_audio(&source).unwrap();

            if index == 1 {
                assert!(preview.output.audio_samples > 0);
                assert!(!preview.output.video_pts.is_empty());
            }

            assert!(preview.buffered_samples <= 4800 + preview.required_samples());
        }

        preview.finish().unwrap();
        assert_eq!(preview.output.audio_samples, 30 * 4800);
        assert_eq!(preview.output.video_pts.len(), 30);
        assert!(preview.output.audio_rms[20] < 0.08);
    }
}
