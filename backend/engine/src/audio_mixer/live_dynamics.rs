//! Shared broadcast-style processing for program output and the live_loudness example.

use ffmpeg_next::frame;

use crate::analysis::loudness::LoudnessAnalyzer;

use super::{
    AudioEffect, LiveLoudnessConfig, LiveLoudnessMetrics, lookahead::LookaheadLimiter,
    volume::GainEffect,
};

const WORKING_TARGET_LUFS: f64 = -23.0;
const PREVIEW_SECONDS: f64 = 0.050;
const RMS_SECONDS: f64 = 0.010;
const GATE_HYSTERESIS_DB: f64 = 6.0;
const GAIN_LIMIT_RAMP_SECONDS: f64 = 0.020;

/// Slow loudness AGC, stereo-linked soft-knee compressor and true-peak limiter.
/// Input leveling, compression and bounded output correction have separate time scales.
pub struct LiveDynamicsProcessor {
    analyzer: LoudnessAnalyzer,
    limiter: LookaheadLimiter,
    output_analyzer: LoudnessAnalyzer,
    output_gain_db: f64,
    output_reference_db: f64,
    output_target_db: f64,
    allow_gain_increase: bool,
    config: LiveLoudnessConfig,
    rate: u32,
    until_analysis: usize,
    agc_db: f64,
    applied_gain_db: f64,
    gain_limit_ramp_remaining: usize,
    target_db: f64,
    reduction_db: f64,
    preview_primed: bool,
    frame_reduction_db: f64,
    hold_samples: usize,
    source_power: f64,
    active: bool,
    quiet_samples: usize,
    metrics: LiveLoudnessMetrics,
    source_left: Vec<f32>,
    source_right: Vec<f32>,
    powers: Vec<f64>,
    gains: Vec<f32>,
    attack: f64,
    release: f64,
    strong_release: f64,
    detector: f64,
    pause_power: f64,
    resume_power: f64,
}

impl LiveDynamicsProcessor {
    pub fn new(rate: u32, config: LiveLoudnessConfig) -> Result<Self, ebur128_stream::Error> {
        Ok(Self {
            analyzer: LoudnessAnalyzer::new(rate)?,
            limiter: LookaheadLimiter::new(rate),
            output_analyzer: LoudnessAnalyzer::new_display_meter(rate)?,
            output_gain_db: 0.0,
            output_reference_db: 0.0,
            output_target_db: 0.0,
            allow_gain_increase: true,
            config,
            rate,
            until_analysis: (rate as usize / 10).max(1),
            agc_db: 0.0,
            applied_gain_db: 0.0,
            gain_limit_ramp_remaining: 0,
            target_db: 0.0,
            reduction_db: 0.0,
            preview_primed: false,
            frame_reduction_db: 0.0,
            hold_samples: 0,
            source_power: 0.0,
            active: false,
            quiet_samples: 0,
            metrics: LiveLoudnessMetrics::default(),
            source_left: Vec::new(),
            source_right: Vec::new(),
            powers: Vec::new(),
            gains: Vec::new(),
            attack: envelope_coefficient(config.compressor_attack_ms, rate),
            release: envelope_coefficient(config.compressor_release_ms, rate),
            strong_release: envelope_coefficient(config.compressor_strong_release_ms, rate),
            detector: envelope_coefficient(RMS_SECONDS * 1000.0, rate),
            pause_power: 10.0_f64.powf(config.pause_threshold_dbfs / 10.0),
            resume_power: 10.0_f64.powf((config.pause_threshold_dbfs + GATE_HYSTERESIS_DB) / 10.0),
        })
    }

    pub fn metrics(&self) -> LiveLoudnessMetrics {
        self.metrics
    }

    #[cfg(feature = "desktop-base")]
    pub(crate) fn compressor_gain_reduction_db(&self) -> f64 {
        self.frame_reduction_db
    }

