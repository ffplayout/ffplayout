use std::collections::VecDeque;

use anyhow::Result;
use ffmpeg_next::{Rational, Rescale, frame};

use crate::{
    benchmark::{self, Stage},
    compositor::logo::LogoOverlay,
    output::FrameOutput,
    utils::{
        config::OutputConfig,
        ffmpeg::{make_audio_frame_writable, reference_audio_frame, reference_video_frame},
    },
};

use super::{
    AudioEffect, LiveDynamicsProcessor, LiveLoudnessControl, LoudnessScope, collect_audio_preview,
    volume::GainEffect,
};

/// Processor history survives clip and live-session boundaries; preview frames do not.
#[derive(Default)]
pub(crate) struct ProgramAudioState {
    processor: Option<LiveDynamicsProcessor>,
    volume: Option<GainEffect>,
}

enum PendingFrame {
    Video(frame::Video),
    Audio {
        frame: frame::Audio,
        live: bool,
        hold_gain: bool,
    },
}

/// A single processing stage shared by playlist playback and live takeover.
/// Audio and video retain their timestamps and are held together for lookahead.
pub(crate) struct ProgramAudioOutput<'a, O> {
    output: &'a mut O,
    state: &'a mut ProgramAudioState,
    control: LiveLoudnessControl,
    scope: LoudnessScope,
    pending: VecDeque<PendingFrame>,
    buffered_samples: usize,
    latest_video_sample: i64,
    sample_rate: u32,
    video_time_base: Rational,
    video_decoded: bool,
    video_finished: bool,
    last_audio_source: Option<bool>,
}

impl<'a, O: FrameOutput> ProgramAudioOutput<'a, O> {
    pub(crate) fn new(
        output: &'a mut O,
        state: &'a mut ProgramAudioState,
        config: &OutputConfig,
    ) -> Result<Self> {
        if config.loudness_scope != LoudnessScope::Off && state.processor.is_none() {
            state.processor = Some(LiveDynamicsProcessor::new(
                config.sample_rate,
                config.live_loudness_control.settings().config,
            )?);
        }

        if let Some(processor) = state.processor.as_mut() {
            processor.begin_segment();
        }

        if state.volume.is_none() {
            state.volume = Some(GainEffect::new(
                config.audio_effects.clone(),
                config.sample_rate,
            ));
        }

        Ok(Self {
            output,
            state,
            control: config.live_loudness_control.clone(),
            scope: config.loudness_scope,
            pending: VecDeque::new(),
            buffered_samples: 0,
            latest_video_sample: 0,
            sample_rate: config.sample_rate,
            video_time_base: config.video_time_base,
            video_decoded: false,
            video_finished: false,
            last_audio_source: None,
        })
    }

    fn queue_audio(&mut self, audio: &frame::Audio, live: bool, hold_gain: bool) -> Result<()> {
        let mut frame = reference_audio_frame(audio)?;
        make_audio_frame_writable(&mut frame)?;

        if self.scope == LoudnessScope::Off {
            if let Some(volume) = self.state.volume.as_mut() {
                volume.process(&mut frame);
            }

            return self.output.encode_processed_audio(&frame);
        }

        self.buffered_samples += frame.samples();
        self.pending.push_back(PendingFrame::Audio {
            frame,
            live,
            hold_gain,
        });
        self.drain(false)
    }

