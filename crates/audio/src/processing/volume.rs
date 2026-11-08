use crate::buffer::AudioBuffer;

use super::control::{AudioEffect, AudioEffectsControl};

const GAIN_RAMP_MILLISECONDS: u32 = 20;

pub struct GainEffect {
    control: AudioEffectsControl,
    current: f32,
    target: f32,
    ramp_samples: usize,
    remaining_samples: usize,
    gain_buffer: Vec<f32>,
}

impl GainEffect {
    pub fn new(control: AudioEffectsControl, sample_rate: u32) -> Self {
        let volume = control.volume_f32();

        Self {
            control,
            current: volume,
            target: volume,
            ramp_samples: ((sample_rate as u64 * u64::from(GAIN_RAMP_MILLISECONDS)) / 1_000).max(1)
                as usize,
            remaining_samples: 0,
            gain_buffer: Vec::new(),
        }
    }

    pub fn preview_gain_bound(&self) -> f64 {
        f64::from(self.current.max(self.target).max(self.control.volume_f32()))
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
    fn process(&mut self, frame: &mut dyn AudioBuffer) {
        let samples = frame.samples();
        let ramping = self.remaining_samples > 0 || self.control.volume_f32() != self.target;

        if !ramping {
            let gain = self.current;

            for plane in 0..frame.planes() {
                for sample in frame.plane_mut(plane) {
                    *sample = if sample.is_finite() {
                        *sample * gain
                    } else {
                        0.0
                    };
                }
            }

            return;
        }

        self.gain_buffer.clear();
        self.gain_buffer
            .reserve(samples.saturating_sub(self.gain_buffer.capacity()));

        for _ in 0..samples {
            let gain = self.next_gain();
            self.gain_buffer.push(gain);
        }

        for plane in 0..frame.planes() {
            for (sample, gain) in frame.plane_mut(plane).iter_mut().zip(&self.gain_buffer) {
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
    use crate::{AudioEffectChain, buffer::AudioBuffer, test_support::Audio as TestAudio};

    use super::*;

    struct DoubleEffect;

    impl AudioEffect for DoubleEffect {
        fn process(&mut self, frame: &mut dyn AudioBuffer) {
            for plane in 0..frame.planes() {
                for sample in frame.plane_mut(plane) {
                    *sample *= 2.0;
                }
            }
        }
    }

    #[test]
    fn gain_ramp_supports_surround_without_crossing_track_state() {
        use crate::buffer::{Channel, PlanarBlock};

        let control = AudioEffectsControl::default();
        let mut effect = GainEffect::new(control.clone(), 1_000);
        let mut samples = [[0.5; 20]; 6];
        let mut planes = samples.each_mut().map(|channel| &mut channel[..]);
        let layout = [
            Channel::Left,
            Channel::Right,
            Channel::Center,
            Channel::LowFrequency,
            Channel::SurroundLeft,
            Channel::SurroundRight,
        ];
        let mut audio = PlanarBlock::new(&mut planes, &layout).unwrap();
        let other_control = AudioEffectsControl::new(0.5).unwrap();
        let mut other = GainEffect::new(other_control, 1_000);
        let mut mono = [0.5; 20];
        let mut mono_planes = [&mut mono[..]];
        let mut other_audio = PlanarBlock::new(&mut mono_planes, &[Channel::Center]).unwrap();

        control.set_volume(0.0).unwrap();
        effect.process(&mut audio);
        other.process(&mut other_audio);

        for channel in 0..6 {
            assert_eq!(audio.plane(channel), audio.plane(0));
            assert_eq!(audio.plane(channel).last(), Some(&0.0));
        }

        assert_eq!(other_audio.plane(0), &[0.25; 20]);
    }

    #[test]
    fn rejects_invalid_volume() {
        let control = AudioEffectsControl::default();

        for volume in [-0.1, 3.1, f64::NAN, f64::INFINITY] {
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
        let mut frame = TestAudio::new(4);
        for plane in 0..frame.planes() {
            frame.plane_mut(plane).fill(0.25);
        }

        chain.process(&mut frame);

        for plane in 0..frame.planes() {
            assert_eq!(frame.plane(plane), &[0.5; 4]);
        }
    }
}