    #[cfg(feature = "desktop-base")]
    pub(crate) fn pause_status(&self) -> &'static str {
        if self.quiet_samples == 0 && self.active {
            "SIGNAL"
        } else if self.quiet_samples
            >= ((self.config.pause_return_delay_ms / 1000.0) * f64::from(self.rate)) as usize
            && (self.agc_db > 0.0 || self.output_gain_db > 0.0 || self.output_reference_db > 0.0)
        {
            "PAUSE / GAIN RETURN"
        } else {
            "PAUSE / HOLD"
        }
    }

    /// Future raw samples needed by the compressor and the final peak limiter.
    pub fn lookahead_samples(&self) -> usize {
        (PREVIEW_SECONDS * f64::from(self.rate)).round() as usize
            + (RMS_SECONDS * f64::from(self.rate)).round() as usize
            + super::lookahead_samples(self.rate)
            + super::TRUE_PEAK_FUTURE_SAMPLES
    }

    /// Update coefficients without resetting loudness history or gain envelopes.
    pub fn update_config(&mut self, config: LiveLoudnessConfig) {
        if config != self.config {
            self.attack = envelope_coefficient(config.compressor_attack_ms, self.rate);
            self.release = envelope_coefficient(config.compressor_release_ms, self.rate);
            self.strong_release =
                envelope_coefficient(config.compressor_strong_release_ms, self.rate);
            self.pause_power = 10.0_f64.powf(config.pause_threshold_dbfs / 10.0);
            self.resume_power =
                10.0_f64.powf((config.pause_threshold_dbfs + GATE_HYSTERESIS_DB) / 10.0);
        }

        if config.max_gain_db != self.config.max_gain_db
            || config.max_attenuation_db != self.config.max_attenuation_db
        {
            self.gain_limit_ramp_remaining =
                ((f64::from(self.rate) * GAIN_LIMIT_RAMP_SECONDS).round() as usize).max(1);
        }

        self.config = config;
        self.output_target_db = self.output_target_db.clamp(
            -config.output_max_correction_db,
            config.output_max_correction_db,
        );
        self.target_db = self
            .target_db
            .clamp(config.max_attenuation_db, config.max_gain_db);
    }

    /// A new segment has no preceding preview, but retains analysis and envelopes.
    pub(crate) fn begin_segment(&mut self) {
        self.preview_primed = false;
    }

    pub fn process(&mut self, audio: &mut frame::Audio, future: &[[f32; 2]]) {
        self.process_with_gain_hold(audio, future, false);
    }

    /// Intentional playout fades may reduce gain but must never trigger compensation.
    pub(crate) fn process_with_gain_hold(
        &mut self,
        audio: &mut frame::Audio,
        future: &[[f32; 2]],
        hold: bool,
    ) {
        self.process_with_output_gain(audio, future, hold, None);
    }

    pub(crate) fn process_with_output_gain(
        &mut self,
        audio: &mut frame::Audio,
        future: &[[f32; 2]],
        hold: bool,
        mut output_gain: Option<&mut GainEffect>,
    ) {
        self.allow_gain_increase = !hold;
        if audio.planes() != 2 || audio.samples() == 0 {
            return;
        }

        self.frame_reduction_db = 0.0;
        self.source_left.clear();
        self.source_right.clear();
        self.source_left
            .extend(audio.plane::<f32>(0).iter().copied().map(finite));
        self.source_right
            .extend(audio.plane::<f32>(1).iter().copied().map(finite));
        let samples = audio.samples();
        let preview = (PREVIEW_SECONDS * f64::from(self.rate)).round() as usize;
        let window = ((RMS_SECONDS * f64::from(self.rate)).round() as usize).max(1);
        // The powers are needed repeatedly by the rolling preview. Compute
        // once, including sanitized future samples, and reuse the allocation.
        let mut powers = std::mem::take(&mut self.powers);
        powers.clear();
        let mut gains = std::mem::take(&mut self.gains);
        powers.extend(
            self.source_left
                .iter()
                .zip(&self.source_right)
                .map(|(&left, &right)| power([left, right])),
        );
        powers.extend(future.iter().copied().map(power));
        let power_at = |index: usize| powers.get(index).copied().unwrap_or(0.0);
        let mut preview_power: f64 = (preview..preview + window).map(power_at).sum();
        if !self.preview_primed {
            // Startup has no preceding audio in which to build the envelope.
            // Include the whole preview interval, so short initial transients
            // cannot disappear before the forward detector reaches them.
            let mut initial_power: f64 = (0..window).map(power_at).sum();
            let mut maximum_power = initial_power;

            for index in 0..preview {
                initial_power =
                    (initial_power - power_at(index) + power_at(index + window)).max(0.0);
                maximum_power = maximum_power.max(initial_power);
            }

            let level = 10.0 * (maximum_power / window as f64).max(1e-20).log10() + self.agc_db;
            let required = compression(
                level,
                self.config.compressor_threshold_dbfs,
                self.config.compressor_ratio,
                self.config.compressor_knee_db,
            );

            if required > self.reduction_db {
                self.reduction_db = required;
                self.hold_samples = self.hold_samples.max(
                    ((self.config.compressor_hold_ms / 1000.0) * f64::from(self.rate)) as usize
                        + preview,
                );
            }

            self.preview_primed = true;
        }

        let mut offset = 0;

        while offset < samples {
            let end = (offset + self.until_analysis).min(samples);
            let measurements = self.analyzer.process_samples(
                &self.source_left[offset..end],
                &self.source_right[offset..end],
            );
            gains.clear();

            for index in offset..end {
                self.source_power += self.detector * (power_at(index) - self.source_power);
                self.update_gate();
                self.update_agc();
                let level = 10.0 * (preview_power / window as f64).max(1e-20).log10() + self.agc_db;
                // The compressor threshold is RMS dBFS, independent of the LUFS target.
                let desired = compression(
                    level,
                    self.config.compressor_threshold_dbfs,
                    self.config.compressor_ratio,
                    self.config.compressor_knee_db,
                );

                // A loud entrance after silence must already be controlled when
                // it arrives. Preview may lower gain during a pause, never raise it.
                if desired > self.reduction_db {
                    self.reduction_db += self.attack * (desired - self.reduction_db);
                    self.hold_samples =
                        ((self.config.compressor_hold_ms / 1000.0) * f64::from(self.rate)) as usize;
                } else if self.active {
                    if self.hold_samples > 0 {
                        self.hold_samples -= 1;
                    } else if self.quiet_samples == 0 && self.allow_gain_increase {
                        // Large reductions recover faster; small changes recover gently.
                        let coefficient = if self.reduction_db > 12.0 {
                            self.strong_release
                        } else {
                            self.release
                        };
                        self.reduction_db += coefficient * (desired - self.reduction_db);
                    }
                }

                // Keep the compressor's working level independent of the desired
                // output loudness. Changes to the output reference ramp smoothly.
                let reference_target = self.config.target_lufs - WORKING_TARGET_LUFS;
                let reference_speed = if reference_target < self.output_reference_db {
                    self.config.gain_down_db_per_second
                } else {
                    self.config.gain_up_db_per_second
                } / f64::from(self.rate);

                if reference_target <= self.output_reference_db
                    || (self.active
                        && self.quiet_samples == 0
                        && self.allow_gain_increase
                        && self.metrics.limiter_gain_reduction_db < 0.2)
                {
                    self.output_reference_db += (reference_target - self.output_reference_db)
                        .clamp(-reference_speed, reference_speed);
                }

                let output_step = if self.output_target_db < self.output_gain_db {
                    self.config.output_gain_down_db_per_second
                } else {
                    self.config.output_gain_up_db_per_second
                } / f64::from(self.rate);
                let can_raise = self.active
                    && self.quiet_samples == 0
                    && self.allow_gain_increase
                    && self.metrics.limiter_gain_reduction_db < 0.2;

                if self.output_target_db <= self.output_gain_db || can_raise {
                    self.output_gain_db += (self.output_target_db - self.output_gain_db)
                        .clamp(-output_step, output_step);
                }

                self.frame_reduction_db = self.frame_reduction_db.max(self.reduction_db);
                let total_gain = (self.agc_db + self.output_reference_db + self.output_gain_db)
                    .clamp(self.config.max_attenuation_db, self.config.max_gain_db);
                if self.gain_limit_ramp_remaining > 0 {
                    self.applied_gain_db +=
                        (total_gain - self.applied_gain_db) / self.gain_limit_ramp_remaining as f64;
                    self.gain_limit_ramp_remaining -= 1;
                } else {
                    self.applied_gain_db = total_gain;
                }

                let gain = ((self.applied_gain_db - self.reduction_db)
                    * (std::f64::consts::LOG2_10 / 20.0))
                    .exp2() as f32;
                gains.push(gain);
                preview_power = (preview_power - power_at(index + preview)
                    + power_at(index + preview + window))
                .max(0.0);
            }

            for (plane, source) in [&self.source_left, &self.source_right]
                .into_iter()
                .enumerate()
            {
                let output = audio.plane_mut::<f32>(plane);

                for index in offset..end {
                    output[index] = source[index] * gains[index - offset];
                }
            }

            let output_measurements = self.output_analyzer.process_samples(
                &audio.plane::<f32>(0)[offset..end],
                &audio.plane::<f32>(1)[offset..end],
            );

            self.until_analysis -= end - offset;
            offset = end;
            self.metrics.momentary_lufs = measurements.momentary_lufs;
            self.metrics.short_term_lufs = measurements.short_term_lufs;
            self.metrics.integrated_lufs = measurements.integrated_lufs;
            self.metrics.true_peak_dbtp = measurements.true_peak_dbtp;

            if self.until_analysis == 0 {
                self.until_analysis = (self.rate as usize / 10).max(1);

                if self.active
                    && self.quiet_samples == 0
                    && self.allow_gain_increase
                    && let Some(level) = output_measurements.short_term_lufs
                {
                    let error = self.config.target_lufs - level;

                    if error.abs() > self.config.dead_band_lu {
                        self.output_target_db = (self.output_gain_db + error).clamp(
                            -self.config.output_max_correction_db,
                            self.config.output_max_correction_db,
                        );
                    } else {
                        self.output_target_db = self.output_gain_db.clamp(
                            -self.config.output_max_correction_db,
                            self.config.output_max_correction_db,
                        );
                    }
                }

                if self.active
                    && let Some(level) =
                        measurements.short_term_lufs.or(measurements.momentary_lufs)
                    && level >= self.config.silence_gate_lufs
                {
                    let error = WORKING_TARGET_LUFS - level;
                    self.target_db = if error.abs() <= self.config.dead_band_lu {
                        0.0
                    } else {
                        error
                    }
                    .clamp(self.config.max_attenuation_db, self.config.max_gain_db);
                }
            }
        }

        self.powers = powers;
        self.gains = gains;
        self.metrics.rider_gain_db = self.applied_gain_db;
        let gain_bound_db = if self.gain_limit_ramp_remaining > 0 {
            self.applied_gain_db.max(
                (self.agc_db + self.output_reference_db + self.output_gain_db)
                    .clamp(self.config.max_attenuation_db, self.config.max_gain_db),
            )
        } else {
            self.applied_gain_db
        };
        let preview_gain = 10.0_f64.powf(
            (gain_bound_db
                + (2.0 * self.config.gain_up_db_per_second
                    + self.config.output_gain_up_db_per_second)
                    * super::LIVE_LATENCY.as_secs_f64())
                / 20.0,
        );
        // Manual volume belongs after loudness correction and before the final limiter.
        let preview_gain = if let Some(effect) = output_gain.as_mut() {
            effect.process(audio);
            preview_gain * effect.preview_gain_bound()
        } else {
            preview_gain
        };
        self.metrics.limiter_gain_reduction_db = self.limiter.process(
            audio,
            future,
            preview_gain,
            self.config.true_peak_ceiling_dbtp,
        );
    }

    fn update_gate(&mut self) {
        if self.source_power >= self.resume_power {
            self.active = true;
            self.quiet_samples = 0;
        } else if self.source_power < self.pause_power {
            self.quiet_samples = self.quiet_samples.saturating_add(1);

            if self.quiet_samples
                >= ((self.config.pause_hold_ms / 1000.0) * f64::from(self.rate)) as usize
            {
                self.active = false;
            }
        } else {
            self.quiet_samples = 0;
        }
    }

    fn update_agc(&mut self) {
        if !self.active {
            if self.quiet_samples
                >= ((self.config.pause_return_delay_ms / 1000.0) * f64::from(self.rate)) as usize
                && self.output_reference_db > 0.0
            {
                self.output_reference_db =
                    (self.output_reference_db - 1.0 / f64::from(self.rate)).max(0.0);
            }

            if self.quiet_samples
                >= ((self.config.pause_return_delay_ms / 1000.0) * f64::from(self.rate)) as usize
                && self.agc_db > 0.0
            {
                self.agc_db = (self.agc_db - 1.0 / f64::from(self.rate)).max(0.0);
            }

            if self.quiet_samples
                >= ((self.config.pause_return_delay_ms / 1000.0) * f64::from(self.rate)) as usize
                && self.output_gain_db > 0.0
            {
                self.output_gain_db = (self.output_gain_db
                    - self.config.output_gain_down_db_per_second / f64::from(self.rate))
                .max(0.0);
                self.output_target_db = self.output_gain_db.clamp(
                    -self.config.output_max_correction_db,
                    self.config.output_max_correction_db,
                );
            }

            return;
        }

        // Freeze upward gain as soon as the signal is quiet, even during pause hold.
        if (self.quiet_samples > 0 || !self.allow_gain_increase) && self.target_db > self.agc_db {
            return;
        }

        let speed = if self.target_db < self.agc_db {
            self.config.gain_down_db_per_second
        } else {
            self.config.gain_up_db_per_second
        };
        let step = speed / f64::from(self.rate);
        self.agc_db += (self.target_db - self.agc_db).clamp(-step, step);
    }
}

