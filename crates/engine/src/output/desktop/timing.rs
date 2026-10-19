use std::time::{Duration, Instant};

use ffmpeg_next::{Rational, Rescale};

#[derive(Default)]
pub(super) struct VideoRedrawTiming {
    last_queued_pts: Option<i64>,
    queued_at: Option<Instant>,
    superseded_frames: u64,
    last_redraw: Option<Instant>,
}

pub(super) struct VideoRedrawSample {
    pub(super) queue_delay: Duration,
    pub(super) redraw_interval: Option<Duration>,
    pub(super) superseded_frames: u64,
}

impl VideoRedrawTiming {
    pub(super) fn queue_frame(&mut self, pts: Option<i64>, now: Instant) {
        let Some(pts) = pts else {
            *self = Self::default();

            return;
        };

        // Overlay updates and OS redraws of the same video frame must not
        // restart its waiting time or count as skipped video frames.
        if self.last_queued_pts == Some(pts) {
            return;
        }

        if self.queued_at.replace(now).is_some() {
            self.superseded_frames = self.superseded_frames.saturating_add(1);
        }

        self.last_queued_pts = Some(pts);
    }

    pub(super) fn begin_redraw(&mut self, now: Instant) -> Option<VideoRedrawSample> {
        let queued_at = self.queued_at.take()?;
        let redraw_interval = self.last_redraw.replace(now).map(|last| now - last);
        let superseded_frames = self.superseded_frames;
        self.superseded_frames = 0;

        Some(VideoRedrawSample {
            queue_delay: now - queued_at,
            redraw_interval,
            superseded_frames,
        })
    }
}

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

    pub(super) fn position(
        &mut self,
        submitted: u64,
        queued: u64,
        now: Instant,
        allow_underflow: bool,
    ) -> u64 {
        let consumed = submitted.saturating_sub(queued);

        if consumed != self.last_consumed_samples {
            self.last_consumed_samples = consumed;
            self.anchor_samples = consumed.saturating_sub(self.device_buffer_samples);
            self.anchor_time = now;
        }

        let elapsed_samples = (now.duration_since(self.anchor_time).as_secs_f64()
            * f64::from(self.sample_rate)) as u64;
        let interpolated = self.anchor_samples.saturating_add(elapsed_samples);

        if allow_underflow {
            interpolated
        } else {
            interpolated.min(consumed)
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_updates_do_not_hide_frame_waiting_time() {
        let start = Instant::now();
        let mut timing = VideoRedrawTiming::default();
        timing.queue_frame(Some(1), start);
        timing.queue_frame(Some(1), start + Duration::from_millis(5));
        let sample = timing
            .begin_redraw(start + Duration::from_millis(10))
            .unwrap();

        assert_eq!(sample.queue_delay, Duration::from_millis(10));
        assert_eq!(sample.superseded_frames, 0);
        assert_eq!(sample.redraw_interval, None);
        assert!(
            timing
                .begin_redraw(start + Duration::from_millis(15))
                .is_none()
        );

        timing.queue_frame(Some(2), start + Duration::from_millis(40));
        let sample = timing
            .begin_redraw(start + Duration::from_millis(50))
            .unwrap();

        assert_eq!(sample.redraw_interval, Some(Duration::from_millis(40)));
    }

    #[test]
    fn coalesced_frames_report_latest_delay_and_reset_skip_count() {
        let start = Instant::now();
        let mut timing = VideoRedrawTiming::default();
        timing.queue_frame(Some(1), start);
        timing.queue_frame(Some(2), start + Duration::from_millis(20));
        timing.queue_frame(Some(3), start + Duration::from_millis(40));
        let sample = timing
            .begin_redraw(start + Duration::from_millis(45))
            .unwrap();

        assert_eq!(sample.queue_delay, Duration::from_millis(5));
        assert_eq!(sample.superseded_frames, 2);

        timing.queue_frame(Some(4), start + Duration::from_millis(60));
        let sample = timing
            .begin_redraw(start + Duration::from_millis(65))
            .unwrap();

        assert_eq!(sample.superseded_frames, 0);
    }

    #[test]
    fn clearing_video_resets_timing_for_reused_pts() {
        let start = Instant::now();
        let mut timing = VideoRedrawTiming::default();
        timing.queue_frame(Some(1), start);
        timing.begin_redraw(start + Duration::from_millis(5));
        timing.queue_frame(None, start + Duration::from_millis(10));
        timing.queue_frame(Some(1), start + Duration::from_millis(20));
        let sample = timing
            .begin_redraw(start + Duration::from_millis(25))
            .unwrap();

        assert_eq!(sample.queue_delay, Duration::from_millis(5));
        assert_eq!(sample.redraw_interval, None);
        assert_eq!(sample.superseded_frames, 0);
    }
}
