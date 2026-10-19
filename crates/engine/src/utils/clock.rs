#[cfg(test)]
use std::thread;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub(crate) type SharedPlayoutClock = Arc<Mutex<PlayoutClock>>;

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

    pub(crate) fn media_time_us(&self) -> Option<i64> {
        self.anchor_wall.map(|wall| {
            self.anchor_media_us
                .saturating_add((wall.elapsed().as_secs_f64() * 1_000_000.0 * self.rate) as i64)
        })
    }

    pub(crate) fn set_rate(&mut self, rate: f64) {
        let rate = if rate.is_finite() && rate > 0.0 {
            rate
        } else {
            1.0
        };
        if rate != self.pending_rate {
            self.pending_rate = rate;
            self.reanchor_next = true;
        }
    }

    #[cfg(test)]
    pub(crate) fn wait_until(&mut self, timestamp: Option<i64>, time_base: Rational) {
        thread::sleep(self.delay_until(timestamp, time_base));
    }

    pub(crate) fn delay_until(&mut self, timestamp: Option<i64>, time_base: Rational) -> Duration {
        let Some(timestamp) = timestamp else {
            return Duration::ZERO;
        };

        let target_us = timestamp.rescale(time_base, rescale::TIME_BASE);
        let now = Instant::now();

        if self.anchor_wall.is_none() || self.reanchor_next {
            let anchor = self.anchor_wall.map(|wall| {
                let delta = ((target_us - self.anchor_media_us) as f64 / self.rate).max(0.0);
                wall.checked_add(Duration::from_micros(delta as u64))
                    .unwrap_or(now)
            });
            self.anchor_wall = Some(anchor.unwrap_or(now));
            self.anchor_media_us = target_us;
            self.rate = self.pending_rate;
            self.reanchor_next = false;
        }

        let anchor_wall = self.anchor_wall.unwrap_or(now);
        let media_delta_us = target_us.saturating_sub(self.anchor_media_us);
        let paced_delta_us = media_delta_us as f64 / self.rate;
        let delta = Duration::from_micros(paced_delta_us.abs() as u64);
        let deadline = if paced_delta_us >= 0.0 {
            anchor_wall.checked_add(delta)
        } else {
            anchor_wall.checked_sub(delta)
        };

        deadline.map_or(Duration::ZERO, |deadline| {
            deadline.saturating_duration_since(now)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_boundaries_keep_the_original_deadline() {
        let mut clock = PlayoutClock::new();
        clock.wait_until(Some(0), Rational(1, 1_000));
        let anchor = clock.anchor_wall;
        for _ in 0..100 {
            clock.set_rate(1.0);
            clock.wait_until(Some(0), Rational(1, 1_000));
            assert_eq!(clock.anchor_wall, anchor);
        }
    }

    #[test]
    fn rate_change_still_waits_for_a_future_boundary() {
        let mut clock = PlayoutClock::new();
        clock.anchor_wall = Some(Instant::now() + Duration::from_millis(100));
        clock.set_rate(1.02);
        let delay = clock.delay_until(Some(0), Rational(1, 1_000));
        assert!(delay > Duration::from_millis(70));
        assert!(delay <= Duration::from_millis(100));
    }

    #[test]
    fn rate_change_preserves_media_deadline_even_when_processing_is_late() {
        let mut clock = PlayoutClock::new();
        let anchor = Instant::now() - Duration::from_secs(2);
        clock.anchor_wall = Some(anchor);
        clock.set_rate(1.02);
        clock.wait_until(Some(1_000), Rational(1, 1_000));
        assert_eq!(clock.anchor_wall, Some(anchor + Duration::from_secs(1)));
        assert_eq!(clock.rate, 1.02);
    }
}