fn envelope_coefficient(milliseconds: f64, rate: u32) -> f64 {
    1.0 - (-1.0 / ((milliseconds / 1000.0) * f64::from(rate))).exp()
}

fn finite(sample: f32) -> f32 {
    if sample.is_finite() { sample } else { 0.0 }
}

fn power(sample: [f32; 2]) -> f64 {
    let left = f64::from(finite(sample[0]));
    let right = f64::from(finite(sample[1]));

    (left * left + right * right) * 0.5
}

fn compression(level: f64, threshold: f64, ratio: f64, knee_db: f64) -> f64 {
    let excess = level - threshold;
    let slope = 1.0 - 1.0 / ratio;

    if excess <= -knee_db / 2.0 {
        0.0
    } else if excess < knee_db / 2.0 {
        slope * (excess + knee_db / 2.0).powi(2) / (2.0 * knee_db)
    } else {
        (slope * excess).min(24.0)
    }
}

#[cfg(test)]
mod tests {
    use ffmpeg_next::{
        ChannelLayout,
        format::{Sample, sample::Type},
    };

    use super::*;

    const RATE: u32 = 48000;

    fn tone(position: usize, amplitude: f32) -> [f32; 2] {
        let value = (std::f64::consts::TAU * 997.0 * position as f64 / f64::from(RATE)).sin()
            as f32
            * amplitude;
        [value, value * 0.5]
    }

