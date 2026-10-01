use std::time::Duration;

use ffmpeg_next::frame;

/// Shared live A/V buffering and limiter lookahead. Rebuild after changing it.
pub(crate) const LIVE_LATENCY: Duration = Duration::from_millis(10);
pub(crate) const TRUE_PEAK_FUTURE_SAMPLES: usize = 6;
const HISTORY_SAMPLES: usize = 5;
const RELEASE_SECONDS: f64 = 0.05;
// Leave a little room for interpolation of the changing gain envelope.
const PEAK_HEADROOM_DB: f64 = 0.2;

// ITU-R BS.1770 Annex 2, four polyphase filters with twelve taps each.
// https://www.itu.int/rec/R-REC-BS.1770
const COEFFS: [[f64; 12]; 4] = [
    // phase 0
    [
        0.001_708_984_4,
        0.010_986_328_0,
        -0.019_653_320_0,
        0.033_203_125_0,
        -0.059_448_242_0,
        0.137_329_100_0,
        0.972_167_970_0,
        -0.102_294_920_0,
        0.047_607_422_0,
        -0.026_611_328_0,
        0.014_892_578_0,
        -0.008_300_781_3,
    ],
    // phase 1
    [
        -0.029_174_805_0,
        0.029_296_875_0,
        -0.051_757_812_0,
        0.089_111_328_0,
        -0.166_503_910_0,
        0.465_087_890_0,
        0.779_785_160_0,
        -0.200_317_380_0,
        0.101_562_500_0,
        -0.058_227_540_0,
        0.033_081_055_0,
        -0.018_920_898_0,
    ],
    // phase 2 (= phase 1 reversed by symmetry)
    [
        -0.018_920_898_0,
        0.033_081_055_0,
        -0.058_227_540_0,
        0.101_562_500_0,
        -0.200_317_380_0,
        0.779_785_160_0,
        0.465_087_890_0,
        -0.166_503_910_0,
        0.089_111_328_0,
        -0.051_757_812_0,
        0.029_296_875_0,
        -0.029_174_805_0,
    ],
    // phase 3 (= phase 0 reversed by symmetry)
    [
        -0.008_300_781_3,
        0.014_892_578_0,
        -0.026_611_328_0,
        0.047_607_422_0,
        -0.102_294_920_0,
        0.972_167_970_0,
        0.137_329_100_0,
        -0.059_448_242_0,
        0.033_203_125_0,
        -0.019_653_320_0,
        0.010_986_328_0,
        0.001_708_984_4,
    ],
];

pub(super) struct LookaheadLimiter {
    history: [[f64; 2]; HISTORY_SAMPLES],
    gain: f64,
    release: f64,
    lookahead: usize,
}

impl LookaheadLimiter {
    pub(super) fn new(sample_rate: u32) -> Self {
        Self {
            history: [[0.0; 2]; HISTORY_SAMPLES],
            gain: 1.0,
            release: 1.0 - (-1.0 / (RELEASE_SECONDS * f64::from(sample_rate))).exp(),
            lookahead: lookahead_samples(sample_rate),
        }
    }

