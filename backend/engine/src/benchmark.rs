#[derive(Clone, Copy)]
pub(crate) enum Stage {
    VideoDecode,
    AudioDecode,
    AudioProcess,
    AudioEncode,
    Scale,
    LogoOverlay,
    TextStatic,
    TextRuntime,
    Vtt,
    EncodeMux,
    #[cfg(feature = "desktop")]
    DesktopOutput,
    LiveQueue,
}

#[cfg(feature = "processing-bench")]
impl Stage {
    const ALL: &[Self] = &[
        Self::VideoDecode,
        Self::AudioDecode,
        Self::AudioProcess,
        Self::AudioEncode,
        Self::Scale,
        Self::LogoOverlay,
        Self::TextStatic,
        Self::TextRuntime,
        Self::Vtt,
        Self::EncodeMux,
        #[cfg(feature = "desktop")]
        Self::DesktopOutput,
        Self::LiveQueue,
    ];

    const COUNT: usize = Self::ALL.len();

    const fn index(self) -> usize {
        match self {
            Self::VideoDecode => 0,
            Self::AudioDecode => 1,
            Self::AudioProcess => 2,
            Self::AudioEncode => 3,
            Self::Scale => 4,
            Self::LogoOverlay => 5,
            Self::TextStatic => 6,
            Self::TextRuntime => 7,
            Self::Vtt => 8,
            Self::EncodeMux => 9,
            #[cfg(feature = "desktop")]
            Self::DesktopOutput => 10,
            Self::LiveQueue => Self::COUNT - 1,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::VideoDecode => "video_decode",
            Self::AudioDecode => "audio_decode",
            Self::AudioProcess => "audio_process",
            Self::AudioEncode => "audio_encode",
            Self::Scale => "scale",
            Self::LogoOverlay => "logo",
            Self::TextStatic => "text_static",
            Self::TextRuntime => "text_runtime",
            Self::Vtt => "vtt",
            Self::EncodeMux => "encode_mux",
            #[cfg(feature = "desktop")]
            Self::DesktopOutput => "desktop_out",
            Self::LiveQueue => "live_queue",
        }
    }
}

#[cfg(feature = "processing-bench")]
mod enabled {
    use std::{
        cell::RefCell,
        fmt::Write,
        sync::{
            Arc, Mutex,
            atomic::{AtomicU64, Ordering},
        },
        time::{Duration, Instant},
    };

    use log::info;

    use super::Stage;

    const DEFAULT_REPORT_INTERVAL: Duration = Duration::from_secs(1);
    static REPORT_INTERVAL_MILLIS: AtomicU64 =
        AtomicU64::new(DEFAULT_REPORT_INTERVAL.as_millis() as u64);

    fn report_interval() -> Duration {
        Duration::from_millis(REPORT_INTERVAL_MILLIS.load(Ordering::Relaxed))
    }

    #[derive(Clone, Copy, Default)]
    struct StageStats {
        calls: u64,
        total: Duration,
        max: Duration,
        overlay_size: Option<(u32, u32)>,
        overlay_size_varies: bool,
    }

    impl StageStats {
        fn record(&mut self, elapsed: Duration, overlay_size: Option<(u32, u32)>) {
            self.calls += 1;
            self.total += elapsed;
            self.max = self.max.max(elapsed);

            if let Some(overlay_size) = overlay_size {
                if let Some(previous_size) = self.overlay_size
                    && previous_size != overlay_size
                {
                    self.overlay_size_varies = true;
                } else {
                    self.overlay_size = Some(overlay_size);
                }
            }
        }

        fn overlay_size(&self) -> String {
            match (self.overlay_size, self.overlay_size_varies) {
                (Some((width, height)), false) => format!("{width}x{height}"),
                (Some(_), true) => "varied".to_string(),
                (None, _) => "-".to_string(),
            }
        }
    }

    pub(crate) struct ProcessingBench {
        channel_id: i32,
        started_at: Instant,
        last_report_at: Instant,
        stats: [StageStats; Stage::COUNT],
    }

    impl ProcessingBench {
        fn new(channel_id: Option<i32>) -> Self {
            let now = Instant::now();
            Self {
                channel_id: channel_id.unwrap_or_default(),
                started_at: now,
                last_report_at: now,
                stats: [StageStats::default(); Stage::COUNT],
            }
        }

        fn record(&mut self, stage: Stage, elapsed: Duration, overlay_size: Option<(u32, u32)>) {
            self.stats[stage.index()].record(elapsed, overlay_size);
        }

        fn due(&self) -> bool {
            self.last_report_at.elapsed() >= report_interval()
        }

        fn report(&mut self, final_report: bool) {
            let elapsed = self.last_report_at.elapsed();
            if !final_report && elapsed < report_interval() {
                return;
            }

            let measured = self
                .stats
                .iter()
                .fold(Duration::ZERO, |total, stats| total + stats.total);
            let mut stages = String::from(
                "\n    <span class=\"log-bold\">stage               total  share      avg      max  calls       size</span>\n",
            );
            let mut has_stages = false;
            for &stage in Stage::ALL {
                let stats = self.stats[stage.index()];
                if stats.calls == 0 {
                    continue;
                }

                let average_ms = stats.total.as_secs_f64() * 1_000.0 / stats.calls as f64;
                let max_ms = stats.max.as_secs_f64() * 1_000.0;
                let share = stats.total.as_secs_f64() / measured.as_secs_f64() * 100.0;
                let overlay_size = stats.overlay_size();
                let _ = writeln!(
                    stages,
                    "    {:<16} <span class=\"log-number\">{:>7.3}s</span> <span class=\"log-number\">{:>5.1}%</span> <span class=\"log-number\">{:>6.2}ms</span> <span class=\"log-number\">{:>6.2}ms</span> <span class=\"log-number\">{:>6}</span> <span class=\"log-number\">{:>10}</span>",
                    stage.name(),
                    stats.total.as_secs_f64(),
                    share,
                    average_ms,
                    max_ms,
                    stats.calls,
                    overlay_size
                );
                has_stages = true;
            }

            if has_stages {
                info!(channel = self.channel_id;
                    "<span class=\"log-gray\">[CPU Bench]</span>\n    interval=<span class=\"log-number\">{:.1}s</span>\n    runtime=<span class=\"log-number\">{:.1}s</span>\n    measured=<span class=\"log-number\">{:.3}s</span>{}",
                    elapsed.as_secs_f64(),
                    self.started_at.elapsed().as_secs_f64(),
                    measured.as_secs_f64(),
                    stages
                );
            }

            self.last_report_at = Instant::now();
            self.stats = [StageStats::default(); Stage::COUNT];
        }
    }

