use anyhow::Result;
pub use ff_audio::AudioEffectsControl;
use ffmpeg_next::frame;

use super::samples::stereo;

pub(crate) trait AudioEffect: Send {
    fn process(&mut self, frame: &mut frame::Audio) -> Result<()>;
}

pub(crate) struct AudioEffectChain(ff_audio::AudioEffectChain);

impl AudioEffectChain {
    pub(crate) fn new(control: AudioEffectsControl, rate: u32) -> Self {
        Self(ff_audio::AudioEffectChain::new(control, rate))
    }

    pub(crate) fn process(&mut self, frame: &mut frame::Audio) -> Result<()> {
        self.0.process(&mut stereo(frame)?);

        Ok(())
    }
}