    /// Future samples are a preview only: no samples are inserted, removed or
    /// shifted. The live event buffer holds the frames until preview is ready.
    pub(super) fn process(
        &mut self,
        frame: &mut frame::Audio,
        future: &[[f32; 2]],
        future_gain: f64,
        ceiling_db: f64,
    ) -> f64 {
        let samples = frame.samples();
        let preview_len = self.lookahead + TRUE_PEAK_FUTURE_SAMPLES;
        let mut signal = Vec::with_capacity(HISTORY_SAMPLES + samples + preview_len);
        signal.extend_from_slice(&self.history);

        for index in 0..samples {
            signal.push([
                f64::from(frame.plane::<f32>(0)[index]),
                f64::from(frame.plane::<f32>(1)[index]),
            ]);
        }

        for sample in future.iter().take(preview_len) {
            signal.push(sample.map(|value| {
                if value.is_finite() {
                    f64::from(value) * future_gain
                } else {
                    0.0
                }
            }));
        }

        // At end of stream, gaps and forced dropout recovery, absent future
        // audio is silence. History still bridges the previous packet boundary.
        signal.resize(HISTORY_SAMPLES + samples + preview_len, [0.0; 2]);
        self.history
            .copy_from_slice(&signal[samples..samples + HISTORY_SAMPLES]);
        let mut peaks = Vec::with_capacity(samples + self.lookahead);

        for index in 0..samples + self.lookahead {
            let center = HISTORY_SAMPLES + index;
            let mut peak = signal[center][0].abs().max(signal[center][1].abs());

            for coefficients in COEFFS {
                let mut reconstructed = [0.0_f64; 2];

                for (tap, coefficient) in coefficients.iter().enumerate() {
                    for (channel, value) in reconstructed.iter_mut().enumerate() {
                        *value +=
                            coefficient * signal[center + TRUE_PEAK_FUTURE_SAMPLES - tap][channel];
                    }
                }

                peak = peak.max(reconstructed[0].abs()).max(reconstructed[1].abs());
            }

            peaks.push(peak);
        }

        let ceiling = 10.0_f64.powf((ceiling_db - PEAK_HEADROOM_DB) / 20.0);
        // Propagate every peak's gain requirement backwards. A later, larger
        // peak must not postpone the attenuation needed by an earlier peak.
        // Reserve the interpolation support so gain has already settled for
        // samples contributing to a true peak. The full attack still fits in
        // the preview window and is independent of packet boundaries.
        let attack_samples = self
            .lookahead
            .saturating_sub(TRUE_PEAK_FUTURE_SAMPLES)
            .max(1);
        let attack_step = 1.0 / attack_samples as f64;
        let mut envelope: Vec<f64> = peaks
            .iter()
            .map(|peak| if *peak > ceiling { ceiling / peak } else { 1.0 })
            .collect();

        for index in (0..envelope.len().saturating_sub(1)).rev() {
            envelope[index] = envelope[index].min(envelope[index + 1] + attack_step);
        }

        let mut minimum_gain: f64 = 1.0;

        for index in 0..samples {
            let support_end = (index + TRUE_PEAK_FUTURE_SAMPLES + 1).min(envelope.len());
            let required = envelope[index..support_end]
                .iter()
                .copied()
                .fold(1.0_f64, f64::min);
            // Recover in dB: an additive step towards unity would produce
            // a large relative jump after strong attenuation, creating new
            // intersample peaks between adjacent output samples.
            let released = self.gain.max(f64::MIN_POSITIVE).powf(1.0 - self.release);
            self.gain = released.min(required);

            let sample_peak = signal[HISTORY_SAMPLES + index][0]
                .abs()
                .max(signal[HISTORY_SAMPLES + index][1].abs());

            if sample_peak * self.gain > ceiling {
                self.gain = ceiling / sample_peak;
            }

            minimum_gain = minimum_gain.min(self.gain);

            for (channel, sample) in signal[HISTORY_SAMPLES + index].iter().enumerate() {
                frame.plane_mut::<f32>(channel)[index] = (sample * self.gain) as f32;
            }
        }

        -20.0 * minimum_gain.log10()
    }
}

pub(crate) fn lookahead_samples(sample_rate: u32) -> usize {
    (LIVE_LATENCY.as_secs_f64() * f64::from(sample_rate)).ceil() as usize
}

#[cfg(test)]
mod tests {
    use ffmpeg_next::{
        format::sample::{Sample, Type},
        util::channel_layout::ChannelLayout,
    };

    use crate::analysis::loudness::LoudnessAnalyzer;

    use super::*;

    fn process_packets(signal: &[[f32; 2]], sizes: &[usize]) -> Vec<[f32; 2]> {
        let mut limiter = LookaheadLimiter::new(48_000);
        let mut output = Vec::new();
        let mut offset = 0;
        let mut packet = 0;

        while offset < signal.len() {
            let end = (offset + sizes[packet % sizes.len()]).min(signal.len());
            let mut frame = frame::Audio::new(
                Sample::F32(Type::Planar),
                end - offset,
                ChannelLayout::STEREO,
            );
            frame.set_rate(48_000);
            frame.set_pts(Some(offset as i64));

            for channel in 0..2 {
                for (sample, input) in frame
                    .plane_mut::<f32>(channel)
                    .iter_mut()
                    .zip(&signal[offset..end])
                {
                    *sample = input[channel];
                }
            }

            limiter.process(&mut frame, &signal[end..], 1.0, -1.0);
            assert_eq!(frame.pts(), Some(offset as i64));
            assert_eq!(frame.samples(), end - offset);

            for index in 0..frame.samples() {
                output.push([frame.plane::<f32>(0)[index], frame.plane::<f32>(1)[index]]);
            }

            offset = end;
            packet += 1;
        }

        output
    }