    pub(crate) type BenchHandle = Arc<Mutex<ProcessingBench>>;

    thread_local! {
        static BENCH: RefCell<Option<BenchHandle>> = const { RefCell::new(None) };
    }

    pub(crate) fn start(channel_id: Option<i32>) -> BenchHandle {
        let handle = Arc::new(Mutex::new(ProcessingBench::new(channel_id)));
        BENCH.with(|bench| *bench.borrow_mut() = Some(handle.clone()));
        handle
    }

    pub(crate) fn activate(handle: BenchHandle) {
        BENCH.with(|bench| *bench.borrow_mut() = Some(handle));
    }

    #[cfg(feature = "tokio")]
    pub(crate) fn current() -> Option<BenchHandle> {
        BENCH.with(|bench| bench.borrow().as_ref().cloned())
    }

    pub(crate) fn set_report_interval(interval: Duration) {
        let milliseconds = u64::try_from(interval.as_millis())
            .unwrap_or(u64::MAX)
            .max(1);
        REPORT_INTERVAL_MILLIS.store(milliseconds, Ordering::Relaxed);
    }

    pub(crate) fn finish() {
        BENCH.with(|bench| {
            if let Some(handle) = bench.borrow_mut().take()
                && let Ok(mut bench) = handle.lock()
            {
                bench.report(true);
            }
        });
    }

    #[cfg(feature = "desktop")]
    pub(crate) fn detach() {
        BENCH.with(|bench| {
            bench.borrow_mut().take();
        });
    }

    fn record(stage: Stage, elapsed: Duration, overlay_size: Option<(u32, u32)>) {
        BENCH.with(|bench| {
            let handle = bench.borrow().as_ref().cloned();
            if let Some(handle) = handle
                && let Ok(mut bench) = handle.lock()
            {
                bench.record(stage, elapsed, overlay_size);
                if bench.due() {
                    bench.report(false);
                }
            }
        });
    }

    pub(crate) fn measure<T>(stage: Stage, operation: impl FnOnce() -> T) -> T {
        let started_at = Instant::now();
        let result = operation();
        let elapsed = started_at.elapsed();

        record(stage, elapsed, None);

        result
    }

    pub(crate) fn measure_success<T, E>(
        stage: Stage,
        operation: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E> {
        let started_at = Instant::now();
        let result = operation();

        if result.is_ok() {
            let elapsed = started_at.elapsed();
            record(stage, elapsed, None);
        }

        result
    }

    pub(crate) fn measure_overlay<T>(
        stage: Stage,
        width: u32,
        height: u32,
        operation: impl FnOnce() -> T,
    ) -> T {
        let started_at = Instant::now();
        let result = operation();
        let elapsed = started_at.elapsed();

        record(stage, elapsed, Some((width, height)));

        result
    }
}

#[cfg(feature = "processing-bench")]
pub(crate) use enabled::{
    finish, measure, measure_overlay, measure_success, set_report_interval, start,
};

#[cfg(all(feature = "processing-bench", feature = "tokio"))]
pub(crate) use enabled::current;

#[cfg(feature = "processing-bench")]
pub(crate) use enabled::{BenchHandle, activate};

#[cfg(all(feature = "processing-bench", feature = "desktop"))]
pub(crate) use enabled::detach;

#[cfg(not(feature = "processing-bench"))]
#[derive(Clone)]
pub(crate) struct BenchHandle;

#[cfg(not(feature = "processing-bench"))]
#[inline]
pub(crate) fn start(_channel_id: Option<i32>) -> BenchHandle {
    BenchHandle
}

#[cfg(not(feature = "processing-bench"))]
#[inline]
pub(crate) fn activate(_handle: BenchHandle) {}

#[cfg(all(not(feature = "processing-bench"), feature = "tokio"))]
#[inline]
pub(crate) fn current() -> Option<BenchHandle> {
    None
}

#[cfg(all(not(feature = "processing-bench"), feature = "desktop"))]
#[inline]
pub(crate) fn detach() {}

#[cfg(not(feature = "processing-bench"))]
#[inline]
pub(crate) fn set_report_interval(_interval: std::time::Duration) {}

#[cfg(not(feature = "processing-bench"))]
#[inline]
pub(crate) fn finish() {}

#[cfg(not(feature = "processing-bench"))]
#[inline]
pub(crate) fn measure<T>(_stage: Stage, operation: impl FnOnce() -> T) -> T {
    operation()
}

#[cfg(not(feature = "processing-bench"))]
#[inline]
pub(crate) fn measure_overlay<T>(
    _stage: Stage,
    _width: u32,
    _height: u32,
    operation: impl FnOnce() -> T,
) -> T {
    operation()
}

#[cfg(not(feature = "processing-bench"))]
#[inline]
pub(crate) fn measure_success<T, E>(
    _stage: Stage,
    operation: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    operation()
}