    fn process(source: &[[f32; 2]], size: usize, config: LiveLoudnessConfig) -> Vec<[f32; 2]> {
        let mut processor = LiveDynamicsProcessor::new(RATE, config).unwrap();
        let mut output = Vec::new();

        for offset in (0..source.len()).step_by(size) {
            let end = (offset + size).min(source.len());
            let mut audio = frame::Audio::new(
                Sample::F32(Type::Planar),
                end - offset,
                ChannelLayout::STEREO,
            );
            audio.set_rate(RATE);
            audio.set_pts(Some(offset as i64));

            for channel in 0..2 {
                for (index, sample) in source[offset..end].iter().enumerate() {
                    audio.plane_mut::<f32>(channel)[index] = sample[channel];
                }
            }

            let future_end = (end + processor.lookahead_samples()).min(source.len());
            processor.process(&mut audio, &source[end..future_end]);
            assert_eq!(audio.pts(), Some(offset as i64));
            output.extend(
                audio
                    .plane::<f32>(0)
                    .iter()
                    .zip(audio.plane::<f32>(1))
                    .map(|(&left, &right)| [left, right]),
            );
        }

        output
    }

    #[test]
    fn runtime_changes_refresh_cached_envelopes_and_pause_thresholds() {
        let config = LiveLoudnessConfig {
            compressor_attack_ms: 13.0,
            compressor_release_ms: 1700.0,
            compressor_strong_release_ms: 450.0,
            pause_threshold_dbfs: -65.0,
            ..LiveLoudnessConfig::default()
        };
        let fresh = LiveDynamicsProcessor::new(RATE, config).unwrap();
        let mut updated = LiveDynamicsProcessor::new(RATE, LiveLoudnessConfig::default()).unwrap();
        updated.update_config(config);
        assert_eq!(fresh.attack, updated.attack);
        assert_eq!(fresh.release, updated.release);
        assert_eq!(fresh.strong_release, updated.strong_release);
        assert_eq!(fresh.pause_power, updated.pause_power);
        assert_eq!(fresh.resume_power, updated.resume_power);
        updated.source_power = updated.resume_power;
        updated.update_gate();
        assert!(updated.active);
        updated.source_power = updated.pause_power * 0.99;
        updated.update_gate();
        assert_eq!(updated.quiet_samples, 1);
    }