    fn drain(&mut self, flush: bool) -> Result<()> {
        let preview = self
            .state
            .processor
            .as_ref()
            .map_or(0, LiveDynamicsProcessor::lookahead_samples);

        while let Some(front) = self.pending.front() {
            let (samples, end) = match front {
                PendingFrame::Video(video) => (
                    0,
                    video
                        .pts()
                        .unwrap_or_default()
                        .rescale(self.video_time_base, Rational(1, self.sample_rate as i32)),
                ),
                PendingFrame::Audio { frame, .. } => (
                    frame.samples(),
                    frame.pts().unwrap_or_default() + frame.samples() as i64,
                ),
            };
            // Missing audio must not retain video indefinitely. Bound queued
            // events as well, including malformed or absent source timestamps.
            if !flush
                && self.buffered_samples.saturating_sub(samples) < preview
                && self.latest_video_sample.saturating_sub(end) < preview as i64
                && self.pending.len() < 256
            {
                break;
            }

            match self.pending.pop_front() {
                Some(PendingFrame::Video(video)) => self.output.encode_video(&video)?,
                Some(PendingFrame::Audio {
                    mut frame,
                    live,
                    hold_gain,
                }) => {
                    self.buffered_samples -= frame.samples();

                    if self.scope == LoudnessScope::All || live {
                        let frames = self
                            .pending
                            .iter()
                            .filter_map(|pending| match pending {
                                PendingFrame::Audio { frame, live, .. } => Some((frame, *live)),
                                PendingFrame::Video(_) => None,
                            })
                            .take_while(|(_, live)| self.scope != LoudnessScope::Live || *live)
                            .map(|(frame, _)| frame);
                        let future = collect_audio_preview(
                            frames,
                            frame.pts().unwrap_or_default() + frame.samples() as i64,
                            preview,
                            0,
                        );

                        if let Some(processor) = self.state.processor.as_mut() {
                            if self.last_audio_source != Some(live) {
                                processor.begin_segment();
                            }

                            processor.update_config(self.control.settings().config);
                            benchmark::measure(Stage::AudioProcess, || {
                                processor.process_with_output_gain(
                                    &mut frame,
                                    &future,
                                    hold_gain,
                                    self.state.volume.as_mut(),
                                );
                            });
                            self.control.set_metrics(processor.metrics());
                        }
                    } else if let Some(volume) = self.state.volume.as_mut() {
                        volume.process(&mut frame);
                    }

                    self.last_audio_source = Some(live);
                    self.output.encode_processed_audio(&frame)?;
                }
                None => break,
            }
        }

        Ok(())
    }

    pub(crate) fn finish(&mut self) -> Result<()> {
        self.drain(true)?;

        if self.video_decoded {
            self.output.video_decoded()?;
        }

        if self.video_finished {
            self.output.video_finished()?;
        }

        Ok(())
    }
}

impl<O: FrameOutput> FrameOutput for ProgramAudioOutput<'_, O> {
    fn audio_frame_size(&self) -> usize {
        self.output.audio_frame_size()
    }

    fn handles_loudness(&self) -> bool {
        true
    }

    fn encode_audio(&mut self, frame: &frame::Audio) -> Result<()> {
        self.queue_audio(frame, false, false)
    }

    fn encode_live_audio(&mut self, frame: &frame::Audio) -> Result<()> {
        self.queue_audio(frame, true, false)
    }

    fn encode_audio_with_gain_hold(&mut self, frame: &frame::Audio, hold: bool) -> Result<()> {
        self.queue_audio(frame, false, hold)
    }

    fn encode_video(&mut self, video: &frame::Video) -> Result<()> {
        if self.scope == LoudnessScope::Off {
            return self.output.encode_video(video);
        }

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

    fn reset_after_skip(&mut self, video_pts: i64, audio_pts: i64) -> Result<bool> {
        if let Some(processor) = self.state.processor.as_mut() {
            processor.begin_segment();
        }

        self.last_audio_source = None;
        self.pending.clear();
        self.buffered_samples = 0;
        self.latest_video_sample =
            video_pts.rescale(self.video_time_base, Rational(1, self.sample_rate as i32));
        self.output.reset_after_skip(video_pts, audio_pts)
    }

    fn apply_logo_overlay(&mut self, frame: &mut frame::Video, logo: &LogoOverlay, opacity: f64) {
        self.output.apply_logo_overlay(frame, logo, opacity);
    }

    fn benchmarks_logo_overlay(&self) -> bool {
        self.output.benchmarks_logo_overlay()
    }

    fn set_video_end(&mut self, end: Option<i64>) -> Result<()> {
        self.output.set_video_end(end)
    }

    fn video_decoded(&mut self) -> Result<()> {
        self.video_decoded = true;
        Ok(())
    }

    fn video_finished(&mut self) -> Result<()> {
        self.video_finished = true;
        Ok(())
    }

    fn pad_audio(&mut self, samples: i64) -> Result<bool> {
        // Pending audio must reach the output before its clock can account for padding.
        self.drain(true)?;
        self.output.pad_audio(samples)
    }

    fn write_vtt_subtitles(&mut self, path: &str, start: i64, source: i64) -> Result<()> {
        self.output.write_vtt_subtitles(path, start, source)
    }

    fn clear_vtt_subtitles(&mut self) -> Result<()> {
        self.output.clear_vtt_subtitles()
    }

    fn advance_vtt_subtitles(&mut self, position: i64) -> Result<()> {
        self.output.advance_vtt_subtitles(position)
    }
}

