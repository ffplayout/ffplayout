//! Live-only loudness processing for decoded planar `f32` audio.
//!
//! The gain rider intentionally follows short-term loudness instead of the
//! programme-integrated value.  The latter is a reporting metric and must not
//! steer a live programme because it never forgets earlier material.

use std::sync::{Arc, PoisonError, RwLock};

use ffmpeg_next::frame;

use crate::analysis::loudness::LoudnessAnalyzer;

use super::lookahead::LookaheadLimiter;

const TARGET_LUFS: f64 = -23.0;
const DEAD_BAND_LU: f64 = 1.0;
const MAX_GAIN_DB: f64 = 8.0;
const MAX_ATTENUATION_DB: f64 = -12.0;
const SILENCE_GATE_LUFS: f64 = -60.0;
const GAIN_UP_DB_PER_SECOND: f64 = 0.5;
const GAIN_DOWN_DB_PER_SECOND: f64 = 2.0;
const TRUE_PEAK_CEILING_DBTP: f64 = -1.0;
const FAST_ATTENUATION_ENTER_LU: f64 = 6.0;
const FAST_ATTENUATION_EXIT_LU: f64 = 3.0;
const FAST_ATTENUATION_RATE_MULTIPLIER: f64 = 6.0;
const GAIN_LIMIT_RAMP_SECONDS: f64 = 0.02;
const LIMITER_RELEASE_SECONDS: f64 = 0.05;
const GAIN_BATCH_SAMPLES: usize = 1_024;

/// Parameters for the live gain rider and the final safety limiter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiveLoudnessConfig {
    pub compressor_ratio: f64,
    pub compressor_attack_ms: f64,
    pub compressor_hold_ms: f64,
    pub compressor_release_ms: f64,
    /// Release time while compressor attenuation exceeds 12 dB.
    pub compressor_strong_release_ms: f64,
    pub compressor_knee_db: f64,
    pub pause_hold_ms: f64,
    /// Delay from the start of quiet input before positive gain returns to zero.
    pub pause_return_delay_ms: f64,
    pub output_max_correction_db: f64,
    pub output_gain_up_db_per_second: f64,
    pub output_gain_down_db_per_second: f64,

    pub compressor_threshold_dbfs: f64,
    pub pause_threshold_dbfs: f64,
    pub target_lufs: f64,
    pub dead_band_lu: f64,
    pub max_gain_db: f64,
    pub max_attenuation_db: f64,
    pub gain_up_db_per_second: f64,
    /// Normal attenuation speed. Strong Short-Term overshoots use up to six
    /// times this rate until Momentary loudness is back within 3 LU of target.
    pub gain_down_db_per_second: f64,
    pub silence_gate_lufs: f64,
    /// Ceiling for the oversampled live lookahead limiter.
    pub true_peak_ceiling_dbtp: f64,
}

impl Default for LiveLoudnessConfig {
    fn default() -> Self {
        Self {
            compressor_ratio: 3.0,
            compressor_attack_ms: 5.0,
            compressor_hold_ms: 100.0,
            compressor_release_ms: 1200.0,
            compressor_strong_release_ms: 500.0,
            compressor_knee_db: 6.0,
            pause_hold_ms: 300.0,
            pause_return_delay_ms: 2000.0,
            output_max_correction_db: 3.0,
            output_gain_up_db_per_second: 0.1,
            output_gain_down_db_per_second: 0.25,

            compressor_threshold_dbfs: -26.0,
            pause_threshold_dbfs: -55.0,
            target_lufs: TARGET_LUFS,
            dead_band_lu: DEAD_BAND_LU,
            max_gain_db: MAX_GAIN_DB,
            max_attenuation_db: MAX_ATTENUATION_DB,
            gain_up_db_per_second: GAIN_UP_DB_PER_SECOND,
            gain_down_db_per_second: GAIN_DOWN_DB_PER_SECOND,
            silence_gate_lufs: SILENCE_GATE_LUFS,
            true_peak_ceiling_dbtp: TRUE_PEAK_CEILING_DBTP,
        }
    }
}

/// Shared runtime settings for the live processor. Updates take effect on the
/// next audio frame and do not require recreating the playout.
#[derive(Debug, Clone)]
pub struct LiveLoudnessControl(Arc<RwLock<LiveLoudnessSettings>>);

#[derive(Debug, Clone, Copy)]
pub struct LiveLoudnessSettings {
    pub enabled: bool,
    pub config: LiveLoudnessConfig,
    pub metrics: LiveLoudnessMetrics,
}

