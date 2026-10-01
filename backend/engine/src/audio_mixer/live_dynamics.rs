//! Experimental broadcast-style processing used by the live_loudness example.

use ffmpeg_next::frame;

use crate::analysis::loudness::LoudnessAnalyzer;

use super::{LiveLoudnessConfig, LiveLoudnessMetrics, lookahead::LookaheadLimiter};

const PREVIEW_SECONDS: f64 = 0.050;
const RMS_SECONDS: f64 = 0.010;
const RATIO: f64 = 6.0;
const PAUSE_THRESHOLD_DBFS: f64 = -55.0;
const KNEE_DB: f64 = 6.0;
const ATTACK_SECONDS: f64 = 0.005;
const HOLD_SECONDS: f64 = 0.100;
const PAUSE_HOLD_SECONDS: f64 = 0.300;
const PAUSE_RETURN_SECONDS: f64 = 2.0;
const GATE_HYSTERESIS_DB: f64 = 6.0;

/// Slow loudness AGC, stereo-linked soft-knee compressor and true-peak limiter.
/// This experimental processor is opt-in and does not change production live input.
pub struct LiveDynamicsProcessor {
    analyzer: LoudnessAnalyzer,
    limiter: LookaheadLimiter,
    config: LiveLoudnessConfig,
    rate: u32,
    until_analysis: usize,
    agc_db: f64,
    target_db: f64,
    reduction_db: f64,
    frame_reduction_db: f64,
    hold_samples: usize,
    source_power: f64,
    active: bool,
    quiet_samples: usize,
    metrics: LiveLoudnessMetrics,
}

