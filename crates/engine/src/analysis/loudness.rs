pub use ff_audio::{LoudnessMeterControl, LoudnessMetrics};
use ffmpeg_next::frame;

use crate::audio_mixer::samples::stereo_planes;

#[cfg(any(test, feature = "desktop-base"))]
pub struct LoudnessAnalyzer(ff_audio::LoudnessAnalyzer);
#[cfg(any(test, feature = "desktop-base"))]
impl LoudnessAnalyzer {
    #[cfg(test)]
    pub fn new(rate: u32) -> Result<Self, ff_audio::AnalysisError> {
        ff_audio::LoudnessAnalyzer::new(rate).map(Self)
    }

    #[cfg(feature = "desktop-base")]
    pub(crate) fn new_display_meter(rate: u32) -> Result<Self, ff_audio::AnalysisError> {
        ff_audio::LoudnessAnalyzer::new_display_meter(rate).map(Self)
    }

    #[cfg(test)]
    pub(crate) fn process_samples(&mut self, left: &[f32], right: &[f32]) -> LoudnessMetrics {
        self.0.process_samples(left, right)
    }

    #[cfg(feature = "desktop-base")]
    pub fn process_frame(&mut self, frame: &frame::Audio) -> LoudnessMetrics {
        let (left, right) = stereo_planes(frame).unwrap_or((&[], &[]));

        self.0.process_samples(left, right)
    }
}

pub(crate) struct LoudnessMeter(ff_audio::LoudnessMeter);

impl LoudnessMeter {
    pub(crate) fn new(rate: u32, control: LoudnessMeterControl) -> Self {
        Self(ff_audio::LoudnessMeter::new(rate, control))
    }

    pub(crate) fn process_frame(&mut self, frame: &frame::Audio) {
        let (left, right) = stereo_planes(frame).unwrap_or((&[], &[]));

        self.0.process_samples(left, right);
    }
}