impl LiveLoudnessControl {
    pub fn new(enabled: bool, config: LiveLoudnessConfig) -> Self {
        Self(Arc::new(RwLock::new(LiveLoudnessSettings {
            enabled,
            config,
            metrics: LiveLoudnessMetrics::default(),
        })))
    }

    pub fn settings(&self) -> LiveLoudnessSettings {
        *self.0.read().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn update(&self, enabled: bool, config: LiveLoudnessConfig) {
        let mut settings = self.0.write().unwrap_or_else(PoisonError::into_inner);
        if !enabled || !settings.enabled {
            settings.metrics = LiveLoudnessMetrics::default();
        }

        settings.enabled = enabled;
        settings.config = config;
    }

    pub fn metrics(&self) -> LiveLoudnessMetrics {
        self.0
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .metrics
    }

    pub(crate) fn set_metrics(&self, metrics: LiveLoudnessMetrics) {
        self.0
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .metrics = metrics;
    }
}

/// Metrics from the source signal and the gain stage, suitable for API/UI
/// publishing without exposing the analyzer itself.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LiveLoudnessMetrics {
    pub momentary_lufs: Option<f64>,
    pub short_term_lufs: Option<f64>,
    pub integrated_lufs: Option<f64>,
    pub true_peak_dbtp: Option<f64>,
    pub rider_gain_db: f64,
    pub limiter_gain_reduction_db: f64,
}

/// Source measurements associated with positions in one buffered audio frame.
/// Gain and limiter state remain on the playback timeline, not the decode timeline.
pub struct BufferedLoudnessAnalysis {
    measurements: Vec<(usize, LiveLoudnessMetrics)>,
}

/// Measurement window used by the gain rider.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LiveLoudnessMeasurement {
    /// The EBU R128 three-second short-term measurement.
    #[default]
    ShortTerm,
    /// The EBU R128 400 ms momentary measurement.
    Momentary,
}

/// Stateful EBU R128 analyzer, slow gain rider and ceiling limiter.
///
/// `ebur128-stream` performs the 4x true-peak measurement on the source. The
/// live path uses a stereo-linked, 4x oversampled lookahead limiter. Direct
/// callers of `process` retain the immediate sample limiter without buffering.
pub struct LiveLoudnessProcessor {
    analyzer: LoudnessAnalyzer,
    config: LiveLoudnessConfig,
    sample_rate: u32,
    measurement: LiveLoudnessMeasurement,
    rider_gain_db: f64,
    target_gain_db: f64,
    fast_attenuation: bool,
    gain_limit_ramp_remaining: usize,
    samples_until_analysis: usize,
    lookahead: LookaheadLimiter,
    limiter_gain: f64,
    limiter_release: f64,
    metrics: LiveLoudnessMetrics,
    analysis_control_metrics: LiveLoudnessMetrics,
}

impl LiveLoudnessProcessor {
    pub fn new(
        sample_rate: u32,
        config: LiveLoudnessConfig,
    ) -> Result<Self, ebur128_stream::Error> {
        let analyzer = LoudnessAnalyzer::new(sample_rate)?;

        Ok(Self {
            analyzer,
            config,
            sample_rate,
            measurement: LiveLoudnessMeasurement::default(),
            rider_gain_db: 0.0,
            target_gain_db: 0.0,
            fast_attenuation: false,
            gain_limit_ramp_remaining: 0,
            samples_until_analysis: sample_rate as usize / 10,
            lookahead: LookaheadLimiter::new(sample_rate),
            limiter_gain: 1.0,
            limiter_release: 1.0
                - (-1.0 / (LIMITER_RELEASE_SECONDS * f64::from(sample_rate))).exp(),
            metrics: LiveLoudnessMetrics::default(),
            analysis_control_metrics: LiveLoudnessMetrics::default(),
        })
    }

    pub fn metrics(&self) -> LiveLoudnessMetrics {
        self.metrics
    }

    pub fn config(&self) -> LiveLoudnessConfig {
        self.config
    }

    /// Preserves measurement history and applied gain. Newly tightened gain
    /// bounds are reached over 20 ms, while the sample ceiling applies at once.
    pub fn update_config(&mut self, config: LiveLoudnessConfig) {
        self.config = config;
        self.target_gain_db = self
            .target_gain_db
            .clamp(config.max_attenuation_db, config.max_gain_db);

        if self.rider_gain_db < config.max_attenuation_db || self.rider_gain_db > config.max_gain_db
        {
            self.gain_limit_ramp_remaining =
                (GAIN_LIMIT_RAMP_SECONDS * f64::from(self.sample_rate)).round() as usize;
        } else {
            self.gain_limit_ramp_remaining = 0;
        }

        self.update_rider_target();
    }

