//! Presentation timing for reusable image and video graphics sources.
//! Times are integer microseconds on a source-local timeline, independent of decoding.

use crate::playback::Repeat;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Completion {
    Hide,
    HoldLast,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameTiming {
    pub start_us: i64,
    pub duration_us: i64,
}

impl FrameTiming {
    pub fn end_us(self) -> i64 {
        self.start_us.saturating_add(self.duration_us.max(1))
    }

    pub fn contains(self, time_us: i64) -> bool {
        time_us >= self.start_us && time_us < self.end_us()
    }
}

/// Exact source duration, expressed as a rational number of microseconds.
/// Modulo is evaluated before rounding so fractional frame rates cannot
/// accumulate a rounded-duration error on each repetition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoopDuration {
    numerator: i128,
    denominator: i128,
}

impl LoopDuration {
    pub fn from_ticks(ticks: i64, numerator: i32, denominator: i32) -> Self {
        Self {
            numerator: i128::from(ticks.max(1)) * i128::from(numerator.max(1)) * 1_000_000,
            denominator: i128::from(denominator.max(1)),
        }
    }

    /// Finite/forever source playback is independent of a layer's fade cycle.
    pub fn playback_position(
        self,
        elapsed_us: i64,
        repeat: Repeat,
        completion: Completion,
    ) -> Option<i64> {
        if elapsed_us < 0 {
            return None;
        }
        let scaled = i128::from(elapsed_us) * self.denominator;
        let finished = match repeat {
            Repeat::Count(count) => scaled / self.numerator >= i128::from(count.get()),
            Repeat::Forever => false,
        };
        if finished {
            return match completion {
                Completion::Hide => None,
                Completion::HoldLast => {
                    Some(((self.numerator - 1) / self.denominator).min(i128::from(i64::MAX)) as i64)
                }
            };
        }

        Some(self.position_us(elapsed_us))
    }

    pub fn position_us(self, elapsed_us: i64) -> i64 {
        let remainder = (i128::from(elapsed_us.max(0)) * self.denominator) % self.numerator;

        (remainder / self.denominator) as i64
    }
}

pub fn loop_time(program_us: i64, start_us: i64, duration_us: i64) -> i64 {
    program_us.saturating_sub(start_us).max(0) % duration_us.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_repeat_counts_include_the_first_play_and_can_hold_or_hide_at_eof() {
        use std::num::NonZeroU32;

        let duration = LoopDuration::from_ticks(2002, 1, 30_000);
        let twice = Repeat::Count(NonZeroU32::new(2).unwrap());
        assert_eq!(
            duration.playback_position(-1, twice, Completion::Hide),
            None
        );
        assert_eq!(
            duration.playback_position(133_466, twice, Completion::Hide),
            Some(66_732)
        );
        assert_eq!(
            duration.playback_position(133_467, twice, Completion::Hide),
            None
        );
        assert_eq!(
            duration.playback_position(133_467, twice, Completion::HoldLast),
            Some(66_733)
        );
        assert_eq!(
            duration.playback_position(2_002_000_000, Repeat::Forever, Completion::Hide),
            Some(0)
        );
    }

    #[test]
    fn fractional_frame_rate_loops_do_not_accumulate_microsecond_rounding_error() {
        // Two frames at 30000/1001 fps last 66,733⅓ microseconds.
        let duration = LoopDuration::from_ticks(2002, 1, 30_000);
        // 30,000 exact loops land exactly on the first frame.
        assert_eq!(duration.position_us(2_002_000_000), 0);
        assert_eq!(duration.position_us(2_002_000_001), 1);
    }

    #[test]
    fn variable_durations_and_loop_boundaries_use_presentation_time() {
        let frames = [
            FrameTiming {
                start_us: 0,
                duration_us: 100_000,
            },
            FrameTiming {
                start_us: 100_000,
                duration_us: 300_000,
            },
            FrameTiming {
                start_us: 400_000,
                duration_us: 100_000,
            },
        ];
        for (time, expected) in [
            (0, 0),
            (99_999, 0),
            (100_000, 1),
            (399_999, 1),
            (400_000, 2),
            (500_000, 0),
        ] {
            let local = loop_time(time, 0, 500_000);
            assert_eq!(
                frames.iter().position(|frame| frame.contains(local)),
                Some(expected)
            );
        }
    }

    #[test]
    fn differing_frame_rates_and_long_running_loops_do_not_accumulate_error() {
        for fps in [24_i64, 25, 30, 50, 60] {
            for n in [1_i64, 2, 49, 1_000_000_000] {
                let program_us = (i128::from(n) * 1_000_000 / i128::from(fps)) as i64;
                assert_eq!(loop_time(program_us, 0, 2_000_000), program_us % 2_000_000);
            }
        }
    }
}
