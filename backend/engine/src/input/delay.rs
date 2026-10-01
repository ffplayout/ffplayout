use std::{
    collections::VecDeque,
    sync::mpsc::{Receiver, TryRecvError},
    time::{Duration, Instant},
};

use ffmpeg_next::frame;

use crate::audio_mixer::{TRUE_PEAK_FUTURE_SAMPLES, lookahead_samples};

use super::live::{LIVE_AUDIO_FRAME_SAMPLES, LiveEvent};

// Allow a full latency window plus headroom for interleaving and decoder bursts.
// A fixed frame count is too small for longer delays and causes periodic stalls.
const DELAY_EVENT_HEADROOM: usize = 32;

pub(super) struct LiveDelay {
    latency: Duration,
    max_events: usize,
    sample_rate: u32,
    audio_lookahead: bool,
    events: VecDeque<(Instant, LiveEvent)>,
}

impl LiveDelay {
    pub(super) fn set_audio_lookahead(&mut self, enabled: bool) {
        self.audio_lookahead = enabled;
    }

    pub(super) fn future_audio(&self, session_id: u64) -> impl Iterator<Item = &frame::Audio> {
        self.events
            .iter()
            .take_while(move |(_, event)| {
                !matches!(event, LiveEvent::Started { .. } | LiveEvent::Ended(_))
            })
            .filter_map(move |(_, event)| match event {
                LiveEvent::Audio(id, audio) if *id == session_id => Some(audio),
                _ => None,
            })
    }