#[cfg(test)]
mod tests {
    use ffmpeg_next::{
        ChannelLayout,
        format::sample::{Sample, Type},
    };

    use super::*;
    use crate::LiveLoudnessConfig;

    #[derive(Default)]
    struct Capture {
        audio: Vec<(i64, Vec<f32>)>,
        video: Vec<i64>,
        finished: bool,
    }

    impl FrameOutput for Capture {
        fn audio_frame_size(&self) -> usize {
            4800
        }

        fn encode_video(&mut self, video: &frame::Video) -> Result<()> {
            self.video.push(video.pts().unwrap());
            Ok(())
        }

        fn encode_audio(&mut self, audio: &frame::Audio) -> Result<()> {
            self.audio
                .push((audio.pts().unwrap(), audio.plane::<f32>(0).to_vec()));
            Ok(())
        }

        fn video_finished(&mut self) -> Result<()> {
            self.finished = true;
            Ok(())
        }
    }

    fn audio(pts: i64, amplitude: f32) -> frame::Audio {
        let mut audio = frame::Audio::new(Sample::F32(Type::Planar), 4800, ChannelLayout::STEREO);
        audio.set_rate(48000);
        audio.set_pts(Some(pts));

        for channel in 0..2 {
            for (index, sample) in audio.plane_mut::<f32>(channel).iter_mut().enumerate() {
                *sample = ((pts + index as i64) as f64 * std::f64::consts::TAU * 997.0 / 48000.0)
                    .sin() as f32
                    * amplitude;
            }
        }

        audio
    }

    fn config(scope: LoudnessScope) -> OutputConfig {
        OutputConfig::new(16, 16, 25, 48000)
            .with_loudness_scope(scope)
            .with_live_loudness_control(LiveLoudnessControl::new(
                true,
                LiveLoudnessConfig {
                    max_gain_db: 0.0,
                    ..LiveLoudnessConfig::default()
                },
            ))
    }

    #[test]
    fn short_burst_at_clip_and_live_transitions_is_compressed() {
        for separate_clip in [false, true] {
            let mut capture = Capture::default();
            let mut state = ProgramAudioState::default();
            let config = config(LoudnessScope::All);
            let mut output = ProgramAudioOutput::new(&mut capture, &mut state, &config).unwrap();
            output.encode_audio(&audio(0, 0.01)).unwrap();

            if separate_clip {
                output.finish().unwrap();
                drop(output);
                output = ProgramAudioOutput::new(&mut capture, &mut state, &config).unwrap();
            }

            let mut burst = audio(4800, 0.01);
            let loud = audio(4800, 0.8);

            for channel in 0..2 {
                burst.plane_mut::<f32>(channel)[..960]
                    .copy_from_slice(&loud.plane::<f32>(channel)[..960]);
            }

            if separate_clip {
                output.encode_audio(&burst).unwrap();
                output.encode_audio(&audio(9600, 0.01)).unwrap();
            } else {
                output.encode_live_audio(&burst).unwrap();
                output.encode_live_audio(&audio(9600, 0.01)).unwrap();
            }

            output.finish().unwrap();
            drop(output);
            let peak = capture.audio[1].1[..960]
                .iter()
                .copied()
                .map(f32::abs)
                .fold(0.0_f32, f32::max);
            assert!(peak < 0.3, "transition burst: {peak}");
            assert_eq!(
                capture
                    .audio
                    .iter()
                    .map(|(pts, _)| *pts)
                    .collect::<Vec<_>>(),
                [0, 4800, 9600]
            );
        }
    }

