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

        if left.len() != right.len() {
            return;
        }

        for (index, (left, right)) in left.iter().zip(right).take(left.len()).enumerate() {
            let left = finite_sample(*left);
            let right = finite_sample(*right);
            let mono = (left + right) * 0.5;
            let abs = mono.abs();

            self.sum_squares += f64::from(mono * mono);
            self.peak = self.peak.max(abs);
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
