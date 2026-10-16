//! Optional primary video timing measurements.
use std::time::{Duration, Instant};

use ffmpeg_next::{Rational, Rescale, rescale};
const REPORT_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Default)]
struct Stage {
    previous: Option<Instant>,
    anchor: Option<(Instant, i64)>,
    max_gap: Duration,
    drift_ms: Option<f64>,
}

impl Stage {
    fn record(&mut self, now: Instant, media_us: Option<i64>) {
        if let Some(previous) = self.previous {
            self.max_gap = self.max_gap.max(now.saturating_duration_since(previous));
        }
        self.previous = Some(now);

        if let Some(media_us) = media_us {
            let (wall, media) = *self.anchor.get_or_insert((now, media_us));
            self.drift_ms = Some(
                now.saturating_duration_since(wall).as_secs_f64() * 1000.0
                    - (media_us as f64 - media as f64) / 1000.0,
            );
        }
    }
}

#[derive(Default)]
struct PrimaryWindow {
    frames: u64,
    packets: u64,
    pts_us: Option<i64>,
    input: Stage,
    output: Stage,
    submit_max: Duration,
    drain_max: Duration,
    queue_max: Duration,
    call_max: Duration,
}

/// Measures the primary video path when timing diagnostics are enabled.
pub(in crate::output) struct PrimaryTiming {
    channel: i32,
    reported: Instant,
    window: PrimaryWindow,
    active: bool,
}

impl PrimaryTiming {
    pub(in crate::output) fn new(channel: i32) -> Self {
        Self {
            channel,
            reported: Instant::now(),
            window: PrimaryWindow::default(),
            active: false,
        }
    }

    pub(in crate::output) fn begin(&mut self) {
        self.active = true;
    }

    pub(in crate::output) fn packet(&mut self, queued: Duration) {
        if self.active {
            self.window.packets += 1;
            self.window.queue_max = self.window.queue_max.max(queued);
        }
    }

    pub(in crate::output) fn finish(
        &mut self,
        pts: Option<i64>,
        time_base: Rational,
        entered: Instant,
        submit: Duration,
        drain: Duration,
    ) {
        self.active = false;
        let now = Instant::now();
        let window = &mut self.window;
        window.frames += 1;
        window.pts_us = pts.map(|pts| pts.rescale(time_base, rescale::TIME_BASE));
        window.input.record(entered, window.pts_us);
        window.output.record(now, window.pts_us);
        window.submit_max = window.submit_max.max(submit);
        window.drain_max = window.drain_max.max(drain);
        window.call_max = window.call_max.max(now.saturating_duration_since(entered));
        let interval = now.saturating_duration_since(self.reported);

        if interval < REPORT_INTERVAL {
            return;
        }
        let ms = |duration: Duration| duration.as_secs_f64() * 1000.0;
        let number =
            |value: Option<f64>| value.map_or_else(|| "unavailable".into(), |v| format!("{v:.3}"));
        log::trace!(channel = self.channel;
            "[Preview Timing] stage=primary_video window_s={:.3} frames={} packets={} pts_us={} entry_gap_max_ms={:.3} exit_gap_max_ms={:.3} entry_drift_ms={} exit_drift_ms={} submit_max_ms={:.3} drain_max_ms={:.3} queue_wait_max_ms={:.3} total_call_max_ms={:.3}",
            interval.as_secs_f64(), window.frames, window.packets,
            window.pts_us.map_or_else(|| "unavailable".into(), |v| v.to_string()),
            ms(window.input.max_gap), ms(window.output.max_gap), number(window.input.drift_ms), number(window.output.drift_ms),
            ms(window.submit_max), ms(window.drain_max), ms(window.queue_max), ms(window.call_max),
        );
        // Keep stage anchors and previous times across report boundaries.
        let input = std::mem::take(&mut window.input);
        let output = std::mem::take(&mut window.output);
        *window = PrimaryWindow {
            input,
            output,
            ..Default::default()
        };
        window.input.max_gap = Duration::ZERO;
        window.output.max_gap = Duration::ZERO;
        self.reported = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn primary_metrics_only_count_packets_during_video_calls() {
        let mut timing = PrimaryTiming::new(1);
        timing.packet(Duration::from_millis(500));
        assert_eq!(timing.window.packets, 0);
        timing.begin();
        timing.packet(Duration::from_millis(12));
        timing.finish(
            Some(25),
            Rational(1, 25),
            Instant::now() - Duration::from_millis(20),
            Duration::from_millis(2),
            Duration::from_millis(18),
        );
        timing.packet(Duration::from_secs(1));
        assert_eq!(timing.window.frames, 1);
        assert_eq!(timing.window.packets, 1);
        assert_eq!(timing.window.pts_us, Some(1_000_000));
        assert_eq!(timing.window.submit_max, Duration::from_millis(2));
        assert_eq!(timing.window.drain_max, Duration::from_millis(18));
        assert_eq!(timing.window.queue_max, Duration::from_millis(12));
        assert!(timing.window.call_max >= Duration::from_millis(20));
    }
}
