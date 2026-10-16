use std::sync::PoisonError;

use anyhow::Result;

use crate::{
    clock::SharedPlayoutClock,
    utils::pipeline_queue::{POLL_INTERVAL, PipelineControl},
};

/// FFmpeg CLI-style file read governor. The slowest selected input stream
/// determines progress; 500 ms of media may be read ahead. Both this governor
/// and mux deadlines reference the same persistent output clock.
pub(super) struct ReadRate {
    clock: Option<SharedPlayoutClock>,
    base_us: i64,
    source_start_us: i64,
    first: [Option<i64>; 2],
    progress: [Option<i64>; 2],
    catchup: Option<(i64, i64)>,
    previous_lag: i64,
    pub(super) initial_burst_us: i64,
}

impl ReadRate {
    pub(super) fn new(
        clock: Option<SharedPlayoutClock>,
        base_us: i64,
        source_start_us: i64,
    ) -> Self {
        Self {
            clock,
            base_us,
            source_start_us,
            first: [None; 2],
            progress: [None; 2],
            catchup: None,
            previous_lag: 0,
            initial_burst_us: 500_000,
        }
    }

    pub(super) fn finish(&mut self, track: usize) {
        self.progress[track] = None;
    }

    pub(super) fn allowed_progress(&mut self, now: i64, absolute_progress: i64) -> i64 {
        let allowed = now.saturating_add(self.initial_burst_us);
        let lag = allowed.saturating_sub(absolute_progress).max(0);

        if (self.previous_lag == 0 && lag > 300_000)
            || lag > self.previous_lag.saturating_add(300_000)
        {
            self.catchup = Some((now, absolute_progress));
            self.previous_lag = lag;
        }

        if lag == 0 {
            self.catchup = None;
            self.previous_lag = 0;
        }

        self.catchup.map_or(allowed, |(wall, media)| {
            allowed.min(media.saturating_add((now.saturating_sub(wall) as f64 * 1.05) as i64))
        })
    }

    pub(super) fn update(
        &mut self,
        stream: usize,
        timestamp_us: Option<i64>,
        control: &PipelineControl,
    ) -> Result<()> {
        let Some(timestamp) = timestamp_us else {
            return Ok(());
        };
        let first = *self.first[stream].get_or_insert(timestamp.max(self.source_start_us));
        self.progress[stream] = Some(
            self.progress[stream]
                .unwrap_or(0)
                .max(timestamp.saturating_sub(first)),
        );
        let progress = self.progress.iter().flatten().copied().min().unwrap_or(0);

        if progress <= self.initial_burst_us {
            return Ok(());
        }

        let Some(clock) = self.clock.clone() else {
            return Ok(());
        };

        loop {
            control.check()?;
            let Some(now) = clock
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .media_time_us()
            else {
                return Ok(());
            };
            let absolute_progress = self.base_us.saturating_add(progress);
            let limit = self.allowed_progress(now, absolute_progress);

            if absolute_progress <= limit {
                return Ok(());
            }
            control.wait(POLL_INTERVAL)?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::PlayoutClock;
    use ffmpeg_next::rescale;
    use std::{
        sync::{Arc, Mutex},
        time::{Duration, Instant},
    };
    #[test]
    fn readrate_uses_slowest_stream_with_half_second_initial_lead() {
        let clock = Arc::new(Mutex::new(PlayoutClock::new()));
        clock
            .lock()
            .unwrap()
            .delay_until(Some(4_000_000), rescale::TIME_BASE);
        let mut rate = ReadRate::new(Some(clock), 4_000_000, 100_000_000);
        let control = PipelineControl::default();
        rate.update(0, Some(99_000_000), &control).unwrap();
        rate.update(1, Some(100_000_000), &control).unwrap();
        let start = Instant::now();
        rate.update(0, Some(100_600_000), &control).unwrap();
        assert!(start.elapsed() < Duration::from_millis(50));
        rate.update(1, Some(100_600_000), &control).unwrap();
        assert!(start.elapsed() >= Duration::from_millis(90));
        assert!(start.elapsed() < Duration::from_millis(300));
    }

    #[test]
    fn readrate_catches_up_at_five_percent_without_resetting_program_time() {
        let mut rate = ReadRate::new(None, 0, 0);
        assert_eq!(rate.allowed_progress(1_000_000, 600_000), 600_000);
        assert_eq!(rate.allowed_progress(2_000_000, 1_650_000), 1_650_000);
        assert_eq!(rate.allowed_progress(3_000_000, 2_700_000), 2_700_000);
        rate.allowed_progress(4_000_000, 4_500_000);
        assert!(rate.catchup.is_none());
    }
}
