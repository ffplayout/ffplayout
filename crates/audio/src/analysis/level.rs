use std::{fmt, sync::Arc};

const AUDIO_LEVEL_INTERVAL_MILLIS: u64 = 250;
const MIN_DBFS: f32 = -100.0;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AudioLevel {
    pub rms_db: f32,
    pub peak_db: f32,
    /// End of the measured interval on the program audio timeline, in microseconds.
    pub timestamp_us: Option<i64>,
}

/// The callback runs synchronously on the audio processing path and must
/// return quickly; hand slow work (I/O, locks held elsewhere) to another thread.
#[derive(Clone)]
pub struct AudioLevelCallback {
    callback: Arc<dyn Fn(AudioLevel) + Send + Sync>,
}

impl AudioLevelCallback {
    pub fn new(callback: impl Fn(AudioLevel) + Send + Sync + 'static) -> Self {
        Self {
            callback: Arc::new(callback),
        }
    }

    fn emit(&self, level: AudioLevel) {
        (self.callback)(level);
    }
}

impl fmt::Debug for AudioLevelCallback {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AudioLevelCallback")
    }
}

pub struct AudioLevelMeter {
    callback: Option<AudioLevelCallback>,
    interval_samples: u64,
    sample_rate: u32,
    timestamp_us: Option<i64>,
    samples: u64,
    sum_squares: f64,
    peak: f32,
}

impl AudioLevelMeter {
    pub fn new(sample_rate: u32, callback: Option<AudioLevelCallback>) -> Self {
        Self {
            callback,
            sample_rate,
            timestamp_us: None,
            interval_samples: (u64::from(sample_rate) * AUDIO_LEVEL_INTERVAL_MILLIS / 1000).max(1),
            samples: 0,
            sum_squares: 0.0,
            peak: 0.0,
        }
    }

    pub fn process_samples(&mut self, left: &[f32], right: &[f32], sample_position: Option<i64>) {
        if self.callback.is_none() || left.is_empty() {
            return;
        }

        // Measure each channel separately: a mono downmix would hide
        // anti-phase content and under-report the peak.
        for (index, (left, right)) in left.iter().zip(right).enumerate() {
            let left = finite_sample(*left);
            let right = finite_sample(*right);

            self.sum_squares += (f64::from(left).powi(2) + f64::from(right).powi(2)) * 0.5;
            self.peak = self.peak.max(left.abs()).max(right.abs());
            self.samples += 1;

            if self.samples >= self.interval_samples {
                self.timestamp_us = sample_position.map(|pts| {
                    let end = pts.saturating_add(index as i64 + 1);
                    ((i128::from(end) * 1_000_000) / i128::from(self.sample_rate.max(1))) as i64
                });
                self.emit();
            }
        }
    }

    fn emit(&mut self) {
        if self.samples == 0 {
            return;
        }

        let rms = (self.sum_squares / self.samples as f64).sqrt() as f32;
        let level = AudioLevel {
            rms_db: amplitude_to_db(rms),
            peak_db: amplitude_to_db(self.peak),
            timestamp_us: self.timestamp_us,
        };

        if let Some(callback) = &self.callback {
            callback.emit(level);
        }

        self.samples = 0;
        self.sum_squares = 0.0;
        self.peak = 0.0;
    }
}

fn finite_sample(sample: f32) -> f32 {
    if sample.is_finite() { sample } else { 0.0 }
}

fn amplitude_to_db(amplitude: f32) -> f32 {
    if amplitude <= 0.0 {
        MIN_DBFS
    } else {
        (20.0 * amplitude.log10()).max(MIN_DBFS)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    fn measure(left: &[f32], right: &[f32]) -> AudioLevel {
        let levels = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&levels);
        let mut meter = AudioLevelMeter::new(
            4_000,
            Some(AudioLevelCallback::new(move |level| {
                sink.lock().unwrap().push(level);
            })),
        );

        meter.process_samples(left, right, Some(0));

        let levels = levels.lock().unwrap();
        assert_eq!(levels.len(), 1);
        levels[0]
    }

    #[test]
    fn anti_phase_signal_is_not_measured_as_silence() {
        let left = vec![0.5; 1_000];
        let right = vec![-0.5; 1_000];
        let level = measure(&left, &right);

        assert!((level.peak_db - 20.0 * 0.5_f32.log10()).abs() < 0.01);
        assert!((level.rms_db - 20.0 * 0.5_f32.log10()).abs() < 0.01);
    }

    #[test]
    fn peak_follows_the_louder_channel() {
        let left = vec![0.25; 1_000];
        let mut right = vec![0.0; 1_000];
        right[10] = -1.0;
        let level = measure(&left, &right);

        assert!(level.peak_db.abs() < 0.01);
    }

    #[test]
    fn mismatched_plane_lengths_measure_the_common_prefix() {
        let level = measure(&vec![0.5; 1_200], &vec![0.5; 1_000]);

        assert!((level.peak_db - 20.0 * 0.5_f32.log10()).abs() < 0.01);
    }
}
