use std::{fmt, sync::Arc};

use ffmpeg_next::frame;

use crate::audio_mixer::samples::stereo_planes;

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

pub use ff_audio::{AudioLevel, AudioLevelCallback};

pub(crate) struct AudioLevelMeter(ff_audio::AudioLevelMeter);

impl AudioLevelMeter {
    pub(crate) fn new(rate: u32, callback: Option<AudioLevelCallback>) -> Self {
        Self(ff_audio::AudioLevelMeter::new(rate, callback))
    }

    pub(crate) fn process_frame(&mut self, frame: &frame::Audio) {
        if let Some((left, right)) = stereo_planes(frame) {
            self.0.process_samples(left, right, frame.pts());
        }
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

    use super::{AudioLevelCallback, AudioLevelMeter};

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
}