    #[test]
    fn tightened_and_relaxed_gain_limits_ramp_without_sample_jumps() {
        let config = LiveLoudnessConfig {
            compressor_ratio: 1.0,
            gain_up_db_per_second: 0.0,
            gain_down_db_per_second: 0.0,
            ..LiveLoudnessConfig::default()
        };
        let mut processor = LiveDynamicsProcessor::new(RATE, config).unwrap();
        processor.agc_db = 8.0;
        processor.target_db = 8.0;
        processor.active = true;
        processor.source_power = 0.0001;
        processor.preview_primed = true;
        let mut audio = frame::Audio::new(Sample::F32(Type::Planar), 480, ChannelLayout::STEREO);
        audio.set_rate(RATE);
        audio.plane_mut::<f32>(0).fill(0.01);
        audio.plane_mut::<f32>(1).fill(0.01);
        processor.process(&mut audio, &[[0.01; 2]; 3500]);
        let mut previous = *audio.plane::<f32>(0).last().unwrap();

        for limit in [0.0, 8.0] {
            processor.update_config(LiveLoudnessConfig {
                max_gain_db: limit,
                ..config
            });

            for block in 0..2 {
                // Runtime configuration is re-read each frame; it must not restart the ramp.
                processor.update_config(processor.config);
                audio.plane_mut::<f32>(0).fill(0.01);
                audio.plane_mut::<f32>(1).fill(0.01);
                processor.process(&mut audio, &[[0.01; 2]; 3500]);

                for &sample in audio.plane::<f32>(0) {
                    let delta_db = 20.0 * (f64::from(sample) / f64::from(previous)).log10();
                    assert!(delta_db.abs() < 0.009, "sample jump: {delta_db} dB");
                    previous = sample;
                }

                if block == 1 {
                    assert!((processor.metrics().rider_gain_db - limit).abs() < 1e-9);
                }
            }
        }
    }

    #[test]
    fn segment_repriming_keeps_loudness_history_and_existing_attenuation() {
        let mut processor =
            LiveDynamicsProcessor::new(RATE, LiveLoudnessConfig::default()).unwrap();
        processor.preview_primed = true;
        processor.agc_db = 2.0;
        processor.reduction_db = 6.0;
        processor.until_analysis = 100;
        processor.begin_segment();
        assert!(!processor.preview_primed);
        assert_eq!(processor.agc_db, 2.0);
        assert_eq!(processor.until_analysis, 100);
        let mut audio = frame::Audio::new(Sample::F32(Type::Planar), 48, ChannelLayout::STEREO);
        audio.set_rate(RATE);
        audio.plane_mut::<f32>(0).fill(0.0);
        audio.plane_mut::<f32>(1).fill(0.0);
        processor.process(&mut audio, &[]);
        assert_eq!(processor.reduction_db, 6.0);
        assert_eq!(processor.until_analysis, 52);
    }

    #[test]
    fn knee_supports_hard_and_soft_transitions() {
        assert_eq!(compression(-26.0, -26.0, 3.0, 0.0), 0.0);
        assert_eq!(compression(-27.0, -26.0, 3.0, 0.0), 0.0);
        assert!((compression(-20.0, -26.0, 3.0, 0.0) - 4.0).abs() < 1e-12);
        assert!(compression(-26.0, -26.0, 3.0, 6.0) > 0.0);
        assert!((compression(-23.0, -26.0, 3.0, 6.0) - 2.0).abs() < 1e-12);
    }

