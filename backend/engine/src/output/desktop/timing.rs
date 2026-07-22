use std::time::Instant;

use ffmpeg_next::{Rational, Rescale};

pub(super) struct AudioMasterClock {
    sample_rate: u32,
    device_buffer_samples: u64,
    last_consumed_samples: u64,
    anchor_samples: u64,
    anchor_time: Instant,
}

impl AudioMasterClock {
    pub(super) fn new(sample_rate: u32, device_buffer_samples: u64) -> Self {
        Self {
            sample_rate,
            device_buffer_samples,
            last_consumed_samples: 0,
            anchor_samples: 0,
            anchor_time: Instant::now(),
        }
    }

    pub(super) fn reset_at(&mut self, samples: u64, now: Instant) {
        self.last_consumed_samples = samples;
        self.anchor_samples = samples;
        self.anchor_time = now;
    }

    pub(super) fn position(&mut self, submitted: u64, queued: u64, now: Instant) -> u64 {
        let consumed = submitted.saturating_sub(queued);
        if consumed != self.last_consumed_samples {
            self.last_consumed_samples = consumed;
            self.anchor_samples = consumed.saturating_sub(self.device_buffer_samples);
            self.anchor_time = now;
        }
        let elapsed_samples = (now.duration_since(self.anchor_time).as_secs_f64()
            * f64::from(self.sample_rate)) as u64;
        self.anchor_samples
            .saturating_add(elapsed_samples)
            .min(consumed)
    }
}

pub(super) fn video_pts_in_audio_samples(
    video_pts: i64,
    video_time_base: Rational,
    sample_rate: u32,
) -> u64 {
    video_pts
        .rescale(video_time_base, Rational(1, sample_rate as i32))
        .max(0) as u64
}

pub(super) fn adjusted_volume(volume: f64, delta: f64, min: f64, max: f64) -> f64 {
    (volume + delta).clamp(min, max)
}
