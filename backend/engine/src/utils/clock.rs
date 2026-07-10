use std::{
    thread,
    time::{Duration, Instant},
};

use ffmpeg_next::{Rational, Rescale, rescale};

pub(crate) struct PlayoutClock {
    anchor_wall: Option<Instant>,
    anchor_media_us: i64,
    rate: f64,
    pending_rate: f64,
    reanchor_next: bool,
}

impl PlayoutClock {
    pub(crate) fn new() -> Self {
        Self {
            anchor_wall: None,
            anchor_media_us: 0,
            rate: 1.0,
            pending_rate: 1.0,
            reanchor_next: false,
        }
    }

    pub(crate) fn set_rate(&mut self, rate: f64) {
        self.pending_rate = if rate.is_finite() && rate > 0.0 {
            rate
        } else {
            1.0
        };
        self.reanchor_next = true;
    }

    pub(crate) fn wait_until(&mut self, timestamp: Option<i64>, time_base: Rational) {
        let Some(timestamp) = timestamp else {
            return;
        };

        let target_us = timestamp.rescale(time_base, rescale::TIME_BASE);
        let now = Instant::now();
        if self.anchor_wall.is_none() || self.reanchor_next {
            self.anchor_wall = Some(now);
            self.anchor_media_us = target_us;
            self.rate = self.pending_rate;
            self.reanchor_next = false;
        }
        if target_us <= self.anchor_media_us {
            return;
        }

        let anchor_wall = self.anchor_wall.unwrap_or(now);
        let media_delta_us = target_us - self.anchor_media_us;
        let paced_target_us = ((media_delta_us as f64) / self.rate).max(0.0) as u64;
        let target = Duration::from_micros(paced_target_us);
        if let Some(delay) = target.checked_sub(anchor_wall.elapsed()) {
            thread::sleep(delay);
        }
    }
}