    pub fn set_measurement(&mut self, measurement: LiveLoudnessMeasurement) {
        self.measurement = measurement;
    }

    /// Processes one normalized stereo frame in place. Non-finite samples are
    /// sanitized before analysis so malformed live input cannot poison the
    /// analyzer or encoder.
    pub fn process(&mut self, frame: &mut frame::Audio) {
        self.analyze_frame(frame, true, true, None);
    }

    /// Applies the live limiter using buffered future source samples, keeping
    /// this frame's sample count and PTS intact. Preview never enters analysis.
    pub fn process_with_lookahead(&mut self, frame: &mut frame::Audio, future: &[[f32; 2]]) {
        if frame.planes() != 2 || frame.samples() == 0 {
            return;
        }

        self.analyze_frame(frame, true, false, None);
        self.limit_with_lookahead(frame, future);
    }

    /// Number of future stereo samples required by the peak limiter.
    pub fn peak_lookahead_samples(&self) -> usize {
        super::lookahead_samples(self.sample_rate) + super::TRUE_PEAK_FUTURE_SAMPLES
    }

    /// Apply the current rider and lookahead limiter to previously analyzed audio.
    /// This supports offline experiments with a longer loudness preview window.
    pub fn apply_gain_with_lookahead(&mut self, frame: &mut frame::Audio, future: &[[f32; 2]]) {
        if frame.planes() != 2 || frame.samples() == 0 {
            return;
        }

        sanitize_samples(frame);
        self.metrics.limiter_gain_reduction_db = 0.0;
        self.apply_gain_and_ceiling(frame, 0, frame.samples(), false);
        self.limit_with_lookahead(frame, future);
    }

    fn limit_with_lookahead(&mut self, frame: &mut frame::Audio, future: &[[f32; 2]]) {
        let mut preview_gain_db = self.rider_gain_db
            + self.config.gain_up_db_per_second * super::LIVE_LATENCY.as_secs_f64();

        if self.gain_limit_ramp_remaining > 0 {
            preview_gain_db = preview_gain_db.max(
                self.rider_gain_db
                    .clamp(self.config.max_attenuation_db, self.config.max_gain_db),
            );
        }

        self.metrics.limiter_gain_reduction_db = self.lookahead.process(
            frame,
            future,
            db_to_gain(preview_gain_db),
            self.config.true_peak_ceiling_dbtp,
        );
    }

    /// Updates source metrics and the rider target without applying gain.
    /// This is used by offline/lookahead callers that apply the resulting gain
    /// to an earlier buffered frame.
    pub fn analyze(&mut self, frame: &mut frame::Audio) {
        self.analyze_frame(frame, false, false, None);
    }

    /// Analyze ahead of playback, retaining the exact 100 ms control boundaries.
    pub fn analyze_buffered(&mut self, frame: &mut frame::Audio) -> BufferedLoudnessAnalysis {
        let mut measurements = vec![(0, self.analysis_control_metrics)];
        self.analyze_frame(frame, false, false, Some(&mut measurements));

        BufferedLoudnessAnalysis { measurements }
    }

    /// Replay source measurements at their original sample positions, then limit peaks.
    pub fn apply_buffered_gain(
        &mut self,
        frame: &mut frame::Audio,
        analysis: &BufferedLoudnessAnalysis,
        future: &[[f32; 2]],
    ) {
        if frame.planes() != 2 || frame.samples() == 0 {
            return;
        }

        sanitize_samples(frame);
        self.metrics.limiter_gain_reduction_db = 0.0;

        for (index, &(start, measurement)) in analysis.measurements.iter().enumerate() {
            self.metrics.momentary_lufs = measurement.momentary_lufs;
            self.metrics.short_term_lufs = measurement.short_term_lufs;
            self.metrics.integrated_lufs = measurement.integrated_lufs;
            self.metrics.true_peak_dbtp = measurement.true_peak_dbtp;
            if index > 0 {
                self.update_rider_target();
            }

            let end = analysis
                .measurements
                .get(index + 1)
                .map_or(frame.samples(), |&(offset, _)| offset);
            self.apply_gain_and_ceiling(frame, start, end, false);
        }

        self.limit_with_lookahead(frame, future);
    }

