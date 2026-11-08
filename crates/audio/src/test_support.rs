//! Owned PCM fixtures; DSP tests require no codecs or native multimedia libraries.
use crate::buffer::{AudioBuffer, Channel};

pub(crate) struct Audio {
    channels: [Vec<f32>; 2],
    pts: Option<i64>,
}

impl Audio {
    pub(crate) fn new(samples: usize) -> Self {
        Self {
            channels: [vec![0.0; samples], vec![0.0; samples]],
            pts: None,
        }
    }

    pub(crate) fn set_rate(&mut self, rate: u32) {
        assert!(rate > 0);
    }
    pub(crate) fn set_pts(&mut self, pts: Option<i64>) {
        self.pts = pts;
    }

    pub(crate) fn pts(&self) -> Option<i64> {
        self.pts
    }
}

impl AudioBuffer for Audio {
    fn layout(&self) -> &[Channel] {
        &[Channel::Left, Channel::Right]
    }

    fn samples(&self) -> usize {
        self.channels[0].len()
    }

    fn plane(&self, channel: usize) -> &[f32] {
        &self.channels[channel]
    }

    fn plane_mut(&mut self, channel: usize) -> &mut [f32] {
        &mut self.channels[channel]
    }
}
