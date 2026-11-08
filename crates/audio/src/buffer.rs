//! Borrowed planar PCM. Channels are identified independently of their storage order.
use anyhow::{Result, ensure};

/// A speaker position, or a caller-defined channel with a stable identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Left,
    Right,
    Center,
    LowFrequency,
    SurroundLeft,
    SurroundRight,
    RearLeft,
    RearRight,
    Discrete(u16),
}

/// Planar `f32` samples. All planes must contain exactly `samples()` values.
/// Implementations must preserve channel order and sample count during processing.
pub trait AudioBuffer {
    fn layout(&self) -> &[Channel];

    fn samples(&self) -> usize;

    fn plane(&self, channel: usize) -> &[f32];

    fn plane_mut(&mut self, channel: usize) -> &mut [f32];

    fn planes(&self) -> usize {
        self.layout().len()
    }
}

/// A validated planar block borrowing storage owned by the caller.
/// Construction and processing do not allocate or convert samples.
pub struct PlanarBlock<'a, 'b> {
    channels: &'a mut [&'b mut [f32]],
    layout: &'a [Channel],
    samples: usize,
}

impl<'a, 'b> PlanarBlock<'a, 'b> {
    pub fn new(channels: &'a mut [&'b mut [f32]], layout: &'a [Channel]) -> Result<Self> {
        ensure!(
            !channels.is_empty() && channels.len() == layout.len(),
            "audio channels and layout must match"
        );
        let samples = channels[0].len();

        ensure!(
            channels.iter().all(|channel| channel.len() == samples),
            "audio planes must have equal lengths"
        );
        ensure!(
            layout
                .iter()
                .enumerate()
                .all(|(index, channel)| !layout[..index].contains(channel)),
            "audio layout contains duplicate channels"
        );

        Ok(Self {
            channels,
            layout,
            samples,
        })
    }
}

impl AudioBuffer for PlanarBlock<'_, '_> {
    fn layout(&self) -> &[Channel] {
        self.layout
    }

    fn samples(&self) -> usize {
        self.samples
    }

    fn plane(&self, channel: usize) -> &[f32] {
        self.channels[channel]
    }

    fn plane_mut(&mut self, channel: usize) -> &mut [f32] {
        self.channels[channel]
    }
}

mod sealed {
    pub trait Stereo {}
}

/// Only validated left/right stereo buffers can enter stereo-specific processors.
pub trait StereoBuffer: AudioBuffer + sealed::Stereo {}

/// Borrowed left/right PCM; no timestamps, sample conversion or allocation.
pub struct StereoBlock<'a> {
    left: &'a mut [f32],
    right: &'a mut [f32],
}

impl<'a> StereoBlock<'a> {
    pub fn new(left: &'a mut [f32], right: &'a mut [f32]) -> Result<Self> {
        ensure!(
            left.len() == right.len(),
            "stereo planes must have equal lengths"
        );

        Ok(Self { left, right })
    }

    pub fn from_planar(block: &'a mut PlanarBlock<'_, '_>) -> Result<Self> {
        ensure!(
            block.layout == [Channel::Left, Channel::Right],
            "processor requires left/right stereo"
        );
        let (left, right) = block.channels.split_at_mut(1);

        Self::new(left[0], right[0])
    }
}

impl AudioBuffer for StereoBlock<'_> {
    fn layout(&self) -> &[Channel] {
        &[Channel::Left, Channel::Right]
    }

    fn samples(&self) -> usize {
        self.left.len()
    }

    fn plane(&self, channel: usize) -> &[f32] {
        match channel {
            0 => self.left,
            1 => self.right,
            _ => panic!("invalid stereo channel"),
        }
    }

    fn plane_mut(&mut self, channel: usize) -> &mut [f32] {
        match channel {
            0 => self.left,
            1 => self.right,
            _ => panic!("invalid stereo channel"),
        }
    }
}

impl sealed::Stereo for StereoBlock<'_> {}
impl StereoBuffer for StereoBlock<'_> {}

#[cfg(test)]
impl sealed::Stereo for crate::test_support::Audio {}
#[cfg(test)]
impl StereoBuffer for crate::test_support::Audio {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_layout_and_plane_lengths_without_copying() {
        let mut left = [0.0; 4];
        let mut right = [0.0; 4];
        let mut center = [0.0; 4];
        let mut planes = [&mut left[..], &mut right[..], &mut center[..]];
        let mut block = PlanarBlock::new(
            &mut planes,
            &[Channel::Left, Channel::Right, Channel::Center],
        )
        .unwrap();

        block.plane_mut(2)[1] = 0.5;
        assert!(StereoBlock::from_planar(&mut block).is_err());
        assert_eq!(center[1], 0.5);
        assert!(StereoBlock::new(&mut left[..2], &mut right).is_err());
        assert!(PlanarBlock::new(&mut [], &[]).is_err());
    }
}