    #[test]
    fn volume_is_applied_once_and_final_peaks_stay_below_ceiling() {
        use crate::analysis::loudness::LoudnessAnalyzer;

        for scope in [LoudnessScope::Off, LoudnessScope::Live, LoudnessScope::All] {
            let mut capture = Capture::default();
            let mut state = ProgramAudioState::default();
            let config = config(scope);
            config.audio_effects.set_volume(0.5).unwrap();
            config.live_loudness_control.update(
                true,
                LiveLoudnessConfig {
                    compressor_ratio: 1.0,
                    gain_up_db_per_second: 0.0,
                    gain_down_db_per_second: 0.0,
                    ..LiveLoudnessConfig::default()
                },
            );
            let mut output = ProgramAudioOutput::new(&mut capture, &mut state, &config).unwrap();
            let source = audio(0, 0.1);
            output.encode_audio(&source).unwrap();
            output.finish().unwrap();
            drop(output);

            for (&actual, &sample) in capture.audio[0].1.iter().zip(source.plane::<f32>(0)) {
                assert!((actual - sample * 0.5).abs() < 1e-6);
            }

            if scope == LoudnessScope::Off {
                continue;
            }

            // A volume increase during playback must remain peak limited during its ramp.
            config.audio_effects.set_volume(1.5).unwrap();
            let mut output = ProgramAudioOutput::new(&mut capture, &mut state, &config).unwrap();

            for block in 1..11 {
                output.encode_live_audio(&audio(block * 4800, 1.0)).unwrap();
            }

            output.finish().unwrap();
            drop(output);
            let mut analyzer = LoudnessAnalyzer::new(48000).unwrap();
            let mut maximum_peak = -100.0_f64;

            for (_, left) in capture.audio.iter().skip(1) {
                assert!(
                    left.iter()
                        .all(|sample| sample.abs() <= 10.0_f32.powf(-1.0 / 20.0))
                );
                let metrics = analyzer.process_samples(left, left);
                if let Some(peak) = metrics.true_peak_dbtp {
                    maximum_peak = maximum_peak.max(peak);
                }
            }

            assert!(maximum_peak <= -1.0, "final true peak: {maximum_peak} dBTP");
            assert!(
                maximum_peak > -2.0,
                "unexpected extra attenuation: {maximum_peak}"
            );
        }
    }

    #[test]
    fn scope_selects_file_and_live_without_double_processing_or_changing_pts() {
        for scope in [LoudnessScope::Off, LoudnessScope::Live, LoudnessScope::All] {
            let mut capture = Capture::default();
            let mut state = ProgramAudioState::default();
            let config = config(scope);
            let mut output = ProgramAudioOutput::new(&mut capture, &mut state, &config).unwrap();
            let file = audio(0, 0.8);
            let live = audio(4800, 0.8);
            output.encode_audio(&file).unwrap();
            output.encode_live_audio(&live).unwrap();
            output.finish().unwrap();
            drop(output);
            assert_eq!(capture.audio.len(), 2);
            assert_eq!(capture.audio[0].0, 0);
            assert_eq!(capture.audio[1].0, 4800);

            for (index, original) in [file, live].iter().enumerate() {
                let processed =
                    scope == LoudnessScope::All || (scope == LoudnessScope::Live && index == 1);

                if processed {
                    let peak = capture.audio[index]
                        .1
                        .iter()
                        .copied()
                        .map(f32::abs)
                        .fold(0.0, f32::max);
                    assert!(peak < 0.3, "{scope:?}: {peak}");
                } else {
                    assert_eq!(capture.audio[index].1, original.plane::<f32>(0));
                }
            }
        }
    }