    #[test]
    fn anticipates_a_peak_in_the_next_packet_without_shifting_audio() {
        let mut input = vec![[0.1, -0.025]; 5_000];
        input[960] = [2.0, -0.5];
        let output = process_packets(&input, &[960]);

        assert_eq!(output.len(), input.len());
        assert_eq!(output[0][0], 0.1);
        assert!(
            output[800][0] < 0.09,
            "gain must fall before the future peak"
        );
        assert!(output[950][0] < output[800][0]);
        assert!(output[960][0] <= 10.0_f32.powf(-1.0 / 20.0));
        assert!(output[4_999][0] > output[970][0]);

        for sample in output {
            assert!((sample[1] + sample[0] * 0.25).abs() < 1e-7);
        }
    }

    #[test]
    fn packet_boundaries_do_not_change_the_limiter_envelope() {
        let input: Vec<_> = (0..12_000)
            .map(|index| {
                let amplitude = if index % 1_021 == 0 { 2.0 } else { 0.2 };
                [amplitude, -amplitude * 0.3]
            })
            .collect();
        let whole = process_packets(&input, &[input.len()]);
        let packets = process_packets(&input, &[1, 333, 1_024, 777]);

        assert_eq!(whole, packets);
    }

    #[test]
    fn later_larger_peaks_do_not_mask_earlier_intersample_peaks() {
        for (amplitude, later_peak) in [(1.8, 2.0), (100.0, 100.0), (100.0, 10.0)] {
            for distance in [1, 2, 10, 50, 100, 200, 400] {
                let mut input = vec![[0.0; 2]; 3_000];

                for (index, sample) in input[1_000..1_016].iter_mut().enumerate() {
                    let phase = std::f64::consts::TAU * 12_000.0 * index as f64 / 48_000.0
                        + std::f64::consts::FRAC_PI_4;
                    *sample = [(amplitude * phase.sin()) as f32; 2];
                }

                input[1_000 + distance] = [later_peak; 2];
                let output = process_packets(&input, &[1_024]);
                let left: Vec<_> = output.iter().map(|sample| sample[0]).collect();
                let mut analyzer = LoudnessAnalyzer::new(48_000).unwrap();
                let peak = analyzer
                    .process_samples(&left, &left)
                    .true_peak_dbtp
                    .unwrap();

                assert!(
                    peak <= -1.0,
                    "amplitude {amplitude}, later {later_peak}, distance {distance}: {peak} dBTP exceeds ceiling"
                );
            }
        }
    }

    #[test]
    fn limits_intersample_peaks_measured_by_an_independent_analyzer() {
        for frequency in [997.0, 12_000.0, 19_000.0] {
            let input: Vec<_> = (0..24_000)
                .map(|index| {
                    let phase = std::f64::consts::TAU * frequency * index as f64 / 48_000.0
                        + std::f64::consts::FRAC_PI_4;
                    let amplitude = if index < 4_000 { 0.2 } else { 1.2 };
                    let sample = (phase.sin() * amplitude) as f32;
                    [sample, sample * -0.4]
                })
                .collect();
            let output = process_packets(&input, &[1_024]);
            let left: Vec<_> = output.iter().map(|sample| sample[0]).collect();
            let right: Vec<_> = output.iter().map(|sample| sample[1]).collect();
            let mut analyzer = LoudnessAnalyzer::new(48_000).unwrap();
            let metrics = analyzer.process_samples(&left, &right);

            assert!(
                metrics.true_peak_dbtp.unwrap() <= -1.0,
                "{frequency} Hz: {:?}",
                metrics.true_peak_dbtp
            );
        }
    }
}