    #[test]
    fn configurable_pause_times_freeze_then_return_positive_gain() {
        let config = LiveLoudnessConfig {
            pause_hold_ms: 50.0,
            pause_return_delay_ms: 150.0,
            output_gain_down_db_per_second: 0.5,
            ..LiveLoudnessConfig::default()
        };
        let mut processor = LiveDynamicsProcessor::new(RATE, config).unwrap();
        processor.active = true;
        processor.agc_db = 4.0;
        processor.target_db = 4.0;
        processor.output_gain_db = 2.0;
        processor.output_target_db = 2.0;

        for _ in 0..RATE / 10 {
            processor.update_gate();
            processor.update_agc();
        }

        assert!(!processor.active);
        assert_eq!(processor.agc_db, 4.0);
        assert_eq!(processor.output_gain_db, 2.0);

        for _ in 0..RATE / 5 {
            processor.update_gate();
            processor.update_agc();
        }

        assert!((processor.agc_db - 3.85).abs() < 0.0001);
        assert!((processor.output_gain_db - 1.925).abs() < 0.0001);
    }

    #[test]
    fn runtime_changes_preserve_envelopes_and_ramp_output_correction_to_new_limit() {
        let mut processor =
            LiveDynamicsProcessor::new(RATE, LiveLoudnessConfig::default()).unwrap();
        processor.agc_db = 2.0;
        processor.reduction_db = 4.0;
        processor.output_gain_db = 2.0;
        processor.output_target_db = 3.0;
        processor.preview_primed = true;
        processor.hold_samples = 1000;
        processor.update_config(LiveLoudnessConfig {
            output_max_correction_db: 0.5,
            output_gain_down_db_per_second: 2.0,
            ..LiveLoudnessConfig::default()
        });
        assert_eq!(processor.agc_db, 2.0);
        assert_eq!(processor.reduction_db, 4.0);
        assert_eq!(processor.output_gain_db, 2.0);
        assert_eq!(processor.output_target_db, 0.5);
        assert_eq!(processor.hold_samples, 1000);
        let mut audio = frame::Audio::new(Sample::F32(Type::Planar), 480, ChannelLayout::STEREO);
        audio.set_rate(RATE);
        audio.plane_mut::<f32>(0).fill(0.0);
        audio.plane_mut::<f32>(1).fill(0.0);
        processor.process(&mut audio, &[]);
        assert!((processor.output_gain_db - 1.98).abs() < 1e-9);
        assert!(
            audio
                .plane::<f32>(0)
                .iter()
                .all(|sample| sample.is_finite())
        );
    }

    #[test]
    fn output_correction_uses_configured_upward_speed_and_limit() {
        let mut processor = LiveDynamicsProcessor::new(
            RATE,
            LiveLoudnessConfig {
                output_gain_up_db_per_second: 1.5,
                output_max_correction_db: 0.01,
                ..LiveLoudnessConfig::default()
            },
        )
        .unwrap();
        processor.active = true;
        processor.source_power = 0.0001;
        processor.output_target_db = 1.0;
        processor.update_config(processor.config);
        let mut audio = frame::Audio::new(Sample::F32(Type::Planar), 480, ChannelLayout::STEREO);
        audio.set_rate(RATE);
        audio.plane_mut::<f32>(0).fill(0.01);
        audio.plane_mut::<f32>(1).fill(0.01);
        processor.process(&mut audio, &[]);
        assert!((processor.output_gain_db - 0.01).abs() < 1e-9);
        processor.update_config(LiveLoudnessConfig {
            output_max_correction_db: 1.0,
            ..processor.config
        });
        processor.output_target_db = 1.0;
        audio.plane_mut::<f32>(0).fill(0.01);
        audio.plane_mut::<f32>(1).fill(0.01);
        processor.process(&mut audio, &[]);
        assert!((processor.output_gain_db - 0.025).abs() < 1e-9);
    }

    #[test]
    fn configurable_compressor_envelope_controls_attack_hold_and_release() {
        fn reduction(
            attack_ms: f64,
            hold_ms: f64,
            release_ms: f64,
            strong_release_ms: f64,
            initial: f64,
            amplitude: f32,
        ) -> f64 {
            let mut processor = LiveDynamicsProcessor::new(
                RATE,
                LiveLoudnessConfig {
                    compressor_attack_ms: attack_ms,
                    compressor_hold_ms: hold_ms,
                    compressor_release_ms: release_ms,
                    compressor_strong_release_ms: strong_release_ms,
                    gain_up_db_per_second: 0.0,
                    gain_down_db_per_second: 0.0,
                    ..LiveLoudnessConfig::default()
                },
            )
            .unwrap();
            processor.preview_primed = true;
            processor.active = true;
            processor.reduction_db = initial;
            processor.hold_samples = (hold_ms * f64::from(RATE) / 1000.0) as usize;
            let mut audio =
                frame::Audio::new(Sample::F32(Type::Planar), 480, ChannelLayout::STEREO);
            audio.set_rate(RATE);
            audio.plane_mut::<f32>(0).fill(amplitude);
            audio.plane_mut::<f32>(1).fill(amplitude);
            processor.process(
                &mut audio,
                &vec![[amplitude; 2]; processor.lookahead_samples()],
            );
            processor.reduction_db
        }
        assert!(
            reduction(1.0, 0.0, 1200.0, 500.0, 0.0, 0.5)
                > reduction(50.0, 0.0, 1200.0, 500.0, 0.0, 0.5)
        );
        assert_eq!(reduction(5.0, 100.0, 1200.0, 500.0, 6.0, 0.01), 6.0);
        assert!(
            reduction(5.0, 0.0, 100.0, 500.0, 6.0, 0.01)
                < reduction(5.0, 0.0, 2000.0, 500.0, 6.0, 0.01)
        );
        assert!(
            reduction(5.0, 0.0, 1200.0, 100.0, 18.0, 0.01)
                < reduction(5.0, 0.0, 1200.0, 2000.0, 18.0, 0.01)
        );
    }

