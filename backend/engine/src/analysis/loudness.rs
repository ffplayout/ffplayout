use std::sync::{
    Arc, PoisonError, RwLock,
    atomic::{AtomicUsize, Ordering},
};

use ebur128_stream::{Analyzer, AnalyzerBuilder, Channel, Mode};
use ffmpeg_next::frame;

use super::integrated_loudness::IntegratedLoudness;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LoudnessMetrics {
    pub momentary_lufs: Option<f64>,
    pub short_term_lufs: Option<f64>,
    pub integrated_lufs: Option<f64>,
    pub true_peak_dbtp: Option<f64>,
}

/// Streaming loudness measurements with bounded programme history.
/// Integrated loudness uses an adaptive histogram and is unavailable when
/// gate uncertainty exceeds 0.02 LU. The remaining measurements come directly
/// from the EBU R128 analyzer.
pub struct LoudnessAnalyzer {
    analyzer: Analyzer,
    integrated: IntegratedLoudness,
    samples_per_block: usize,
    samples_until_block: usize,
    metrics: LoudnessMetrics,
}

impl LoudnessAnalyzer {
    pub fn new(sample_rate: u32) -> Result<Self, ebur128_stream::Error> {
        Self::with_modes(
            sample_rate,
            Mode::Momentary | Mode::ShortTerm | Mode::TruePeak,
        )
    }

    /// Display meters report sample peaks separately and do not need 4x true-peak analysis.
    #[cfg(feature = "desktop-base")]
    pub(crate) fn new_display_meter(sample_rate: u32) -> Result<Self, ebur128_stream::Error> {
        Self::with_modes(sample_rate, Mode::Momentary | Mode::ShortTerm)
    }

    fn with_modes(sample_rate: u32, modes: Mode) -> Result<Self, ebur128_stream::Error> {
        Ok(Self {
            analyzer: AnalyzerBuilder::new()
                .sample_rate(sample_rate)
                .channels(&[Channel::Left, Channel::Right])
                .modes(modes)
                .build()?,
            integrated: IntegratedLoudness::new(),
            samples_per_block: sample_rate as usize / 10,
            samples_until_block: sample_rate as usize / 10,
            metrics: LoudnessMetrics::default(),
        })
    }

    pub fn process_frame(&mut self, frame: &frame::Audio) -> LoudnessMetrics {
        if frame.planes() != 2 || frame.samples() == 0 {
            return self.metrics;
        }

        self.process_samples(frame.plane::<f32>(0), frame.plane::<f32>(1))
    }

    pub(crate) fn process_samples(&mut self, left: &[f32], right: &[f32]) -> LoudnessMetrics {
        if left.is_empty() || left.len() != right.len() {
            return self.metrics;
        }

        // Reject the entire frame before pushing chunks: otherwise an invalid
        // sample could leave our block cadence out of sync with the analyzer.
        if left.iter().chain(right).any(|sample| !sample.is_finite()) {
            return self.metrics;
        }

        let mut offset = 0;
        let mut completed_block = false;

        while offset < left.len() {
            let samples = self.samples_until_block.min(left.len() - offset);
            let end = offset + samples;

            if self
                .analyzer
                .push_planar::<f32>(&[&left[offset..end], &right[offset..end]])
                .is_err()
            {
                return self.metrics;
            }

            self.samples_until_block -= samples;
            offset = end;

            if self.samples_until_block == 0 {
                // Capture every overlapping 400 ms gating block at the
                // library's 100 ms cadence, independent of input frame size.
                if let Some(momentary) = self.analyzer.snapshot().momentary_lufs() {
                    self.integrated.record(momentary);
                }

                self.samples_until_block = self.samples_per_block;
                completed_block = true;
            }
        }

        let snapshot = self.analyzer.snapshot();
        self.metrics = LoudnessMetrics {
            momentary_lufs: snapshot.momentary_lufs(),
            short_term_lufs: snapshot.short_term_lufs(),
            integrated_lufs: if completed_block {
                self.integrated.integrated_lufs()
            } else {
                self.metrics.integrated_lufs
            },
            true_peak_dbtp: snapshot.true_peak_dbtp(),
        };
        self.metrics
    }
}

#[derive(Debug, Clone, Default)]
pub struct LoudnessMeterControl {
    subscribers: Arc<AtomicUsize>,
    metrics: Arc<RwLock<Option<LoudnessMetrics>>>,
}

impl LoudnessMeterControl {
    pub fn subscribe(&self) {
        self.subscribers.fetch_add(1, Ordering::Relaxed);
    }

    pub fn unsubscribe(&self) {
        let _ = self
            .subscribers
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_sub(1));
    }

    pub fn active(&self) -> bool {
        self.subscribers.load(Ordering::Relaxed) > 0
    }

    pub fn metrics(&self) -> Option<LoudnessMetrics> {
        self.active()
            .then(|| *self.metrics.read().unwrap_or_else(PoisonError::into_inner))
            .flatten()
    }

    fn set_metrics(&self, metrics: LoudnessMetrics) {
        *self.metrics.write().unwrap_or_else(PoisonError::into_inner) = Some(metrics);
    }
}