impl LiveDynamicsProcessor {
    pub fn new(rate: u32, config: LiveLoudnessConfig) -> Result<Self, ebur128_stream::Error> {
        Ok(Self {
            analyzer: LoudnessAnalyzer::new(rate)?,
            limiter: LookaheadLimiter::new(rate),
            config,
            rate,
            until_analysis: (rate as usize / 10).max(1),
            agc_db: 0.0,
            target_db: 0.0,
            reduction_db: 0.0,
            frame_reduction_db: 0.0,
            hold_samples: 0,
            source_power: 0.0,
            active: false,
            quiet_samples: 0,
            metrics: LiveLoudnessMetrics::default(),
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
        } else if self.quiet_samples >= (PAUSE_RETURN_SECONDS * f64::from(self.rate)) as usize
            && self.agc_db > 0.0
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

    pub fn process(&mut self, audio: &mut frame::Audio, future: &[[f32; 2]]) {
        if audio.planes() != 2 || audio.samples() == 0 {
            return;
        }

        self.frame_reduction_db = 0.0;
        let source: Vec<[f32; 2]> = audio
            .plane::<f32>(0)
            .iter()
            .zip(audio.plane::<f32>(1))
            .map(|(&left, &right)| [finite(left), finite(right)])
            .collect();
        let preview = (PREVIEW_SECONDS * f64::from(self.rate)).round() as usize;
        let window = ((RMS_SECONDS * f64::from(self.rate)).round() as usize).max(1);
        let power_at = |index: usize| {
            source
                .get(index)
                .or_else(|| future.get(index.saturating_sub(source.len())))
                .map_or(0.0, |sample| power(*sample))
        };
        let mut preview_power: f64 = (preview..preview + window).map(power_at).sum();
        let attack = 1.0 - (-1.0 / (ATTACK_SECONDS * f64::from(self.rate))).exp();
        let detector = 1.0 - (-1.0 / (RMS_SECONDS * f64::from(self.rate))).exp();
        let mut offset = 0;

        while offset < source.len() {
            let end = (offset + self.until_analysis).min(source.len());
            let left: Vec<_> = source[offset..end].iter().map(|sample| sample[0]).collect();
            let right: Vec<_> = source[offset..end].iter().map(|sample| sample[1]).collect();
            let measurements = self.analyzer.process_samples(&left, &right);

            for (index, &sample) in source.iter().enumerate().take(end).skip(offset) {
                self.source_power += detector * (power(sample) - self.source_power);
                self.update_gate();
                self.update_agc();
                let level = 10.0 * (preview_power / window as f64).max(1e-20).log10() + self.agc_db;
                // Equal-energy stereo adds about 3 LU to the per-channel RMS.
                // This threshold is a listening-test starting point, not a LUFS meter.
                let desired = compression(level, self.config.target_lufs - 3.0);

                // A loud entrance after silence must already be controlled when
                // it arrives. Preview may lower gain during a pause, never raise it.
                if desired > self.reduction_db {
                    self.reduction_db += attack * (desired - self.reduction_db);
                    self.hold_samples = (HOLD_SECONDS * f64::from(self.rate)) as usize;
                } else if self.active {
                    if self.hold_samples > 0 {
                        self.hold_samples -= 1;
                    } else if self.quiet_samples == 0 {
                        // Large reductions recover faster; small changes recover gently.
                        let release = if self.reduction_db > 12.0 { 0.5 } else { 1.2 };
                        let coefficient = 1.0 - (-1.0 / (release * f64::from(self.rate))).exp();
                        self.reduction_db += coefficient * (desired - self.reduction_db);
                    }
                }

                self.frame_reduction_db = self.frame_reduction_db.max(self.reduction_db);
                let gain = 10.0_f64.powf((self.agc_db - self.reduction_db) / 20.0) as f32;
                audio.plane_mut::<f32>(0)[index] = sample[0] * gain;
                audio.plane_mut::<f32>(1)[index] = sample[1] * gain;
                preview_power = (preview_power - power_at(index + preview)
                    + power_at(index + preview + window))
                .max(0.0);
            }

            self.until_analysis -= end - offset;
            offset = end;
            self.metrics.momentary_lufs = measurements.momentary_lufs;
            self.metrics.short_term_lufs = measurements.short_term_lufs;
            self.metrics.integrated_lufs = measurements.integrated_lufs;
            self.metrics.true_peak_dbtp = measurements.true_peak_dbtp;

            if self.until_analysis == 0 {
                self.until_analysis = (self.rate as usize / 10).max(1);

                if self.active
                    && let Some(level) =
                        measurements.short_term_lufs.or(measurements.momentary_lufs)
                {
                    let error = self.config.target_lufs - level;
                    self.target_db = if error.abs() <= self.config.dead_band_lu {
                        0.0
                    } else {
                        error
                    }
                    .clamp(self.config.max_attenuation_db, self.config.max_gain_db);
                }
            }
        }

        self.metrics.rider_gain_db = self.agc_db;
        let preview_gain = 10.0_f64.powf(
            (self.agc_db + self.config.gain_up_db_per_second * super::LIVE_LATENCY.as_secs_f64())
                / 20.0,
        );
        self.metrics.limiter_gain_reduction_db = self.limiter.process(
            audio,
            future,
            preview_gain,
            self.config.true_peak_ceiling_dbtp,
        );
    }

    fn update_gate(&mut self) {
        let level = 10.0 * self.source_power.max(1e-20).log10();

        if level >= PAUSE_THRESHOLD_DBFS + GATE_HYSTERESIS_DB {
            self.active = true;
            self.quiet_samples = 0;
        } else if level < PAUSE_THRESHOLD_DBFS {
            self.quiet_samples = self.quiet_samples.saturating_add(1);

            if self.quiet_samples >= (PAUSE_HOLD_SECONDS * f64::from(self.rate)) as usize {
                self.active = false;
            }
        } else {
            self.quiet_samples = 0;
        }
    }

    fn update_agc(&mut self) {
        if !self.active {
            if self.quiet_samples >= (PAUSE_RETURN_SECONDS * f64::from(self.rate)) as usize
                && self.agc_db > 0.0
            {
                self.agc_db = (self.agc_db - 1.0 / f64::from(self.rate)).max(0.0);
            }

            return;
        }

        // Freeze upward gain as soon as the signal is quiet, even during pause hold.
        if self.quiet_samples > 0 && self.target_db > self.agc_db {
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

fn finite(sample: f32) -> f32 {
    if sample.is_finite() { sample } else { 0.0 }
}

fn power(sample: [f32; 2]) -> f64 {
    (f64::from(finite(sample[0])).powi(2) + f64::from(finite(sample[1])).powi(2)) * 0.5
}

fn compression(level: f64, threshold: f64) -> f64 {
    let excess = level - threshold;
    let slope = 1.0 - 1.0 / RATIO;

    if excess <= -KNEE_DB / 2.0 {
        0.0
    } else if excess < KNEE_DB / 2.0 {
        slope * (excess + KNEE_DB / 2.0).powi(2) / (2.0 * KNEE_DB)
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
    fn compressor_catches_the_first_loud_samples_and_preserves_earlier_quiet_audio() {
        let rate = RATE as usize;
        let source: Vec<_> = (0..rate * 4)
            .map(|index| tone(index, if index < rate * 2 { 0.01 } else { 0.8 }))
            .collect();
        let config = LiveLoudnessConfig {
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
        let output = process(&source, 1024, LiveLoudnessConfig::default());
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
    fn malformed_source_and_preview_samples_do_not_poison_processing() {
        let source = vec![[f32::NAN, f32::INFINITY]; 1024];
        let output = process(&source, 777, LiveLoudnessConfig::default());

        assert!(output.iter().all(|sample| *sample == [0.0, 0.0]));
    }
}
