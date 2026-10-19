use std::time::{Duration, Instant};

use log::warn;

/// Large discontinuities are repaired only when delivery timing does not
/// support a real gap. Smaller gaps and timestamp quantization remain intact.
const DISCONTINUITY: Duration = Duration::from_millis(500);
const DELIVERY_TOLERANCE: Duration = Duration::from_millis(500);
const AUDIO_RECOVERY_GRACE: Duration = Duration::from_millis(2500);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TimestampCorrection {
    Outlier,
    ClockReset,
}

pub(crate) struct TimestampUpdate {
    pub(crate) pts: i64,
    pub(crate) correction: Option<TimestampCorrection>,
}

impl TimestampUpdate {
    pub(crate) fn log_correction(&self, track: &str, source_pts: i64, rate: i64, channel_id: i32) {
        if let Some(correction) = self.correction {
            warn!(channel = channel_id;
                "live {track} timestamp discontinuity ({correction:?}): {:.3} s corrected to {:.3} s",
                source_pts as f64 / rate as f64, self.pts as f64 / rate as f64
            );
        }
    }
}

/// Each track retains its original position on the shared source timeline.
/// One outlier cannot change the other track's mapping. A consistent new clock
/// is confirmed by the next frame; an isolated outlier leaves no lasting offset.
#[derive(Default)]
pub(crate) struct LiveTimestampTracker {
    next_pts: Option<i64>,
    offset: i64,
    pending_offset: Option<i64>,
    received_at: Option<Instant>,
    duration_pts: i64,
}

impl LiveTimestampTracker {
    pub(crate) fn seed(&mut self, pts: i64) {
        *self = Self {
            next_pts: Some(pts),
            ..Self::default()
        };
    }

    #[cfg(test)]
    pub(crate) fn normalize(
        &mut self,
        source_pts: i64,
        duration_pts: i64,
        ticks_per_second: i64,
        now: Instant,
    ) -> TimestampUpdate {
        self.normalize_with_reference(source_pts, duration_pts, ticks_per_second, None, now)
    }

    pub(crate) fn next_pts(&self) -> Option<i64> {
        self.next_pts
    }

    pub(crate) fn has_received_frame(&self) -> bool {
        self.received_at.is_some()
    }

