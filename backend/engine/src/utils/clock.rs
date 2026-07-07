use std::{
    thread,
    time::{Duration, Instant},
};

use ffmpeg_next::{Rational, Rescale, rescale};

pub(crate) struct PlayoutClock {
    started_at: Option<Instant>,
}

impl PlayoutClock {
    pub(crate) fn new() -> Self {
        Self { started_at: None }
    }

    pub(crate) fn wait_until(&mut self, timestamp: Option<i64>, time_base: Rational) {
        let Some(timestamp) = timestamp else {
            return;
        };

        let started_at = *self.started_at.get_or_insert_with(Instant::now);
        let target_us = timestamp.rescale(time_base, rescale::TIME_BASE);
        if target_us <= 0 {
            return;
        }

        let target = Duration::from_micros(target_us as u64);
        if let Some(delay) = target.checked_sub(started_at.elapsed()) {
            thread::sleep(delay);
        }
    }
}
