use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

use anyhow::{Result, anyhow};
use ffmpeg_next::frame;

const GAIN_RAMP_MILLISECONDS: u32 = 20;

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

    fn volume_f32(&self) -> f32 {
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

struct GainEffect {
    control: AudioEffectsControl,
    current: f32,
    target: f32,
    ramp_samples: usize,
    remaining_samples: usize,
}

impl GainEffect {
    fn new(control: AudioEffectsControl, sample_rate: u32) -> Self {
        let volume = control.volume_f32();
        Self {
            control,
            current: volume,
            target: volume,
            ramp_samples: ((sample_rate as u64 * u64::from(GAIN_RAMP_MILLISECONDS)) / 1_000).max(1)
                as usize,
            remaining_samples: 0,
        }
    }

    fn next_gain(&mut self) -> f32 {
        let requested = self.control.volume_f32();
        if requested != self.target {
            self.target = requested;
            self.remaining_samples = self.ramp_samples;
        }

        if self.remaining_samples == 0 {
            return self.current;
        }

        self.current += (self.target - self.current) / self.remaining_samples as f32;
        self.remaining_samples -= 1;
        if self.remaining_samples == 0 {
            self.current = self.target;
        }
        self.current
    }
}

impl AudioEffect for GainEffect {
    fn process(&mut self, frame: &mut frame::Audio) {
        for sample_index in 0..frame.samples() {
            let gain = self.next_gain();
            for plane in 0..frame.planes() {
                let sample = &mut frame.plane_mut::<f32>(plane)[sample_index];
                *sample = if sample.is_finite() {
                    *sample * gain
                } else {
                    0.0
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ffmpeg_next::{
        format::sample::{Sample, Type as SampleType},
        util::channel_layout::ChannelLayout,
    };

    struct DoubleEffect;

    impl AudioEffect for DoubleEffect {
        fn process(&mut self, frame: &mut frame::Audio) {
            for plane in 0..frame.planes() {
                for sample in frame.plane_mut::<f32>(plane) {
                    *sample *= 2.0;
                }
            }
        }
    }

    #[test]
    fn rejects_invalid_volume() {
        let control = AudioEffectsControl::default();
        for volume in [-0.1, 1.1, f64::NAN, f64::INFINITY] {
            assert!(control.set_volume(volume).is_err());
        }
    }

    #[test]
    fn gain_reaches_new_target_after_ramp() {
        let control = AudioEffectsControl::new(1.0).unwrap();
        let mut gain = GainEffect::new(control.clone(), 1_000);
        control.set_volume(0.0).unwrap();

        let values = (0..20).map(|_| gain.next_gain()).collect::<Vec<_>>();
        assert!(values.windows(2).all(|values| values[0] > values[1]));
        assert_eq!(values.last().copied(), Some(0.0));
    }

    #[test]
    fn chain_runs_added_effects_in_order() {
        let control = AudioEffectsControl::new(1.0).unwrap();
        let mut chain = AudioEffectChain::new(control, 48_000);
        chain.add(DoubleEffect);
        let mut frame =
            frame::Audio::new(Sample::F32(SampleType::Planar), 4, ChannelLayout::STEREO);
        for plane in 0..frame.planes() {
            frame.plane_mut::<f32>(plane).fill(0.25);
        }

        chain.process(&mut frame);

        for plane in 0..frame.planes() {
            assert_eq!(frame.plane::<f32>(plane), &[0.5; 4]);
        }
    }
}
