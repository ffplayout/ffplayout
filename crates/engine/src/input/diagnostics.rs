use std::{
    fmt,
    sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
};

use super::live::monotonic_millis;

const NEVER: u64 = u64::MAX;

#[derive(Clone, Copy)]
#[repr(u8)]
pub(crate) enum LiveInputStage {
    Starting,
    ReadingPacket,
    ProcessingVideo,
    ProcessingAudio,
    WaitingForQueue,
    Finishing,
}

impl LiveInputStage {
    fn label(value: u8) -> &'static str {
        match value {
            0 => "starting",
            1 => "reading packet",
            2 => "processing video",
            3 => "processing audio",
            4 => "waiting for queue",
            5 => "finishing",
            _ => "unknown",
        }
    }
}

/// Diagnostic timestamps do not extend watchdog deadlines. Only frame output
/// and the existing queue heartbeat update `last_activity_ms`, as before.
/// Snapshots span atomics and are approximate, rather than locking the reader.
pub(crate) struct LiveInputDiagnostics {
    pub(crate) listener_id: i32,
    pub(crate) session_id: u64,
    pub(crate) last_activity_ms: AtomicU64,
    pub(crate) frame_seen: AtomicBool,
    activity_source: AtomicU8,
    stage: AtomicU8,
    packet_ms: AtomicU64,
    audio_ms: AtomicU64,
    video_ms: AtomicU64,
    frame_ms: AtomicU64,
    backpressure_ms: AtomicU64,
    queue_wait_ms: AtomicU64,
}

impl LiveInputDiagnostics {
    pub(crate) fn new(listener_id: i32, session_id: u64) -> Self {
        Self {
            listener_id,
            session_id,
            last_activity_ms: AtomicU64::new(monotonic_millis()),
            frame_seen: AtomicBool::new(false),
            activity_source: AtomicU8::new(0),
            stage: AtomicU8::new(LiveInputStage::Starting as u8),
            packet_ms: AtomicU64::new(NEVER),
            audio_ms: AtomicU64::new(NEVER),
            video_ms: AtomicU64::new(NEVER),
            frame_ms: AtomicU64::new(NEVER),
            backpressure_ms: AtomicU64::new(NEVER),
            queue_wait_ms: AtomicU64::new(NEVER),
        }
    }

    pub(crate) fn set_stage(&self, stage: LiveInputStage) {
        self.stage.store(stage as u8, Ordering::Relaxed);
    }

    pub(crate) fn packet_received(&self) {
        self.packet_ms.store(monotonic_millis(), Ordering::Relaxed);
    }

    pub(crate) fn video_decoded(&self) {
        self.video_ms.store(monotonic_millis(), Ordering::Relaxed);
    }

    pub(crate) fn audio_decoded(&self) {
        self.audio_ms.store(monotonic_millis(), Ordering::Relaxed);
    }

    pub(crate) fn frame_ready(&self) {
        let now = monotonic_millis();
        self.frame_ms.store(now, Ordering::Relaxed);
        self.frame_seen.store(true, Ordering::Relaxed);
        self.activity_source.store(1, Ordering::Relaxed);
        self.last_activity_ms.store(now, Ordering::Relaxed);
    }

    pub(crate) fn queue_heartbeat(&self) {
        let now = monotonic_millis();
        let _ =
            self.queue_wait_ms
                .compare_exchange(NEVER, now, Ordering::Relaxed, Ordering::Relaxed);
        self.set_stage(LiveInputStage::WaitingForQueue);
        self.backpressure_ms.store(now, Ordering::Relaxed);
        self.activity_source.store(2, Ordering::Relaxed);
        self.last_activity_ms.store(now, Ordering::Relaxed);
    }

    pub(crate) fn queue_cleared(&self, previous_stage: u8) {
        self.queue_wait_ms.store(NEVER, Ordering::Relaxed);
        self.stage.store(previous_stage, Ordering::Relaxed);
    }

    pub(crate) fn stage(&self) -> u8 {
        self.stage.load(Ordering::Relaxed)
    }

    pub(crate) fn snapshot(&self, now: u64) -> String {
        let age = |timestamp: &AtomicU64| {
            let value = timestamp.load(Ordering::Relaxed);

            ActivityAge((value != NEVER).then(|| now.saturating_sub(value)))
        };
        let activity = match self.activity_source.load(Ordering::Relaxed) {
            1 => "frame",
            2 => "queue heartbeat",
            _ => "startup",
        };

        format!(
            "session={}, stage={}, activity={}, packet_age={}, audio_age={}, video_age={}, frame_age={}, backpressure_age={}, queue_wait={}",
            self.session_id,
            LiveInputStage::label(self.stage()),
            activity,
            age(&self.packet_ms),
            age(&self.audio_ms),
            age(&self.video_ms),
            age(&self.frame_ms),
            age(&self.backpressure_ms),
            age(&self.queue_wait_ms),
        )
    }
}

struct ActivityAge(Option<u64>);

impl fmt::Display for ActivityAge {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(milliseconds) => write!(formatter, "{milliseconds} ms"),
            None => formatter.write_str("none"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_and_decode_tracking_do_not_refresh_the_watchdog() {
        let diagnostics = LiveInputDiagnostics::new(3, 17);
        diagnostics.last_activity_ms.store(42, Ordering::Relaxed);
        diagnostics.packet_received();
        diagnostics.audio_decoded();
        diagnostics.video_decoded();
        diagnostics.set_stage(LiveInputStage::ProcessingVideo);
        assert_eq!(diagnostics.last_activity_ms.load(Ordering::Relaxed), 42);
        assert!(!diagnostics.frame_seen.load(Ordering::Relaxed));
        let snapshot = diagnostics.snapshot(monotonic_millis());
        assert!(snapshot.contains("session=17, stage=processing video, activity=startup"));
        assert!(!snapshot.contains("packet_age=none"));
        assert!(!snapshot.contains("audio_age=none"));
        assert!(!snapshot.contains("video_age=none"));
        assert!(snapshot.contains("frame_age=none"));
    }

    #[test]
    fn queue_heartbeat_is_distinguished_from_real_frames_and_clears_wait_state() {
        let diagnostics = LiveInputDiagnostics::new(3, 17);
        diagnostics.frame_ready();
        diagnostics.set_stage(LiveInputStage::ProcessingAudio);
        let previous_stage = diagnostics.stage();
        diagnostics.queue_heartbeat();
        let blocked = diagnostics.snapshot(monotonic_millis());
        assert!(blocked.contains("stage=waiting for queue, activity=queue heartbeat"));
        assert!(!blocked.contains("queue_wait=none"));
        diagnostics.queue_cleared(previous_stage);
        assert!(
            diagnostics
                .snapshot(monotonic_millis())
                .contains("queue_wait=none")
        );
        diagnostics.frame_ready();
        let recovered = diagnostics.snapshot(monotonic_millis());
        assert!(recovered.contains("stage=processing audio, activity=frame"));
    }
}