    #[test]
    fn compressor_catches_the_first_loud_samples_and_preserves_earlier_quiet_audio() {
        let rate = RATE as usize;
        let source: Vec<_> = (0..rate * 4)
            .map(|index| tone(index, if index < rate * 2 { 0.01 } else { 0.8 }))
            .collect();
        let config = LiveLoudnessConfig {
            compressor_ratio: 6.0,
            max_gain_db: 0.0,
            gain_up_db_per_second: 0.0,
            ..LiveLoudnessConfig::default()
        };
        let output = process(&source, 777, config);

        for index in rate..rate * 2 - rate / 10 {
            assert!((output[index][0] - source[index][0]).abs() < 0.000001);
        }

        let first_loud_peak = output[rate * 2..rate * 2 + rate / 50]
            .iter()
            .map(|sample| sample[0].abs())
            .fold(0.0_f32, f32::max);
        assert!(
            first_loud_peak < 0.16,
            "uncontrolled first loud section: {first_loud_peak}"
        );
        assert_eq!(source.len(), output.len());

        for sample in output {
            assert!((sample[0] * 0.5 - sample[1]).abs() < 0.000001);
            assert!(sample[0].abs() <= 10.0_f32.powf(-1.0 / 20.0));
        }
    }

    #[test]
    fn pause_freezes_gain_then_slowly_removes_positive_gain_without_a_noise_gate() {
        let mut processor =
            LiveDynamicsProcessor::new(RATE, LiveLoudnessConfig::default()).unwrap();
        let mut gains = Vec::new();

        for block in 0..120 {
            let mut audio =
                frame::Audio::new(Sample::F32(Type::Planar), 4800, ChannelLayout::STEREO);
            audio.set_rate(RATE);

            for channel in 0..2 {
                for (index, sample) in audio.plane_mut::<f32>(channel).iter_mut().enumerate() {
                    *sample = if block < 80 {
                        tone(block * 4800 + index, 0.01)[channel]
                    } else {
                        0.00001
                    };
                }
            }

            processor.process(&mut audio, &[]);
            gains.push(processor.agc_db - processor.reduction_db);

            if block > 82 {
                assert!(audio.plane::<f32>(0).iter().all(|&sample| sample > 0.0));
                assert!(
                    gains[block] <= gains[block - 1] + 0.00001,
                    "noise gain rose during pause"
                );
            }
        }

        assert!(gains[79] > 2.0);
        assert!((gains[95] - gains[85]).abs() < 0.00001);
        assert!(gains[119] < gains[95] - 1.0);
    }

    #[test]
    fn processing_is_independent_of_packet_boundaries() {
        let rate = RATE as usize;
        let source: Vec<_> = (0..rate * 5)
            .map(|index| {
                tone(
                    index,
                    if (rate * 2..rate * 3).contains(&index) {
                        0.5
                    } else {
                        0.02
                    },
                )
            })
            .collect();
        let config = LiveLoudnessConfig::default();
        let small = process(&source, 777, config);
        let large = process(&source, 4800, config);

        for (index, (left, right)) in small.iter().zip(&large).enumerate() {
            assert!(
                (left[0] - right[0]).abs() < 0.00001,
                "packet-dependent processing at sample {index}: {} vs {}",
                left[0],
                right[0]
            );
        }
    }
    #[test]
    fn loud_entrance_after_silence_is_prepared_before_the_pause_detector_opens() {
        let rate = RATE as usize;
        let source: Vec<_> = (0..rate * 3)
            .map(|index| tone(index, if index < rate * 2 { 0.0 } else { 0.8 }))
            .collect();
        let output = process(
            &source,
            1024,
            LiveLoudnessConfig {
                compressor_ratio: 6.0,
                ..LiveLoudnessConfig::default()
            },
        );
        let peak = output[rate * 2..rate * 2 + rate / 50]
            .iter()
            .map(|sample| sample[0].abs())
            .fold(0.0_f32, f32::max);

        assert!(peak < 0.16, "loud entrance escaped compression: {peak}");
        assert!(
            output[..rate * 2]
                .iter()
                .all(|sample| *sample == [0.0, 0.0])
        );
    }