pub(crate) struct LoudnessMeter {
    control: LoudnessMeterControl,
    analyzer: Option<LoudnessAnalyzer>,
    sample_rate: u32,
}

impl LoudnessMeter {
    pub(crate) fn new(sample_rate: u32, control: LoudnessMeterControl) -> Self {
        Self {
            control,
            analyzer: None,
            sample_rate,
        }
    }

    pub(crate) fn process_frame(&mut self, frame: &frame::Audio) {
        if !self.control.active() {
            self.analyzer = None;

            return;
        }

        if self.analyzer.is_none() {
            self.analyzer = LoudnessAnalyzer::new(self.sample_rate).ok();
        }

        if let Some(analyzer) = &mut self.analyzer {
            self.control.set_metrics(analyzer.process_frame(frame));
        }
    }
}

#[cfg(test)]
mod tests {
    use ffmpeg_next::{
        format::sample::{Sample, Type},
        util::channel_layout::ChannelLayout,
    };

    use super::*;

    fn audio_frame(left: &[f32], right: &[f32], sample_rate: u32) -> frame::Audio {
        let mut frame =
            frame::Audio::new(Sample::F32(Type::Planar), left.len(), ChannelLayout::STEREO);
        frame.set_rate(sample_rate);
        frame.plane_mut::<f32>(0).copy_from_slice(left);
        frame.plane_mut::<f32>(1).copy_from_slice(right);

        frame
    }

    fn assert_close(actual: Option<f64>, expected: Option<f64>, tolerance: f64) {
        match (actual, expected) {
            (Some(actual), Some(expected)) => assert!(
                (actual - expected).abs() <= tolerance,
                "actual {actual}, expected {expected}, tolerance {tolerance}"
            ),
            (actual, expected) => assert_eq!(actual, expected),
        }
    }

    #[test]
    fn matches_reference_across_sample_rates_levels_and_frame_sizes() {
        for sample_rate in [22_050, 44_100, 48_000] {
            let mut bounded = LoudnessAnalyzer::new(sample_rate).unwrap();
            let mut reference = AnalyzerBuilder::new()
                .sample_rate(sample_rate)
                .channels(&[Channel::Left, Channel::Right])
                .modes(Mode::Momentary | Mode::ShortTerm | Mode::Integrated | Mode::TruePeak)
                .build()
                .unwrap();
            let levels = [0.0, 0.001, 0.1, 0.03, 0.0001, 0.4, 0.0, 0.2];
            let left: Vec<_> = (0..sample_rate as usize * levels.len())
                .map(|sample| {
                    let phase = std::f64::consts::TAU * 997.0 * sample as f64 / sample_rate as f64;
                    (phase.sin() * levels[sample / sample_rate as usize]) as f32
                })
                .collect();
            let right: Vec<_> = left.iter().map(|sample| sample * 0.7).collect();
            let chunk_sizes = [1, 1_024, 7_777, sample_rate as usize, 2_003];
            let mut offset = 0;
            let mut chunk = 0;

            while offset < left.len() {
                let end = (offset + chunk_sizes[chunk % chunk_sizes.len()]).min(left.len());
                let left = &left[offset..end];
                let right = &right[offset..end];
                let actual = bounded.process_frame(&audio_frame(left, right, sample_rate));
                reference.push_planar(&[left, right]).unwrap();
                let expected = reference.snapshot();

                assert_close(actual.momentary_lufs, expected.momentary_lufs(), 1e-10);
                assert_close(actual.short_term_lufs, expected.short_term_lufs(), 1e-10);
                assert_close(actual.true_peak_dbtp, expected.true_peak_dbtp(), 1e-10);
                assert_close(actual.integrated_lufs, expected.integrated_lufs(), 0.02);
                offset = end;
                chunk += 1;
            }
        }
    }

    #[test]
    fn integrated_measurement_waits_for_a_full_400_ms_block() {
        let mut analyzer = LoudnessAnalyzer::new(48_000).unwrap();
        let samples = vec![0.1; 19_199];
        let metrics = analyzer.process_frame(&audio_frame(&samples, &samples, 48_000));
        assert_eq!(metrics.integrated_lufs, None);
        let metrics = analyzer.process_frame(&audio_frame(&[0.1], &[0.1], 48_000));
        assert!(metrics.integrated_lufs.is_some());
    }

    #[test]
    fn invalid_frame_does_not_shift_block_cadence() {
        let mut analyzer = LoudnessAnalyzer::new(48_000).unwrap();
        let mut reference = LoudnessAnalyzer::new(48_000).unwrap();
        let mut invalid = vec![0.1; 9_600];
        invalid[5_000] = f32::NAN;
        assert_eq!(
            analyzer.process_frame(&audio_frame(&invalid, &invalid, 48_000)),
            LoudnessMetrics::default()
        );
        let samples = vec![0.1; 19_200];
        let frame = audio_frame(&samples, &samples, 48_000);

        assert_eq!(
            analyzer.process_frame(&frame),
            reference.process_frame(&frame)
        );
    }
}