    fn analyze_frame(
        &mut self,
        frame: &mut frame::Audio,
        apply_gain: bool,
        sample_ceiling: bool,
        mut measurements: Option<&mut Vec<(usize, LiveLoudnessMetrics)>>,
    ) {
        if frame.planes() != 2 || frame.samples() == 0 {
            return;
        }

        sanitize_samples(frame);

        if apply_gain {
            self.metrics.limiter_gain_reduction_db = 0.0;
        }

        let mut offset = 0;

        while offset < frame.samples() {
            let samples = self.samples_until_analysis.min(frame.samples() - offset);
            let end = offset + samples;
            let metrics = self.analyzer.process_samples(
                &frame.plane::<f32>(0)[offset..end],
                &frame.plane::<f32>(1)[offset..end],
            );
            self.metrics.momentary_lufs = metrics.momentary_lufs;
            self.metrics.short_term_lufs = metrics.short_term_lufs;
            self.metrics.integrated_lufs = metrics.integrated_lufs;
            self.metrics.true_peak_dbtp = metrics.true_peak_dbtp;

            if apply_gain {
                self.apply_gain_and_ceiling(frame, offset, end, sample_ceiling);
            }

            self.samples_until_analysis -= samples;
            offset = end;

            if self.samples_until_analysis == 0 {
                self.samples_until_analysis = self.sample_rate as usize / 10;
                // Apply a completed measurement to subsequent samples only,
                // so control timing is independent of input packet boundaries.
                self.analysis_control_metrics = self.metrics;

                if let Some(measurements) = measurements.as_mut() {
                    measurements.push((offset, self.analysis_control_metrics));
                } else {
                    self.update_rider_target();
                }
            }
        }
    }

    /// Applies the current rider gain and safety ceiling to a frame previously
    /// passed to [`Self::analyze`].
    pub fn apply_gain(&mut self, frame: &mut frame::Audio) {
        if frame.planes() != 2 || frame.samples() == 0 {
            return;
        }

        sanitize_samples(frame);
        self.metrics.limiter_gain_reduction_db = 0.0;
        self.apply_gain_and_ceiling(frame, 0, frame.samples(), true);
    }

    fn update_rider_target(&mut self) {
        let loudness = match self.measurement {
            LiveLoudnessMeasurement::ShortTerm => self.metrics.short_term_lufs,
            LiveLoudnessMeasurement::Momentary => self.metrics.momentary_lufs,
        };

        if self
            .metrics
            .momentary_lufs
            .is_none_or(|level| level < self.config.silence_gate_lufs)
        {
            self.target_gain_db = self
                .rider_gain_db
                .clamp(self.config.max_attenuation_db, self.config.max_gain_db);
            self.fast_attenuation = false;

            return;
        }

        let mut desired = loudness
            .filter(|level| *level >= self.config.silence_gate_lufs)
            .map_or(self.rider_gain_db, |level| self.desired_gain(level));

        if self.measurement == LiveLoudnessMeasurement::ShortTerm
            && let Some(momentary) = self.metrics.momentary_lufs
        {
            let excess = momentary + self.rider_gain_db - self.config.target_lufs;
            let threshold = if self.fast_attenuation {
                FAST_ATTENUATION_EXIT_LU
            } else {
                FAST_ATTENUATION_ENTER_LU
            };
            self.fast_attenuation = excess > threshold;

            if self.fast_attenuation {
                desired = desired.min(self.desired_gain(momentary));
            }
        } else {
            self.fast_attenuation = false;
        }

        self.target_gain_db =
            desired.clamp(self.config.max_attenuation_db, self.config.max_gain_db);
    }

    fn desired_gain(&self, loudness: f64) -> f64 {
        let error = self.config.target_lufs - loudness;

        if error.abs() <= self.config.dead_band_lu {
            0.0
        } else {
            error
        }
    }

    fn next_rider_gain(&mut self) -> f64 {
        if self.gain_limit_ramp_remaining > 0 {
            let bound = self
                .rider_gain_db
                .clamp(self.config.max_attenuation_db, self.config.max_gain_db);
            self.rider_gain_db +=
                (bound - self.rider_gain_db) / self.gain_limit_ramp_remaining as f64;
            self.gain_limit_ramp_remaining -= 1;
        } else {
            let mut rate = if self.target_gain_db < self.rider_gain_db {
                self.config.gain_down_db_per_second
            } else {
                self.config.gain_up_db_per_second
            };

            if self.fast_attenuation && self.target_gain_db < self.rider_gain_db {
                rate *= FAST_ATTENUATION_RATE_MULTIPLIER;
            }

            let step = rate / f64::from(self.sample_rate);
            self.rider_gain_db += (self.target_gain_db - self.rider_gain_db).clamp(-step, step);
        }

        db_to_gain(self.rider_gain_db)
    }

