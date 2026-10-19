use std::{fmt, sync::Arc};

use ffmpeg_next::frame;

const AUDIO_LEVEL_INTERVAL_MILLIS: u64 = 250;
const MIN_DBFS: f32 = -100.0;

/// Lightweight access to decoded, normalized audio frames. The callback runs
/// synchronously on the decoder thread and must return quickly.
#[derive(Clone)]
pub struct AudioFrameCallback {
    callback: Arc<dyn Fn(&frame::Audio) + Send + Sync>,
}

impl AudioFrameCallback {
    pub fn new(callback: impl Fn(&frame::Audio) + Send + Sync + 'static) -> Self {
        Self {
            callback: Arc::new(callback),
        }
    }

    pub(crate) fn emit(&self, frame: &frame::Audio) {
        (self.callback)(frame);
    }
}

impl fmt::Debug for AudioFrameCallback {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AudioFrameCallback")
    }
}

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

pub(crate) struct AudioLevelMeter {
    callback: Option<AudioLevelCallback>,
    interval_samples: u64,
    sample_rate: u32,
    timestamp_us: Option<i64>,
    samples: u64,
    sum_squares: f64,
    peak: f32,
}

impl AudioLevelMeter {
    pub(crate) fn new(sample_rate: u32, callback: Option<AudioLevelCallback>) -> Self {
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

    pub(crate) fn process_frame(&mut self, frame: &frame::Audio) {
        if self.callback.is_none() || frame.samples() == 0 {
            return;
        }

        let left = frame.plane::<f32>(0);
        let right = frame.plane::<f32>(1);

        for (index, (left, right)) in left.iter().zip(right).take(frame.samples()).enumerate() {
            let left = finite_sample(*left);
            let right = finite_sample(*right);
            let mono = (left + right) * 0.5;
            let abs = mono.abs();

            self.sum_squares += f64::from(mono * mono);
            self.peak = self.peak.max(abs);
            self.samples += 1;

            if self.samples >= self.interval_samples {
                self.timestamp_us = frame.pts().map(|pts| {
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

    use ffmpeg_next::{
        ChannelLayout,
        format::{Sample, sample::Type},
        frame,
    };

    use super::{AudioLevelCallback, AudioLevelMeter, amplitude_to_db};

    #[test]
    fn levels_are_measured_every_250_ms_of_audio() {
        let levels = Arc::new(Mutex::new(Vec::new()));
        let captured = levels.clone();
        let callback = AudioLevelCallback::new(move |level| captured.lock().unwrap().push(level));
        let mut meter = AudioLevelMeter::new(48_000, Some(callback));
        let mut audio = frame::Audio::new(Sample::F32(Type::Planar), 480, ChannelLayout::STEREO);
        audio.plane_mut::<f32>(0).fill(0.5);
        audio.plane_mut::<f32>(1).fill(0.5);
        for index in 0..24 {
            audio.set_pts(Some(48_000 + index * 480));
            meter.process_frame(&audio);
        }
        assert!(levels.lock().unwrap().is_empty());
        audio.set_pts(Some(48_000 + 24 * 480));
        meter.process_frame(&audio);
        assert_eq!(levels.lock().unwrap().len(), 1);
        for index in 25..50 {
            audio.set_pts(Some(48_000 + index * 480));
            meter.process_frame(&audio);
        }
        let levels = levels.lock().unwrap();
        assert_eq!(levels.len(), 2);
        assert_eq!(levels[0].timestamp_us, Some(1_250_000));
        assert_eq!(levels[1].timestamp_us, Some(1_500_000));
        assert!((levels[1].peak_db + 6.0206).abs() < 0.001);
    }

    #[test]
    fn amplitude_to_db_handles_silence() {
        assert_eq!(amplitude_to_db(0.0), -100.0);
    }

    #[test]
    fn amplitude_to_db_handles_full_scale() {
        assert!((amplitude_to_db(1.0) - 0.0).abs() < f32::EPSILON);
    }
}
