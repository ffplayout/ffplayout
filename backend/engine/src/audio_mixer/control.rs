use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

use anyhow::{Result, anyhow};
use ffmpeg_next::frame;

use super::volume::GainEffect;

#[derive(Debug, Clone)]
pub struct AudioEffectsControl {
    volume: Arc<AtomicU32>,
}

impl AudioEffectsControl {
    pub fn new(volume: f64) -> Result<Self> {
        validate_volume(volume)?;
        Ok(Self {
            volume: Arc::new(AtomicU32::new((volume as f32).to_bits())),
        })
    }

    pub fn set_volume(&self, volume: f64) -> Result<()> {
        validate_volume(volume)?;
        self.volume
            .store((volume as f32).to_bits(), Ordering::Relaxed);
        Ok(())
    }

    pub fn volume(&self) -> f64 {
        f64::from(f32::from_bits(self.volume.load(Ordering::Relaxed)))
    }

    pub fn volume_f32(&self) -> f32 {
        f32::from_bits(self.volume.load(Ordering::Relaxed))
    }
}

impl Default for AudioEffectsControl {
    fn default() -> Self {
        Self::new(1.0).expect("default audio volume must be valid")
    }
}

fn validate_volume(volume: f64) -> Result<()> {
    if !volume.is_finite() || !(0.0..=3.0).contains(&volume) {
        return Err(anyhow!("audio volume must be between 0.0 and 3.0"));
    }
    Ok(())
}

pub(crate) trait AudioEffect: Send {
    fn process(&mut self, frame: &mut frame::Audio);
}

pub(crate) struct AudioEffectChain {
    effects: Vec<Box<dyn AudioEffect>>,
}

impl AudioEffectChain {
    pub(crate) fn new(control: AudioEffectsControl, sample_rate: u32) -> Self {
        let mut chain = Self {
            effects: Vec::new(),
        };
        chain.add(GainEffect::new(control, sample_rate));
        chain
    }

    pub(crate) fn add(&mut self, effect: impl AudioEffect + 'static) {
        self.effects.push(Box::new(effect));
    }

    pub(crate) fn process(&mut self, frame: &mut frame::Audio) {
        for effect in &mut self.effects {
            effect.process(frame);
        }
    }
}