    fn apply_gain_and_ceiling(
        &mut self,
        frame: &mut frame::Audio,
        start: usize,
        end: usize,
        sample_ceiling: bool,
    ) {
        let ceiling = f64::from(db_to_gain(self.config.true_peak_ceiling_dbtp) as f32);
        let mut offset = start;
        let mut minimum_limiter_gain = db_to_gain(-self.metrics.limiter_gain_reduction_db);

        while offset < end {
            let batch_end = (offset + GAIN_BATCH_SAMPLES).min(end);
            let mut gains = [0.0; GAIN_BATCH_SAMPLES];
            let left = &frame.plane::<f32>(0)[offset..batch_end];
            let right = &frame.plane::<f32>(1)[offset..batch_end];

            for ((left, right), gain) in left.iter().zip(right).zip(&mut gains) {
                let rider = self.next_rider_gain();
                let peak = f64::from(left.abs().max(right.abs())) * rider;
                let required = if peak > ceiling { ceiling / peak } else { 1.0 };

                if !sample_ceiling {
                    *gain = rider;

                    continue;
                }

                if required < self.limiter_gain {
                    self.limiter_gain = required;
                } else {
                    self.limiter_gain += (required - self.limiter_gain) * self.limiter_release;
                }

                *gain = rider * self.limiter_gain;
                minimum_limiter_gain = minimum_limiter_gain.min(self.limiter_gain);
            }

            for channel in 0..2 {
                for (sample, gain) in frame.plane_mut::<f32>(channel)[offset..batch_end]
                    .iter_mut()
                    .zip(gains)
                {
                    *sample = (f64::from(*sample) * gain)
                        .clamp(-f64::from(f32::MAX), f64::from(f32::MAX))
                        as f32;
                }
            }

            offset = batch_end;
        }

        self.metrics.limiter_gain_reduction_db = -20.0 * minimum_limiter_gain.log10();
        self.metrics.rider_gain_db = self.rider_gain_db;
    }
}

fn sanitize_samples(frame: &mut frame::Audio) {
    for plane in 0..2 {
        for sample in frame.plane_mut::<f32>(plane) {
            if !sample.is_finite() {
                *sample = 0.0;
            }
        }
    }
}