    #[test]
    fn processor_history_survives_clip_boundaries_and_updates() {
        let mut capture = Capture::default();
        let mut state = ProgramAudioState::default();
        let config = config(LoudnessScope::All);
        let settings = LiveLoudnessConfig {
            gain_down_db_per_second: 10.0,
            ..LiveLoudnessConfig::default()
        };
        config.live_loudness_control.update(true, settings);

        for clip in 0..2 {
            let mut output = ProgramAudioOutput::new(&mut capture, &mut state, &config).unwrap();

            for index in 0..30 {
                output
                    .encode_audio(&audio((clip * 30 + index) * 4800, 0.8))
                    .unwrap();
            }

            output.finish().unwrap();
            drop(output);
            assert!(config.live_loudness_control.metrics().rider_gain_db < -1.0);
        }

        let before = state.processor.as_ref().unwrap().metrics();
        config.live_loudness_control.update(
            true,
            LiveLoudnessConfig {
                compressor_ratio: 2.0,
                ..settings
            },
        );
        let mut output = ProgramAudioOutput::new(&mut capture, &mut state, &config).unwrap();
        output.encode_audio(&audio(60 * 4800, 0.8)).unwrap();
        output.finish().unwrap();
        assert!(config.live_loudness_control.metrics().rider_gain_db <= before.rider_gain_db + 0.1);
        assert_eq!(capture.audio.len(), 61);
    }

    #[test]
    fn decoded_playlist_clips_keep_the_same_audio_video_timeline_with_processing() {
        ffmpeg_next::init().unwrap();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests_assets/storage/media_mix/av_sync.mp4");
        let mut captures = Vec::new();

        for scope in [LoudnessScope::Off, LoudnessScope::All] {
            let config = config(scope);
            config.live_loudness_control.update(
                true,
                LiveLoudnessConfig {
                    compressor_threshold_dbfs: -45.0,
                    max_gain_db: 0.0,
                    ..LiveLoudnessConfig::default()
                },
            );
            let mut state = ProgramAudioState::default();
            let mut capture = Capture::default();
            let mut timeline = crate::playout::Timeline::new();

            for _ in 0..2 {
                let result = crate::play_program_clip(
                    &mut capture,
                    &mut state,
                    &mut None,
                    path.to_str().unwrap(),
                    &config,
                    &mut timeline,
                    1.0,
                    &crate::PlaybackControl::default(),
                    crate::PlayOptions {
                        seek_seconds: None,
                        duration_seconds: Some(1.0),
                        external_audio_path: None,
                        subtitles_media_path: None,
                        logo_fade: crate::LogoFade::default(),
                    },
                )
                .unwrap();
                assert_eq!(result, crate::ClipResult::Played);
            }

            captures.push(capture);
        }

        assert_eq!(captures[0].video, captures[1].video);
        let timing = |capture: &Capture| {
            capture
                .audio
                .iter()
                .map(|(pts, samples)| (*pts, samples.len()))
                .collect::<Vec<_>>()
        };
        assert_eq!(timing(&captures[0]), timing(&captures[1]));
        assert!(!captures[1].video.is_empty());
        assert!(
            captures[1]
                .audio
                .iter()
                .map(|(_, samples)| samples.len())
                .sum::<usize>()
                >= 96000
        );
        assert!(
            captures[0]
                .audio
                .iter()
                .zip(&captures[1].audio)
                .any(|(before, after)| before.1 != after.1)
        );
    }

    #[test]
    fn missing_audio_releases_video_with_bounded_preview_and_finishes() {
        let mut capture = Capture::default();
        let mut state = ProgramAudioState::default();
        let config = config(LoudnessScope::All);
        let mut output = ProgramAudioOutput::new(&mut capture, &mut state, &config).unwrap();

        for pts in 0..100 {
            let mut video = frame::Video::new(ffmpeg_next::format::Pixel::YUV420P, 16, 16);
            video.set_pts(Some(pts));
            output.encode_video(&video).unwrap();
            assert!(output.pending.len() <= 3);
        }

        output.video_finished().unwrap();
        assert!(!output.output.finished);
        output.finish().unwrap();
        drop(output);
        assert_eq!(capture.video, (0..100).collect::<Vec<_>>());
        assert!(capture.finished);
    }
}
