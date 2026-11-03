//! Source and layer timing on the canonical program clock, never wall time.
use std::num::NonZeroU32;

use anyhow::{Result, ensure};

/// Total presentations, including the first, rather than additional repetitions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repeat {
    Count(NonZeroU32),
    Forever,
}

impl Default for Repeat {
    fn default() -> Self {
        Self::Count(NonZeroU32::MIN)
    }
}

/// A static graphic normally holds until stopped. Repeating a fade cycle needs
/// a finite hold so every iteration has a defined end.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Transition {
    pub fade_in_us: u64,
    pub hold_us: Option<u64>,
    pub fade_out_us: u64,
    /// Invisible interval between presentations; omitted after the final repeat.
    pub pause_us: u64,
    pub repeat: Repeat,
}

impl Transition {
    pub fn validate(self) -> Result<()> {
        ensure!(
            self.hold_us.is_some() || self.repeat == Repeat::default(),
            "repeated transitions need a finite hold"
        );
        let active = self.active_duration();
        let cycle = active + u128::from(self.pause_us);
        ensure!(
            cycle <= i64::MAX as u128,
            "transition duration is too large"
        );
        ensure!(
            self.repeat == Repeat::default() || active > 0,
            "a repeated transition must have a positive visible duration"
        );

        Ok(())
    }

    fn active_duration(self) -> u128 {
        u128::from(self.fade_in_us)
            + u128::from(self.hold_us.unwrap_or(0))
            + u128::from(self.fade_out_us)
    }

