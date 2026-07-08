use std::{fmt, sync::Arc};

use ffmpeg_next::frame;

const AUDIO_LEVEL_INTERVAL_MILLIS: u64 = 500;
const MIN_DBFS: f32 = -100.0;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AudioLevel {
    pub rms_db: f32,
    pub peak_db: f32,
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

pub(crate) struct AudioLevelMeter {
    callback: Option<AudioLevelCallback>,
    interval_samples: u64,
    samples: u64,
    sum_squares: f64,
    peak: f32,
}

impl AudioLevelMeter {
    pub(crate) fn new(sample_rate: u32, callback: Option<AudioLevelCallback>) -> Self {
        Self {
            callback,
            interval_samples: (u64::from(sample_rate) * AUDIO_LEVEL_INTERVAL_MILLIS / 1000).max(1),
            samples: 0,
            sum_squares: 0.0,
            peak: 0.0,
        }
    }

    pub(crate) fn process_frame(&mut self, frame: &frame::Audio) {
        if self.callback.is_none() || frame.samples() == 0 {
            return;
        }

        let left = frame.plane::<f32>(0);
        let right = frame.plane::<f32>(1);

        for (left, right) in left.iter().zip(right).take(frame.samples()) {
            let left = finite_sample(*left);
            let right = finite_sample(*right);
            let mono = (left + right) * 0.5;
            let abs = mono.abs();

            self.sum_squares += f64::from(mono * mono);
            self.peak = self.peak.max(abs);
            self.samples += 1;

            if self.samples >= self.interval_samples {
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
    use super::amplitude_to_db;

    #[test]
    fn amplitude_to_db_handles_silence() {
        assert_eq!(amplitude_to_db(0.0), -100.0);
    }

    #[test]
    fn amplitude_to_db_handles_full_scale() {
        assert!((amplitude_to_db(1.0) - 0.0).abs() < f32::EPSILON);
    }
}