    #[test]
    fn output_correction_is_slow_bounded_and_respects_total_gain_limits() {
        let config = LiveLoudnessConfig {
            compressor_threshold_dbfs: -40.0,
            ..LiveLoudnessConfig::default()
        };
        let mut processor = LiveDynamicsProcessor::new(RATE, config).unwrap();

        for block in 0..100 {
            let mut audio =
                frame::Audio::new(Sample::F32(Type::Planar), 4800, ChannelLayout::STEREO);
            audio.set_rate(RATE);

            for channel in 0..2 {
                for (index, sample) in audio.plane_mut::<f32>(channel).iter_mut().enumerate() {
                    *sample = tone(block * 4800 + index, 0.05)[channel];
                }
            }

            let before = processor.output_gain_db;
            processor.process(&mut audio, &[]);
            assert!((processor.output_gain_db - before).abs() <= 0.025001);
            assert!(processor.output_gain_db.abs() <= 3.0);
            assert!(
                (config.max_attenuation_db..=config.max_gain_db)
                    .contains(&processor.metrics().rider_gain_db)
            );
        }

        assert!(processor.output_gain_db > 0.1);
        // A scheduled fade must freeze every mechanism that could raise gain.
        let before = (
            processor.agc_db,
            processor.output_gain_db,
            processor.reduction_db,
        );
        let mut audio = frame::Audio::new(Sample::F32(Type::Planar), 4800, ChannelLayout::STEREO);
        audio.set_rate(RATE);

        for channel in 0..2 {
            audio.plane_mut::<f32>(channel).fill(0.002);
        }

        processor.process_with_gain_hold(&mut audio, &[], true);
        assert!(processor.agc_db <= before.0);
        assert!(processor.output_gain_db <= before.1);
        assert!(processor.reduction_db >= before.2);
    }

    #[test]
    fn default_compression_preserves_more_musical_dynamics_than_strong_compression() {
        let source: Vec<_> = (0..RATE as usize).map(|index| tone(index, 0.4)).collect();
        let config = LiveLoudnessConfig {
            max_gain_db: 0.0,
            gain_down_db_per_second: 0.0,
            ..LiveLoudnessConfig::default()
        };
        let normal = process(&source, 1024, config);
        let strong = process(
            &source,
            1024,
            LiveLoudnessConfig {
                compressor_ratio: 6.0,
                ..config
            },
        );
        let energy = |audio: &[[f32; 2]]| {
            audio
                .iter()
                .map(|sample| f64::from(sample[0]).powi(2))
                .sum::<f64>()
        };
        assert!(energy(&normal) > energy(&strong) * 1.5);
        assert!(energy(&normal) < energy(&source));
    }

    #[test]
    fn a_higher_output_target_does_not_boost_noise_before_program_starts() {
        let source: Vec<_> = (0..RATE as usize)
            .map(|index| tone(index, 0.0005))
            .collect();
        let output = process(
            &source,
            1024,
            LiveLoudnessConfig {
                target_lufs: -14.0,
                ..LiveLoudnessConfig::default()
            },
        );
        assert_eq!(output, source);
    }

    #[test]
    fn output_target_does_not_change_the_compressors_working_level() {
        let mut processors = [
            LiveDynamicsProcessor::new(RATE, LiveLoudnessConfig::default()).unwrap(),
            LiveDynamicsProcessor::new(
                RATE,
                LiveLoudnessConfig {
                    target_lufs: -20.0,
                    ..LiveLoudnessConfig::default()
                },
            )
            .unwrap(),
        ];

        for block in 0..80 {
            for processor in &mut processors {
                let mut audio =
                    frame::Audio::new(Sample::F32(Type::Planar), 4800, ChannelLayout::STEREO);
                audio.set_rate(RATE);

                for channel in 0..2 {
                    for (index, sample) in audio.plane_mut::<f32>(channel).iter_mut().enumerate() {
                        *sample = tone(block * 4800 + index, 0.05)[channel];
                    }
                }

                processor.process(&mut audio, &[]);
            }

            assert!((processors[0].agc_db - processors[1].agc_db).abs() < 1e-9);
            assert!((processors[0].reduction_db - processors[1].reduction_db).abs() < 1e-9);
        }

        assert!(
            processors[1].metrics().rider_gain_db > processors[0].metrics().rider_gain_db + 1.0
        );
    }

    #[test]
    fn short_initial_burst_is_not_missed_by_the_forward_detector() {
        let source: Vec<_> = (0..RATE as usize / 2)
            .map(|index| {
                tone(
                    index,
                    if index < RATE as usize / 100 {
                        0.8
                    } else {
                        0.0
                    },
                )
            })
            .collect();
        let output = process(&source, 777, LiveLoudnessConfig::default());
        let peak = output
            .iter()
            .map(|sample| sample[0].abs())
            .fold(0.0, f32::max);
        assert!(peak < 0.3, "initial transient escaped compression: {peak}");
    }

    #[test]
    fn malformed_source_and_preview_samples_do_not_poison_processing() {
        let source = vec![[f32::NAN, f32::INFINITY]; 1024];
        let output = process(&source, 777, LiveLoudnessConfig::default());

        assert!(output.iter().all(|sample| *sample == [0.0, 0.0]));
    }
}