    pub(super) fn new(latency: Duration, fps: u32, sample_rate: u32) -> Self {
        let events_per_second =
            f64::from(fps) + f64::from(sample_rate) / LIVE_AUDIO_FRAME_SAMPLES as f64;
        let latency_events = (latency.as_secs_f64() * events_per_second).ceil() as usize;

        Self {
            latency,
            sample_rate,
            audio_lookahead: false,
            max_events: latency_events.saturating_add(DELAY_EVENT_HEADROOM),
            events: VecDeque::new(),
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub(super) fn has_waiting_audio(&self, session_id: u64, now: Instant) -> bool {
        self.events.iter().any(|(received_at, event)| {
            now.saturating_duration_since(*received_at) < self.latency
                && matches!(event, LiveEvent::Audio(id, _) if *id == session_id)
        })
    }

    /// All events share the same minimum hold time and retain their order.
    /// Missing audio preview must not block due video in the same session.
    /// Disconnection cannot discard the buffered tail.
    pub(super) fn next_event(
        &mut self,
        receiver: &Receiver<LiveEvent>,
        now: Instant,
    ) -> Result<LiveEvent, TryRecvError> {
        if self.latency.is_zero() {
            return receiver.try_recv();
        }

        let mut receive_error = TryRecvError::Empty;

        // Refill even when the oldest event is ready. Otherwise paced output
        // drains the entire initial window before reading more frames, assigns
        // those frames fresh deadlines, and leaves another latency-sized gap.
        while self.events.len() < self.max_events {
            match receiver.try_recv() {
                Ok(event) => self.events.push_back((now, event)),
                Err(error) => {
                    receive_error = error;
                    break;
                }
            }
        }

        if self.events.front().is_some_and(|(received_at, _)| {
            now.saturating_duration_since(*received_at) >= self.latency
        }) {
            if self.audio_lookahead
                && !matches!(receive_error, TryRecvError::Disconnected)
                && let Some((received_at, LiveEvent::Audio(session_id, audio))) =
                    self.events.front()
                && now.saturating_duration_since(*received_at)
                    < self.latency + Duration::from_secs_f64(super::live::LIVE_AUDIO_GRACE_SECONDS)
            {
                let needed = lookahead_samples(self.sample_rate) + TRUE_PEAK_FUTURE_SAMPLES;
                let preview = collect_audio_preview(
                    self.future_audio(*session_id).skip(1),
                    audio.pts().unwrap_or(0) + audio.samples() as i64,
                    self.sample_rate,
                    needed,
                );
                let session_ended = self.events.iter().any(|(_, event)| {
                    matches!(event, LiveEvent::Ended(id) if id == session_id)
                        || matches!(event, LiveEvent::Started { .. })
                });

                if preview.len() < needed && !session_ended {
                    // Release the available audio before due video, even if
                    // preview is incomplete. Holding it while video advances
                    // would deliver stale audio later. Retaining FIFO order also
                    // keeps session boundaries and both PTS timelines intact.
                    let video_due = self
                        .events
                        .iter()
                        .skip(1)
                        .take_while(|(_, event)| {
                            !matches!(event, LiveEvent::Started { .. } | LiveEvent::Ended(_))
                        })
                        .any(|(received_at, event)| {
                            matches!(event, LiveEvent::Video(id, _) if id == session_id)
                                && now.saturating_duration_since(*received_at) >= self.latency
                        });

                    if !video_due {
                        return Err(TryRecvError::Empty);
                    }
                }
            }

            return Ok(self.events.pop_front().unwrap().1);
        }

        Err(if self.events.is_empty() {
            receive_error
        } else {
            TryRecvError::Empty
        })
    }
}

/// Collect contiguous source audio without crossing a timestamp gap. A gap
/// contributes silence; overlapping samples are skipped, just as at output.
pub(super) fn collect_audio_preview<'a>(
    frames: impl Iterator<Item = &'a frame::Audio>,
    mut expected_pts: i64,
    sample_rate: u32,
    limit: usize,
) -> Vec<[f32; 2]> {
    let mut samples = Vec::with_capacity(limit);
    let jitter =
        (f64::from(sample_rate) * super::live::LIVE_AUDIO_PTS_JITTER_SECONDS).ceil() as u64;

    for frame in frames {
        if frame.planes() != 2 {
            break;
        }

        let pts = frame.pts().unwrap_or(expected_pts);
        let difference = pts.saturating_sub(expected_pts);
        let skip = if pts.abs_diff(expected_pts) <= jitter {
            0
        } else if difference > 0 {
            let gap = (difference as usize).min(limit - samples.len());
            samples.resize(samples.len() + gap, [0.0; 2]);
            expected_pts += gap as i64;
            0
        } else {
            (difference.unsigned_abs() as usize).min(frame.samples())
        };

        for index in skip..frame.samples() {
            if samples.len() == limit {
                break;
            }

            samples.push([frame.plane::<f32>(0)[index], frame.plane::<f32>(1)[index]]);
            expected_pts += 1;
        }

        if samples.len() == limit {
            break;
        }
    }

    samples
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use ffmpeg_next::frame;

    use super::*;

    #[test]
    fn audio_video_and_session_boundaries_share_the_delay_and_keep_pts() {
        let (tx, rx) = mpsc::channel();
        let mut delay = LiveDelay::new(Duration::from_millis(10), 25, 48_000);
        let start = Instant::now();
        let mut video = frame::Video::empty();
        video.set_pts(Some(25));
        let mut audio = frame::Audio::empty();
        audio.set_pts(Some(48_000));
        tx.send(LiveEvent::Started {
            session_id: 1,
            has_audio: true,
            listener_id: 0,
        })
        .unwrap();
        tx.send(LiveEvent::Video(1, video)).unwrap();
        tx.send(LiveEvent::Audio(1, audio)).unwrap();
        tx.send(LiveEvent::Ended(1)).unwrap();
        drop(tx);

        assert!(matches!(
            delay.next_event(&rx, start),
            Err(TryRecvError::Empty)
        ));
        assert!(delay.has_waiting_audio(1, start));
        assert!(!delay.has_waiting_audio(2, start));
        assert!(matches!(
            delay.next_event(&rx, start + Duration::from_millis(9)),
            Err(TryRecvError::Empty)
        ));
        let ready = start + Duration::from_millis(10);
        assert!(!delay.has_waiting_audio(1, ready));

        assert!(matches!(
            delay.next_event(&rx, ready),
            Ok(LiveEvent::Started { session_id: 1, .. })
        ));
        assert!(
            matches!(delay.next_event(&rx, ready), Ok(LiveEvent::Video(1, frame)) if frame.pts() == Some(25))
        );
        assert!(
            matches!(delay.next_event(&rx, ready), Ok(LiveEvent::Audio(1, frame)) if frame.pts() == Some(48_000))
        );
        assert!(matches!(
            delay.next_event(&rx, ready),
            Ok(LiveEvent::Ended(1))
        ));
        assert!(matches!(
            delay.next_event(&rx, ready),
            Err(TryRecvError::Disconnected)
        ));
        assert!(delay.is_empty());
    }

    #[test]
    fn queue_is_bounded_and_preserves_session_order_under_backpressure() {
        let (tx, rx) = mpsc::channel();
        let mut delay = LiveDelay::new(Duration::from_secs(2), 25, 48_000);
        let start = Instant::now();

        for id in 0..delay.max_events + 1 {
            tx.send(LiveEvent::Ended(id as u64)).unwrap();
        }

        assert!(matches!(
            delay.next_event(&rx, start),
            Err(TryRecvError::Empty)
        ));
        assert_eq!(delay.events.len(), delay.max_events);
        let ready = start + Duration::from_secs(2);

        for id in 0..delay.max_events {
            assert!(
                matches!(delay.next_event(&rx, ready), Ok(LiveEvent::Ended(actual)) if actual == id as u64)
            );
        }

        assert!(matches!(
            delay.next_event(&rx, ready),
            Err(TryRecvError::Empty)
        ));
        assert!(
            matches!(delay.next_event(&rx, ready + Duration::from_secs(2)), Ok(LiveEvent::Ended(actual)) if actual == delay.max_events as u64)
        );
    }

    #[test]
    fn one_second_delay_streams_continuously_while_output_is_paced() {
        let (tx, rx) = mpsc::channel();
        let mut delay = LiveDelay::new(Duration::from_secs(1), 25, 48_000);
        let start = Instant::now();
        let mut video_count = 0;
        let mut audio_count = 0;
        let mut max_queued = 0;

        // Feed 25 fps video and 20 ms audio blocks for ten seconds. Advance
        // the output clock by one tick between reads, as desktop playback does.
        for tick in 0..500 {
            let now = start + Duration::from_millis(tick * 20);

            if tick % 2 == 0 {
                let mut video = frame::Video::empty();
                video.set_pts(Some((tick / 2) as i64));
                tx.send(LiveEvent::Video(1, video)).unwrap();
            }

            let mut audio = frame::Audio::empty();
            audio.set_pts(Some((tick * 960) as i64));
            tx.send(LiveEvent::Audio(1, audio)).unwrap();

            while let Ok(event) = delay.next_event(&rx, now) {
                match event {
                    LiveEvent::Video(_, video) => {
                        assert_eq!(video.pts(), Some(video_count));
                        video_count += 1;
                    }
                    LiveEvent::Audio(_, audio) => {
                        assert_eq!(audio.pts(), Some(audio_count * 960));
                        audio_count += 1;
                    }
                    _ => panic!("unexpected session event"),
                }
            }

            max_queued = max_queued.max(delay.events.len());

            if tick < 50 {
                assert_eq!(video_count, 0);
                assert_eq!(audio_count, 0);
            } else {
                assert_eq!(video_count, ((tick - 50) / 2 + 1) as i64);
                assert_eq!(audio_count, (tick - 50 + 1) as i64);
            }
        }

        assert!(max_queued < delay.max_events);
        assert!(
            max_queued <= 75,
            "buffer must not grow across playback cycles"
        );
        assert!(matches!(rx.try_recv(), Err(TryRecvError::Empty)));
    }

    #[test]
    fn lookahead_waits_for_future_audio_and_releases_the_tail_at_session_end() {
        let (tx, rx) = mpsc::channel();
        let mut delay = LiveDelay::new(Duration::from_millis(10), 25, 48_000);
        delay.set_audio_lookahead(true);
        let start = Instant::now();
        let make_audio = |pts| {
            let mut audio = frame::Audio::new(
                ffmpeg_next::format::Sample::F32(ffmpeg_next::format::sample::Type::Planar),
                1_024,
                ffmpeg_next::ChannelLayout::STEREO,
            );
            audio.set_rate(48_000);
            audio.set_pts(Some(pts));

            for channel in 0..2 {
                audio.plane_mut::<f32>(channel).fill(0.25);
            }

            audio
        };
        tx.send(LiveEvent::Audio(1, make_audio(0))).unwrap();

        assert!(matches!(
            delay.next_event(&rx, start),
            Err(TryRecvError::Empty)
        ));
        let ready = start + Duration::from_millis(10);

        assert!(matches!(
            delay.next_event(&rx, ready),
            Err(TryRecvError::Empty)
        ));
        tx.send(LiveEvent::Video(1, frame::Video::empty())).unwrap();
        tx.send(LiveEvent::Audio(1, make_audio(1_024))).unwrap();
        assert!(
            matches!(delay.next_event(&rx, ready), Ok(LiveEvent::Audio(1, frame)) if frame.pts() == Some(0))
        );
        let preview = collect_audio_preview(delay.future_audio(1), 1_024, 48_000, 486);
        assert_eq!(preview, vec![[0.25; 2]; 486]);
        let next = ready + Duration::from_millis(10);

        assert!(matches!(
            delay.next_event(&rx, next),
            Ok(LiveEvent::Video(1, _))
        ));
        assert!(matches!(
            delay.next_event(&rx, next),
            Err(TryRecvError::Empty)
        ));
        tx.send(LiveEvent::Ended(1)).unwrap();
        assert!(
            matches!(delay.next_event(&rx, next), Ok(LiveEvent::Audio(1, frame)) if frame.pts() == Some(1_024))
        );
    }

    #[test]
    fn missing_preview_releases_audio_before_due_video_without_bypassing_latency() {
        let (tx, rx) = mpsc::channel();
        let mut delay = LiveDelay::new(Duration::from_millis(10), 25, 48_000);
        delay.set_audio_lookahead(true);
        let start = Instant::now();
        let mut audio = frame::Audio::new(
            ffmpeg_next::format::Sample::F32(ffmpeg_next::format::sample::Type::Planar),
            1_024,
            ffmpeg_next::ChannelLayout::STEREO,
        );
        audio.set_pts(Some(0));
        tx.send(LiveEvent::Audio(1, audio)).unwrap();
        assert!(matches!(
            delay.next_event(&rx, start),
            Err(TryRecvError::Empty)
        ));

        for pts in 0..25 {
            let received = start + Duration::from_millis(5 + pts * 40);
            let mut video = frame::Video::empty();
            video.set_pts(Some(pts as i64));
            tx.send(LiveEvent::Video(1, video)).unwrap();
            assert!(matches!(
                delay.next_event(&rx, received),
                Err(TryRecvError::Empty)
            ));
            assert!(matches!(
                delay.next_event(&rx, received + Duration::from_millis(9)),
                Err(TryRecvError::Empty)
            ));
            let due = received + Duration::from_millis(10);

            if pts == 0 {
                // The tail is delivered before video advances, not held for
                // 2.5 seconds and then emitted against an older audio timeline.
                assert!(
                    matches!(delay.next_event(&rx, due), Ok(LiveEvent::Audio(1, audio)) if audio.pts() == Some(0) && audio.samples() == 1_024)
                );
            }

            assert!(
                matches!(delay.next_event(&rx, due), Ok(LiveEvent::Video(1, video)) if video.pts() == Some(pts as i64))
            );
        }

        assert!(delay.is_empty());
    }

    #[test]
    fn missing_future_audio_does_not_block_dropout_recovery_forever() {
        let (tx, rx) = mpsc::channel();
        let mut delay = LiveDelay::new(Duration::from_millis(10), 25, 48_000);
        delay.set_audio_lookahead(true);
        let start = Instant::now();
        tx.send(LiveEvent::Audio(1, frame::Audio::empty())).unwrap();

        assert!(matches!(
            delay.next_event(&rx, start),
            Err(TryRecvError::Empty)
        ));
        assert!(matches!(
            delay.next_event(&rx, start + Duration::from_secs(3)),
            Ok(LiveEvent::Audio(1, _))
        ));
    }

    #[test]
    fn zero_latency_passes_events_through_immediately() {
        let (tx, rx) = mpsc::channel();
        let mut delay = LiveDelay::new(Duration::ZERO, 25, 48_000);
        tx.send(LiveEvent::Ended(1)).unwrap();

        assert!(matches!(
            delay.next_event(&rx, Instant::now()),
            Ok(LiveEvent::Ended(1))
        ));
        assert!(delay.is_empty());
    }
}