    pub(crate) fn normalize_with_reference(
        &mut self,
        source_pts: i64,
        duration_pts: i64,
        ticks_per_second: i64,
        reference_pts: Option<i64>,
        now: Instant,
    ) -> TimestampUpdate {
        let mut pts = source_pts.saturating_add(self.offset);
        let mut correction = None;

        if let Some(expected) = self.next_pts {
            let difference = pts.saturating_sub(expected);
            let threshold = (DISCONTINUITY.as_secs_f64() * ticks_per_second as f64) as i64;
            let elapsed = self
                .received_at
                .map(|previous| now.saturating_duration_since(previous))
                .unwrap_or_default();
            let delivery_gap =
                (elapsed.as_secs_f64() * ticks_per_second as f64) as i64 - self.duration_pts;
            let tolerance = (DELIVERY_TOLERANCE.as_secs_f64() * ticks_per_second as f64) as i64;
            let counterpart_supports_gap =
                reference_pts.is_some_and(|reference| pts <= reference.saturating_add(tolerance));
            let genuine_gap = difference > 0
                && (difference <= delivery_gap.saturating_add(tolerance)
                    || counterpart_supports_gap);

            if difference.unsigned_abs() > threshold as u64 && !genuine_gap {
                let candidate_offset = expected.saturating_sub(source_pts);
                let confirmation_tolerance = duration_pts.max(ticks_per_second / 200).max(1);

                if self.pending_offset.is_some_and(|pending| {
                    candidate_offset.abs_diff(pending) <= confirmation_tolerance as u64
                }) {
                    self.offset = candidate_offset;
                    self.pending_offset = None;
                    correction = Some(TimestampCorrection::ClockReset);
                } else {
                    self.pending_offset = Some(candidate_offset);
                    correction = Some(TimestampCorrection::Outlier);
                }

                pts = expected;
            } else {
                self.pending_offset = None;
            }
        }

        self.next_pts = Some(pts.saturating_add(duration_pts));
        self.duration_pts = duration_pts;
        self.received_at = Some(now);

        TimestampUpdate { pts, correction }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AudioRecoveryAction {
    Wait,
    Report,
    Rebase,
}

#[derive(Default)]
pub(crate) struct LateAudioRecovery {
    first_pts: Option<i64>,
    first_gap: i64,
    last_pts: Option<i64>,
    reported: bool,
    pub(crate) discarded_frames: u64,
}

impl LateAudioRecovery {
    pub(crate) fn observe(
        &mut self,
        source_pts: i64,
        gap: i64,
        sample_rate: u32,
        last_output_at: Option<Instant>,
        now: Instant,
    ) -> AudioRecoveryAction {
        let tolerance = i64::from(sample_rate) / 200;
        let catching_up = gap.saturating_add(tolerance) < self.first_gap;
        let went_backwards = self.last_pts.is_some_and(|previous| source_pts < previous);

        if self.first_pts.is_none() || catching_up || went_backwards {
            self.first_pts = Some(source_pts);
            self.first_gap = gap;
        }

        self.last_pts = Some(source_pts);
        self.discarded_frames = self.discarded_frames.saturating_add(1);
        let overdue = last_output_at.is_some_and(|previous| {
            now.saturating_duration_since(previous) >= AUDIO_RECOVERY_GRACE
        });
        let advancing = source_pts.saturating_sub(self.first_pts.unwrap_or(source_pts))
            >= i64::from(sample_rate);

        if overdue && !self.reported {
            self.reported = true;

            return AudioRecoveryAction::Report;
        }

        if overdue && advancing && !catching_up {
            return AudioRecoveryAction::Rebase;
        }

        AudioRecoveryAction::Wait
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: i64 = 48_000;
    const SAMPLES: i64 = 1920;

    #[test]
    fn isolated_forward_and_backward_outliers_do_not_change_the_clock() {
        for jump in [258_096, -258_096] {
            let now = Instant::now();
            let mut clock = LiveTimestampTracker::default();
            let start = RATE * 10;
            assert_eq!(clock.normalize(start, SAMPLES, RATE, now).pts, start);
            let outlier = clock.normalize(
                start + SAMPLES + jump,
                SAMPLES,
                RATE,
                now + Duration::from_millis(40),
            );
            assert_eq!(outlier.pts, start + SAMPLES);
            assert_eq!(outlier.correction, Some(TimestampCorrection::Outlier));

            for index in 2..100 {
                let expected = start + index * SAMPLES;
                let normal = clock.normalize(
                    expected,
                    SAMPLES,
                    RATE,
                    now + Duration::from_millis(index as u64 * 40),
                );
                assert_eq!(normal.pts, expected);
                assert_eq!(normal.correction, None);
            }
        }
    }

    #[test]
    fn persistent_clock_resets_and_return_to_original_clock_remain_continuous() {
        for jump in [RATE * 30, -RATE * 30] {
            let now = Instant::now();
            let mut clock = LiveTimestampTracker::default();
            clock.normalize(RATE * 10, SAMPLES, RATE, now);

            for index in 1..10 {
                let expected = RATE * 10 + index * SAMPLES;
                let shifted = clock.normalize(
                    expected + jump,
                    SAMPLES,
                    RATE,
                    now + Duration::from_millis(index as u64 * 40),
                );
                assert_eq!(shifted.pts, expected);
                assert_eq!(
                    shifted.correction,
                    match index {
                        1 => Some(TimestampCorrection::Outlier),
                        2 => Some(TimestampCorrection::ClockReset),
                        _ => None,
                    }
                );
            }

            for index in 10..20 {
                let expected = RATE * 10 + index * SAMPLES;
                assert_eq!(
                    clock
                        .normalize(
                            expected,
                            SAMPLES,
                            RATE,
                            now + Duration::from_millis(index as u64 * 40)
                        )
                        .pts,
                    expected
                );
            }
        }
    }

    #[test]
    fn real_delivery_pause_retains_its_timestamp_gap() {
        let now = Instant::now();
        let mut clock = LiveTimestampTracker::default();
        clock.normalize(0, 1024, RATE, now);
        let resumed = clock.normalize(RATE * 7, 1024, RATE, now + Duration::from_secs(7));
        assert_eq!(resumed.pts, RATE * 7);
        assert_eq!(resumed.correction, None);
        assert_eq!(
            clock
                .normalize(
                    RATE * 7 + 1024,
                    1024,
                    RATE,
                    now + Duration::from_secs(7) + Duration::from_millis(21)
                )
                .pts,
            RATE * 7 + 1024
        );
    }

    #[test]
    fn smaller_gaps_and_millisecond_jitter_are_preserved() {
        let now = Instant::now();
        let mut clock = LiveTimestampTracker::default();

        for pts in [0, 1008, 2064, RATE / 5, RATE / 5 + 1024] {
            let update = clock.normalize(pts, 1024, RATE, now);
            assert_eq!(update.pts, pts);
            assert_eq!(update.correction, None);
        }
    }

    #[test]
    fn counterpart_timestamps_preserve_real_gaps_when_delivery_is_bursty() {
        let now = Instant::now();
        let mut clock = LiveTimestampTracker::default();
        clock.normalize(0, 1024, RATE, now);
        let after_packet_loss =
            clock.normalize_with_reference(RATE * 2, 1024, RATE, Some(RATE * 2), now);
        assert_eq!(after_packet_loss.pts, RATE * 2);
        assert_eq!(after_packet_loss.correction, None);
    }

    #[test]
    fn joint_clock_reset_preserves_the_initial_audio_video_offset() {
        let now = Instant::now();
        let mut video = LiveTimestampTracker::default();
        let mut audio = LiveTimestampTracker::default();
        video.normalize(0, SAMPLES, RATE, now);
        audio.normalize(480, SAMPLES, RATE, now);

        for index in 1..10 {
            let time = now + Duration::from_millis(index as u64 * 40);
            let video_pts = video
                .normalize(index * SAMPLES + RATE * 30, SAMPLES, RATE, time)
                .pts;
            let audio_pts = audio
                .normalize(index * SAMPLES + RATE * 30 + 480, SAMPLES, RATE, time)
                .pts;
            assert_eq!(video_pts, index * SAMPLES);
            assert_eq!(audio_pts - video_pts, 480);
        }
    }

    #[test]
    fn genuinely_late_audio_that_is_catching_up_is_not_rebased() {
        let now = Instant::now();
        let mut recovery = LateAudioRecovery::default();

        for index in 0..100 {
            let action = recovery.observe(
                index * 1024,
                RATE * 4 - index * 1024,
                RATE as u32,
                Some(now - Duration::from_secs(4)),
                now,
            );
            assert_ne!(action, AudioRecoveryAction::Rebase);
        }
    }

    #[test]
    fn persistent_lag_is_reported_and_recovered_but_repeated_frames_only_report_once() {
        let now = Instant::now();
        let last_output = Some(now - Duration::from_secs(4));
        let mut recovery = LateAudioRecovery::default();
        assert!(
            recovery.observe(0, RATE * 4, RATE as u32, last_output, now)
                == AudioRecoveryAction::Report
        );

        for _ in 0..100 {
            assert!(
                recovery.observe(0, RATE * 4, RATE as u32, last_output, now)
                    == AudioRecoveryAction::Wait
            );
        }

        assert!(
            recovery.observe(RATE, RATE * 4, RATE as u32, last_output, now)
                == AudioRecoveryAction::Rebase
        );
    }
}
