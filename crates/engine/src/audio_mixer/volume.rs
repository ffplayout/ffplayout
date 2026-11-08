use anyhow::Result;
use ffmpeg_next::frame;

use super::{AudioEffect, AudioEffectsControl, samples::stereo};

pub(crate) struct GainEffect(pub(super) ff_audio::volume::GainEffect);

impl GainEffect {
    pub(crate) fn new(control: AudioEffectsControl, rate: u32) -> Self {
        Self(ff_audio::volume::GainEffect::new(control, rate))
    }
}

impl AudioEffect for GainEffect {
    fn process(&mut self, frame: &mut frame::Audio) -> Result<()> {
        ff_audio::AudioEffect::process(&mut self.0, &mut stereo(frame)?);

        Ok(())
    }
}
