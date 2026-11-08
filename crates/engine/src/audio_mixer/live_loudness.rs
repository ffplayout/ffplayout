use std::ops::{Deref, DerefMut};

use anyhow::Result;
pub use ff_audio::{
    BufferedLoudnessAnalysis, LiveLoudnessConfig, LiveLoudnessControl, LiveLoudnessMeasurement,
    LiveLoudnessMetrics,
};
use ffmpeg_next::frame;

use super::samples::stereo;

/// FFmpeg adapter for the PCM loudness processor.
pub struct LiveLoudnessProcessor(ff_audio::LiveLoudnessProcessor);

impl LiveLoudnessProcessor {
    pub fn new(
        sample_rate: u32,
        config: LiveLoudnessConfig,
    ) -> Result<Self, ff_audio::AnalysisError> {
        ff_audio::LiveLoudnessProcessor::new(sample_rate, config).map(Self)
    }

    pub fn metrics(&self) -> LiveLoudnessMetrics {
        self.0.metrics()
    }

    pub fn process(&mut self, audio: &mut frame::Audio) -> Result<()> {
        self.0.process(&mut stereo(audio)?);

        Ok(())
    }

    pub fn process_with_lookahead(
        &mut self,
        audio: &mut frame::Audio,
        future: &[[f32; 2]],
    ) -> Result<()> {
        self.0.process_with_lookahead(&mut stereo(audio)?, future);

        Ok(())
    }

    pub fn apply_gain_with_lookahead(
        &mut self,
        audio: &mut frame::Audio,
        future: &[[f32; 2]],
    ) -> Result<()> {
        self.0
            .apply_gain_with_lookahead(&mut stereo(audio)?, future);

        Ok(())
    }

    pub fn analyze(&mut self, audio: &mut frame::Audio) -> Result<()> {
        self.0.analyze(&mut stereo(audio)?);

        Ok(())
    }

    pub fn analyze_buffered(
        &mut self,
        audio: &mut frame::Audio,
    ) -> Result<BufferedLoudnessAnalysis> {
        Ok(self.0.analyze_buffered(&mut stereo(audio)?))
    }

    pub fn apply_buffered_gain(
        &mut self,
        audio: &mut frame::Audio,
        analysis: &BufferedLoudnessAnalysis,
        future: &[[f32; 2]],
    ) -> Result<()> {
        self.0
            .apply_buffered_gain(&mut stereo(audio)?, analysis, future);

        Ok(())
    }

    pub fn apply_gain(&mut self, audio: &mut frame::Audio) -> Result<()> {
        self.0.apply_gain(&mut stereo(audio)?);

        Ok(())
    }
}

impl Deref for LiveLoudnessProcessor {
    type Target = ff_audio::LiveLoudnessProcessor;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for LiveLoudnessProcessor {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