fn db_to_gain(db: f64) -> f64 {
    10.0_f64.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use ffmpeg_next::{
        format::sample::{Sample, Type},
        util::channel_layout::ChannelLayout,
    };

    use super::*;

    const SAMPLE_RATE: u32 = 48_000;

    fn stereo_frame(left: &[f32], right: &[f32]) -> frame::Audio {
        let mut frame =
            frame::Audio::new(Sample::F32(Type::Planar), left.len(), ChannelLayout::STEREO);
        frame.set_rate(SAMPLE_RATE);
        frame.plane_mut::<f32>(0).copy_from_slice(left);
        frame.plane_mut::<f32>(1).copy_from_slice(right);

        frame
    }

    fn tone(samples: usize, amplitude: f32, offset: usize) -> Vec<f32> {
        (offset..offset + samples)
            .map(|sample| {
                (std::f64::consts::TAU * 997.0 * sample as f64 / f64::from(SAMPLE_RATE)).sin()
                    as f32
                    * amplitude
            })
            .collect()
    }

    #[test]
    fn config_updates_preserve_history_gain_and_shared_metrics() {
        let config = LiveLoudnessConfig::default();
        let control = LiveLoudnessControl::new(true, config);
        let mut processor = LiveLoudnessProcessor::new(SAMPLE_RATE, config).unwrap();
        let samples = tone(SAMPLE_RATE as usize * 4, 0.01, 0);
        processor.process(&mut stereo_frame(&samples, &samples));
        let previous = processor.metrics();
        assert!(previous.rider_gain_db > 0.0);
        assert!(previous.short_term_lufs.is_some());
        control.set_metrics(previous);
        let updated = LiveLoudnessConfig {
            target_lufs: -20.0,
            ..config
        };
        control.update(true, updated);
        processor.update_config(updated);

        assert_eq!(control.metrics(), previous);
        assert_eq!(processor.metrics(), previous);
        let samples = tone(1_024, 0.01, SAMPLE_RATE as usize * 4);
        processor.process(&mut stereo_frame(&samples, &samples));
        assert!(processor.metrics().short_term_lufs.is_some());
        assert!(processor.metrics().rider_gain_db >= previous.rider_gain_db);
        control.update(false, updated);
        assert_eq!(control.metrics(), LiveLoudnessMetrics::default());
    }

    #[test]
    fn applied_gain_ramps_sample_by_sample() {
        let config = LiveLoudnessConfig {
            gain_up_db_per_second: 6.0,
            ..LiveLoudnessConfig::default()
        };
        let mut processor = LiveLoudnessProcessor::new(SAMPLE_RATE, config).unwrap();
        processor.target_gain_db = 6.0;
        let samples = vec![0.01; 480];
        let mut frame = stereo_frame(&samples, &samples);
        processor.apply_gain(&mut frame);
        let output = frame.plane::<f32>(0);

        assert!(output.windows(2).all(|pair| pair[1] > pair[0]));
        assert!((f64::from(output[0]) / 0.01 - db_to_gain(6.0 / 48_000.0)).abs() < 1e-7);
        assert!((processor.metrics().rider_gain_db - 0.06).abs() < 1e-10);
        assert_eq!(frame.plane::<f32>(0), frame.plane::<f32>(1));
    }

    #[test]
    fn tighter_gain_bounds_ramp_without_resetting_to_zero() {
        for (initial, bounds) in [(6.0, (-12.0, 2.0)), (-6.0, (-2.0, 8.0))] {
            let config = LiveLoudnessConfig::default();
            let mut processor = LiveLoudnessProcessor::new(SAMPLE_RATE, config).unwrap();
            processor.rider_gain_db = initial;
            processor.target_gain_db = initial;
            processor.update_config(LiveLoudnessConfig {
                max_attenuation_db: bounds.0,
                max_gain_db: bounds.1,
                ..config
            });
            assert_eq!(processor.rider_gain_db, initial);
            let samples = vec![0.01; 959];
            processor.apply_gain(&mut stereo_frame(&samples, &samples));
            let bound = initial.clamp(bounds.0, bounds.1);
            assert!((processor.rider_gain_db - bound).abs() > 0.0);
            assert!((processor.rider_gain_db - bound).abs() < 0.01);
            processor.apply_gain(&mut stereo_frame(&[0.01], &[0.01]));
            assert_eq!(processor.rider_gain_db, bound);
        }
    }

    #[test]
    fn limiter_links_stereo_and_recovers_smoothly_after_a_peak() {
        let mut processor =
            LiveLoudnessProcessor::new(SAMPLE_RATE, LiveLoudnessConfig::default()).unwrap();
        let left = [0.1, 2.0, 0.1, 0.1];
        let right = left.map(|sample| sample * -0.25);
        let mut frame = stereo_frame(&left, &right);
        processor.apply_gain(&mut frame);
        let ceiling = db_to_gain(-1.0) as f32;

        for (left, right) in frame.plane::<f32>(0).iter().zip(frame.plane::<f32>(1)) {
            assert!(left.abs() <= ceiling);
            assert!((*right - *left * -0.25).abs() < 1e-7);
        }

        let output = frame.plane::<f32>(0);
        assert_eq!(output[0], 0.1);
        assert!(output[2] < 0.05);
        assert!(output[3] > output[2]);
        assert!(processor.metrics().limiter_gain_reduction_db > 6.0);
        let samples = vec![0.1; SAMPLE_RATE as usize / 2];
        let mut recovered = stereo_frame(&samples, &samples);
        processor.apply_gain(&mut recovered);
        assert!((recovered.plane::<f32>(0).last().unwrap() - 0.1).abs() < 1e-5);
    }

    #[test]
    fn lower_sample_ceiling_applies_on_the_next_sample() {
        let config = LiveLoudnessConfig::default();
        let mut processor = LiveLoudnessProcessor::new(SAMPLE_RATE, config).unwrap();
        processor.apply_gain(&mut stereo_frame(&[1.0], &[0.5]));
        processor.update_config(LiveLoudnessConfig {
            true_peak_ceiling_dbtp: -12.0,
            ..config
        });
        let mut frame = stereo_frame(&[1.0], &[0.5]);
        processor.apply_gain(&mut frame);

        assert!(frame.plane::<f32>(0)[0] <= db_to_gain(-12.0) as f32);
        assert_eq!(frame.plane::<f32>(1)[0], frame.plane::<f32>(0)[0] * 0.5);
    }

    #[test]
    fn strong_level_jump_uses_fast_attenuation_before_short_term_catches_up() {
        let mut processor =
            LiveLoudnessProcessor::new(SAMPLE_RATE, LiveLoudnessConfig::default()).unwrap();
        let quiet = tone(SAMPLE_RATE as usize * 4, 0.01, 0);
        processor.process(&mut stereo_frame(&quiet, &quiet));
        let before = processor.metrics().rider_gain_db;
        let loud = tone(SAMPLE_RATE as usize / 2, 0.8, quiet.len());
        processor.process(&mut stereo_frame(&loud, &loud));

        assert!(before - processor.metrics().rider_gain_db > 2.0);
        assert!(processor.fast_attenuation);
        assert!(processor.metrics().rider_gain_db >= processor.config.max_attenuation_db);
    }

    #[test]
    fn emergency_attenuation_also_works_during_short_term_warmup() {
        let mut processor =
            LiveLoudnessProcessor::new(SAMPLE_RATE, LiveLoudnessConfig::default()).unwrap();
        let loud = tone(SAMPLE_RATE as usize, 0.8, 0);
        processor.process(&mut stereo_frame(&loud, &loud));

        assert_eq!(processor.metrics().short_term_lufs, None);
        assert!(processor.metrics().rider_gain_db < -2.0);
    }

    #[test]
    fn gain_does_not_keep_rising_under_the_silence_gate() {
        let mut processor =
            LiveLoudnessProcessor::new(SAMPLE_RATE, LiveLoudnessConfig::default()).unwrap();
        processor.rider_gain_db = 4.0;
        processor.target_gain_db = 8.0;
        processor.metrics.momentary_lufs = Some(-80.0);
        processor.metrics.short_term_lufs = Some(-40.0);
        processor.update_rider_target();
        let samples = vec![0.0; 1_024];
        let mut frame = stereo_frame(&samples, &samples);
        processor.apply_gain(&mut frame);

        assert_eq!(processor.metrics().rider_gain_db, 4.0);
        assert!(frame.plane::<f32>(0).iter().all(|sample| *sample == 0.0));
        processor.metrics.momentary_lufs = None;
        processor.target_gain_db = 8.0;
        processor.update_rider_target();
        processor.apply_gain(&mut frame);
        assert_eq!(processor.metrics().rider_gain_db, 4.0);
    }

    #[test]
    fn alternating_bursts_music_and_pauses_keep_gain_and_peaks_bounded() {
        let config = LiveLoudnessConfig::default();
        let mut processor = LiveLoudnessProcessor::new(SAMPLE_RATE, config).unwrap();
        let ceiling = db_to_gain(config.true_peak_ceiling_dbtp) as f32;

        for chunk in 0..120 {
            let offset = chunk * 4_800;
            let mut samples = tone(4_800, 0.02, offset);

            match chunk / 30 {
                0 | 3 => {
                    // Quiet, interrupted tonal bursts exercise programme gaps.
                    if chunk % 5 < 2 {
                        samples.fill(0.0);
                    }
                }
                1 => {
                    for (index, sample) in samples.iter_mut().enumerate() {
                        let time = (offset + index) as f64 / f64::from(SAMPLE_RATE);
                        *sample = ((std::f64::consts::TAU * 233.0 * time).sin() * 0.6
                            + (std::f64::consts::TAU * 3_191.0 * time).sin() * 0.6)
                            as f32;
                    }
                }
                _ => samples.fill(0.0),
            }

            let right: Vec<_> = samples.iter().map(|sample| sample * -0.5).collect();
            let mut frame = stereo_frame(&samples, &right);
            frame.set_pts(Some(offset as i64));
            processor.process(&mut frame);
            assert_eq!(frame.pts(), Some(offset as i64));
            assert_eq!(frame.samples(), samples.len());
            assert!(processor.metrics().rider_gain_db >= config.max_attenuation_db);
            assert!(processor.metrics().rider_gain_db <= config.max_gain_db);

            for (left, right) in frame.plane::<f32>(0).iter().zip(frame.plane::<f32>(1)) {
                assert!(left.is_finite() && left.abs() <= ceiling);
                assert!((*right + *left * 0.5).abs() < 1e-7);
            }
        }

        assert!(processor.metrics().true_peak_dbtp.is_some());
    }

    #[test]
    fn processing_is_independent_of_packet_sizes_and_keeps_source_metrics() {
        let config = LiveLoudnessConfig::default();
        let mut whole = LiveLoudnessProcessor::new(SAMPLE_RATE, config).unwrap();
        let mut packetized = LiveLoudnessProcessor::new(SAMPLE_RATE, config).unwrap();
        let left: Vec<_> = (0..SAMPLE_RATE as usize * 4)
            .map(|sample| {
                let amplitude = if sample < SAMPLE_RATE as usize * 2 {
                    0.01
                } else {
                    0.8
                };

                (std::f64::consts::TAU * 997.0 * sample as f64 / f64::from(SAMPLE_RATE)).sin()
                    as f32
                    * amplitude
            })
            .collect();
        let right: Vec<_> = left.iter().map(|sample| sample * 0.5).collect();
        let mut frame = stereo_frame(&left, &right);
        frame.set_pts(Some(123_456));
        let mut reference = LoudnessAnalyzer::new(SAMPLE_RATE).unwrap();
        let source_metrics = reference.process_frame(&frame);
        whole.process(&mut frame);
        assert_eq!(frame.pts(), Some(123_456));
        assert_eq!(frame.samples(), left.len());
        let sizes = [1, 333, 1_024, 7_777];
        let mut offset = 0;
        let mut chunk = 0;

        while offset < left.len() {
            let end = (offset + sizes[chunk % sizes.len()]).min(left.len());
            let mut packet = stereo_frame(&left[offset..end], &right[offset..end]);
            packetized.process(&mut packet);

            for channel in 0..2 {
                assert_eq!(
                    &frame.plane::<f32>(channel)[offset..end],
                    packet.plane::<f32>(channel)
                );
            }

            offset = end;
            chunk += 1;
        }

        assert_eq!(
            whole.metrics().rider_gain_db,
            packetized.metrics().rider_gain_db
        );
        assert_eq!(
            whole.metrics().momentary_lufs,
            source_metrics.momentary_lufs
        );
        assert_eq!(
            whole.metrics().short_term_lufs,
            source_metrics.short_term_lufs
        );
        assert_eq!(
            whole.metrics().true_peak_dbtp,
            source_metrics.true_peak_dbtp
        );
        assert_eq!(
            whole.metrics().integrated_lufs,
            source_metrics.integrated_lufs
        );
    }

    #[test]
    fn non_finite_samples_do_not_poison_limiter_or_analysis() {
        let mut processor =
            LiveLoudnessProcessor::new(SAMPLE_RATE, LiveLoudnessConfig::default()).unwrap();
        let invalid = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.1];
        let mut frame = stereo_frame(&invalid, &invalid);
        processor.process(&mut frame);

        assert!(
            frame
                .plane::<f32>(0)
                .iter()
                .all(|sample| sample.is_finite())
        );
        assert!(processor.limiter_gain.is_finite());
        assert!(processor.metrics().rider_gain_db.is_finite());
        let mut frame = stereo_frame(&invalid, &invalid);
        processor.apply_gain(&mut frame);
        assert!(
            frame
                .plane::<f32>(0)
                .iter()
                .all(|sample| sample.is_finite())
        );
    }

    #[test]
    fn ceiling_limiter_never_allows_a_sample_over_the_ceiling() {
        let mut processor =
            LiveLoudnessProcessor::new(48_000, LiveLoudnessConfig::default()).unwrap();
        let mut frame = frame::Audio::new(Sample::F32(Type::Planar), 48_000, ChannelLayout::STEREO);

        for plane in 0..2 {
            frame.plane_mut::<f32>(plane).fill(1.0);
        }
        processor.process(&mut frame);
        let ceiling = db_to_gain(-1.0) as f32;
        assert!(
            frame
                .plane::<f32>(0)
                .iter()
                .all(|sample| sample.abs() <= ceiling)
        );
        assert!(processor.metrics().limiter_gain_reduction_db > 0.0);
    }

    #[test]
    fn live_lookahead_sanitizes_input_and_preview_without_changing_timestamps() {
        let mut processor =
            LiveLoudnessProcessor::new(SAMPLE_RATE, LiveLoudnessConfig::default()).unwrap();
        processor.rider_gain_db = 8.0;
        processor.target_gain_db = 8.0;
        let input = [f32::MAX, f32::NAN, f32::INFINITY, 0.1];
        let future = vec![[f32::NEG_INFINITY, f32::NAN]; 486];
        let mut frame = stereo_frame(&input, &input);
        frame.set_pts(Some(12_345));
        processor.process_with_lookahead(&mut frame, &future);

        assert_eq!(frame.pts(), Some(12_345));
        assert_eq!(frame.samples(), input.len());
        assert!(
            frame
                .plane::<f32>(0)
                .iter()
                .all(|sample| sample.is_finite() && sample.abs() <= db_to_gain(-1.0) as f32)
        );
        assert!(processor.metrics().limiter_gain_reduction_db.is_finite());
    }

    #[test]
    fn silence_is_not_amplified() {
        let mut processor =
            LiveLoudnessProcessor::new(48_000, LiveLoudnessConfig::default()).unwrap();
        let mut frame = frame::Audio::new(Sample::F32(Type::Planar), 48_000, ChannelLayout::STEREO);

        for channel in 0..2 {
            frame.plane_mut::<f32>(channel).fill(0.0);
        }

        processor.process(&mut frame);
        assert_eq!(processor.metrics().rider_gain_db, 0.0);
    }
}
