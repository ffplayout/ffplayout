use std::ops::{Deref, DerefMut};

use anyhow::Result;
use ffmpeg_next::frame;

use super::{LiveLoudnessConfig, samples::stereo, volume::GainEffect};

/// FFmpeg adapter; processing and persistent state live in ff-audio.
pub struct LiveDynamicsProcessor(ff_audio::LiveDynamicsProcessor);

impl LiveDynamicsProcessor {
    pub fn new(rate: u32, config: LiveLoudnessConfig) -> Result<Self, ff_audio::AnalysisError> {
        ff_audio::LiveDynamicsProcessor::new(rate, config).map(Self)
    }

    pub fn metrics(&self) -> super::LiveLoudnessMetrics {
        self.0.metrics()
    }

    pub fn lookahead_samples(&self) -> usize {
        self.0.lookahead_samples()
    }

    #[cfg(feature = "desktop-base")]
    pub(crate) fn compressor_gain_reduction_db(&self) -> f64 {
        self.0.compressor_gain_reduction_db()
    }

    #[cfg(feature = "desktop-base")]
    pub(crate) fn pause_status(&self) -> &'static str {
        self.0.pause_status()
    }

    pub fn process(&mut self, audio: &mut frame::Audio, future: &[[f32; 2]]) -> Result<()> {
        self.0.process(&mut stereo(audio)?, future);

        Ok(())
    }

    pub(crate) fn process_with_output_gain(
        &mut self,
        audio: &mut frame::Audio,
        future: &[[f32; 2]],
        hold: bool,
        gain: Option<&mut GainEffect>,
    ) -> Result<()> {
        self.0.process_with_output_gain(
            &mut stereo(audio)?,
            future,
            hold,
            gain.map(|gain| &mut gain.0),
        );

        Ok(())
    }
}

impl Deref for LiveDynamicsProcessor {
    type Target = ff_audio::LiveDynamicsProcessor;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for LiveDynamicsProcessor {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