    fn opacity(self, elapsed: u128) -> Option<f64> {
        let elapsed = if self.hold_us.is_some() {
            let active = self.active_duration();
            let cycle = active + u128::from(self.pause_us);
            let finished = match self.repeat {
                Repeat::Count(count) => elapsed >= cycle * u128::from(count.get() - 1) + active,
                Repeat::Forever => false,
            };
            if finished || active == 0 {
                return None;
            }
            let phase = elapsed % cycle;
            if phase >= active {
                // Keep the presentation alive so controllers do not expire it
                // during the invisible interval before another repetition.
                return Some(0.0);
            }

            phase
        } else {
            elapsed
        };
        let entrance = u128::from(self.fade_in_us);
        if entrance > 0 && elapsed < entrance {
            return Some(elapsed as f64 / entrance as f64);
        }
        if let Some(hold) = self.hold_us {
            let exit = entrance + u128::from(hold);
            if elapsed >= exit {
                return Some(1.0 - (elapsed - exit) as f64 / self.fade_out_us.max(1) as f64);
            }
        }

        Some(1.0)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Presentation {
    start_us: i64,
    transition: Transition,
    stopped: Option<(i64, f64, u64)>,
}

impl Presentation {
    pub fn new(start_us: i64, transition: Transition) -> Result<Self> {
        transition.validate()?;

        Ok(Self {
            start_us,
            transition,
            stopped: None,
        })
    }

    /// Source-local time since take. Fade cycles and visibility do not restart
    /// the source; a new take constructs a new presentation anchor.
    pub fn elapsed_us(self, program_us: i64) -> i64 {
        program_us.saturating_sub(self.start_us)
    }

    /// None means absent/finished; Some(0) is a transparent entrance or repeat pause.
    pub fn opacity(self, program_us: i64) -> Option<f64> {
        if program_us < self.start_us {
            return None;
        }
        if let Some((stop, opacity, duration)) = self.stopped
            && program_us >= stop
        {
            let elapsed = (i128::from(program_us) - i128::from(stop)) as u128;
            let duration = u128::from(duration);
            if duration == 0 || elapsed >= duration || opacity == 0.0 {
                return None;
            }

            return Some(opacity * (1.0 - elapsed as f64 / duration as f64));
        }

        self.transition
            .opacity((i128::from(program_us) - i128::from(self.start_us)) as u128)
    }

    /// Idempotent: repeated stops never prolong an exit or restart a fade.
    pub fn stop(&mut self, program_us: i64) {
        if self.stopped.is_none() {
            let opacity = self.opacity(program_us).unwrap_or(0.0);
            let mut duration = self.transition.fade_out_us;
            if self.transition.hold_us.is_some() {
                let active = self.transition.active_duration();
                let cycle = active + u128::from(self.transition.pause_us);
                let elapsed = (i128::from(program_us) - i128::from(self.start_us)).max(0) as u128;
                let remaining = if cycle > 0 {
                    active.saturating_sub(elapsed % cycle)
                } else {
                    0
                };
                duration = u128::from(duration).min(remaining) as u64;
            }
            self.stopped = Some((program_us, opacity, duration));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_graphic_holds_and_stops_from_its_current_opacity() {
        let mut presentation = Presentation::new(
            0,
            Transition {
                fade_in_us: 1_000_000,
                fade_out_us: 2_000_000,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(presentation.opacity(-1), None);
        assert_eq!(presentation.opacity(0), Some(0.0));
        presentation.stop(500_000);
        assert_eq!(presentation.opacity(500_000), Some(0.5));
        assert_eq!(presentation.opacity(1_500_000), Some(0.25));
        presentation.stop(1_500_000);
        assert_eq!(presentation.opacity(2_500_000), None);
    }

    #[test]
    fn finite_and_infinite_cycles_are_evaluated_without_drift() {
        for repeat in [Repeat::Count(NonZeroU32::new(2).unwrap()), Repeat::Forever] {
            let p = Presentation::new(
                100,
                Transition {
                    fade_in_us: 100,
                    hold_us: Some(200),
                    fade_out_us: 100,
                    repeat,
                    pause_us: 0,
                },
            )
            .unwrap();
            for cycle in 0..2 {
                assert_eq!(p.opacity(100 + cycle * 400 + 50), Some(0.5));
                assert_eq!(p.opacity(100 + cycle * 400 + 200), Some(1.0));
                assert_eq!(p.opacity(100 + cycle * 400 + 350), Some(0.5));
            }
            if repeat == Repeat::Forever {
                assert_eq!(p.opacity(100 + 1_000_000_000 * 400 + 50), Some(0.5));
            } else {
                assert_eq!(p.opacity(900), None);
            }
        }
    }

    fn paused_transition(repeat: Repeat) -> Transition {
        Transition {
            fade_in_us: 500_000,
            hold_us: Some(4_000_000),
            fade_out_us: 500_000,
            pause_us: 10_000_000,
            repeat,
        }
    }

    #[test]
    fn repeat_pauses_are_transparent_without_a_trailing_pause() {
        let p = Presentation::new(
            0,
            paused_transition(Repeat::Count(NonZeroU32::new(3).unwrap())),
        )
        .unwrap();
        for start in [0, 15_000_000, 30_000_000] {
            assert_eq!(p.opacity(start + 250_000), Some(0.5));
            assert_eq!(p.opacity(start + 1_000_000), Some(1.0));
            assert_eq!(p.opacity(start + 4_750_000), Some(0.5));
        }
        for time in [5_000_000, 14_999_999, 20_000_000, 29_999_999] {
            assert_eq!(p.opacity(time), Some(0.0));
        }
        assert_eq!(p.opacity(35_000_000), None);
        assert_eq!(p.opacity(44_999_999), None);
        let once = Presentation::new(0, paused_transition(Repeat::default())).unwrap();
        assert_eq!(once.opacity(5_000_000), None);
    }

    #[test]
    fn infinite_pause_cycles_remain_seekable_and_do_not_accumulate_drift() {
        let p = Presentation::new(0, paused_transition(Repeat::Forever)).unwrap();
        assert_eq!(p.opacity(15_000_000 * 1_000_000_000 + 250_000), Some(0.5));
        assert_eq!(p.opacity(8_000_000), Some(0.0));
        assert_eq!(p.opacity(250_000), Some(0.5));
        assert_eq!(p.opacity(8_000_000), Some(0.0));
    }

    #[test]
    fn stopping_in_a_pause_cancels_future_repeats_and_automatic_exits_never_extend() {
        let mut paused = Presentation::new(0, paused_transition(Repeat::Forever)).unwrap();
        paused.stop(8_000_000);
        assert_eq!(paused.opacity(8_000_000), None);
        assert_eq!(paused.opacity(16_000_000), None);
        for stop in [4_750_000, 34_750_000] {
            let mut exiting = Presentation::new(
                0,
                paused_transition(Repeat::Count(NonZeroU32::new(3).unwrap())),
            )
            .unwrap();
            exiting.stop(stop);
            assert_eq!(exiting.opacity(stop + 125_000), Some(0.25));
            assert_eq!(exiting.opacity(stop + 250_000), None);
        }
        assert!(
            Presentation::new(
                0,
                Transition {
                    hold_us: Some(0),
                    pause_us: 1,
                    repeat: Repeat::Forever,
                    ..Default::default()
                }
            )
            .is_err()
        );
        assert!(
            Presentation::new(
                0,
                Transition {
                    pause_us: u64::MAX,
                    ..Default::default()
                }
            )
            .is_err()
        );
    }

    #[test]
    fn pauses_and_program_fps_do_not_change_the_opacity_function() {
        let p = Presentation::new(
            0,
            Transition {
                fade_in_us: 1_000_000,
                ..Default::default()
            },
        )
        .unwrap();
        for fps in [24_i64, 25, 30, 50, 60] {
            for frame in [0, fps / 2, fps] {
                let time = frame * 1_000_000 / fps;
                let expected = time as f64 / 1_000_000.0;
                assert_eq!(p.opacity(time), Some(expected));
                assert_eq!(
                    p.opacity(time),
                    Some(expected),
                    "paused time must not advance"
                );
            }
        }

        assert_eq!(p.opacity(i64::MAX), Some(1.0));
    }

    #[test]
    fn stopping_an_automatic_exit_cannot_extend_its_deadline() {
        let mut p = Presentation::new(
            0,
            Transition {
                fade_in_us: 100,
                hold_us: Some(100),
                fade_out_us: 100,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(p.opacity(250), Some(0.5));
        p.stop(250);
        assert_eq!(p.opacity(275), Some(0.25));
        assert_eq!(p.opacity(300), None);
    }

    #[test]
    fn instant_transitions_and_invalid_cycles_have_explicit_semantics() {
        let mut p = Presentation::new(0, Transition::default()).unwrap();
        assert_eq!(p.opacity(0), Some(1.0));
        p.stop(1);
        assert_eq!(p.opacity(1), None);
        assert!(
            Presentation::new(
                0,
                Transition {
                    repeat: Repeat::Forever,
                    ..Default::default()
                }
            )
            .is_err()
        );
        assert!(
            Presentation::new(
                0,
                Transition {
                    hold_us: Some(0),
                    repeat: Repeat::Forever,
                    ..Default::default()
                }
            )
            .is_err()
        );
        assert!(
            Presentation::new(
                0,
                Transition {
                    fade_in_us: u64::MAX,
                    ..Default::default()
                }
            )
            .is_err()
        );
    }
}
