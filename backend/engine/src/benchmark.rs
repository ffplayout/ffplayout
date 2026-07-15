#[derive(Clone, Copy)]
pub(crate) enum Stage {
    Decode,
    Scale,
    LogoOverlay,
    TextStatic,
    TextRuntime,
    Vtt,
    EncodeMux,
}

#[cfg(feature = "processing-bench")]
impl Stage {
    const COUNT: usize = 7;

    const fn index(self) -> usize {
        match self {
            Self::Decode => 0,
            Self::Scale => 1,
            Self::LogoOverlay => 2,
            Self::TextStatic => 3,
            Self::TextRuntime => 4,
            Self::Vtt => 5,
            Self::EncodeMux => 6,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Decode => "decode",
            Self::Scale => "scale",
            Self::LogoOverlay => "logo",
            Self::TextStatic => "text_static",
            Self::TextRuntime => "text_runtime",
            Self::Vtt => "vtt",
            Self::EncodeMux => "encode_mux",
        }
    }

    const ALL: [Self; Self::COUNT] = [
        Self::Decode,
        Self::Scale,
        Self::LogoOverlay,
        Self::TextStatic,
        Self::TextRuntime,
        Self::Vtt,
        Self::EncodeMux,
    ];
}

#[cfg(feature = "processing-bench")]
mod enabled {
    use std::{
        cell::RefCell,
        fmt::Write,
        sync::atomic::{AtomicU64, Ordering},
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

    struct ProcessingBench {
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
                "\n    stage           total  share      avg      max  calls       size\n",
            );
            let mut has_stages = false;
            for stage in Stage::ALL {
                let stats = self.stats[stage.index()];
                if stats.calls == 0 {
                    continue;
                }

                let average_ms = stats.total.as_secs_f64() * 1_000.0 / stats.calls as f64;
                let max_ms = stats.max.as_secs_f64() * 1_000.0;
                let share = stats.total.as_secs_f64() / measured.as_secs_f64() * 100.0;
                let overlay_size = stats.overlay_size();
                let _ = write!(
                    stages,
                    "    {:<12} {:>7.3}s {:>5.1}% {:>6.2}ms {:>6.2}ms {:>6} {:>10}\n",
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
                    "[CPU Bench]\n    interval={:.1}s\n    runtime={:.1}s\n    measured={:.3}s{}",
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

    thread_local! {
        static BENCH: RefCell<Option<ProcessingBench>> = const { RefCell::new(None) };
    }

    pub(crate) fn start(channel_id: Option<i32>) {
        BENCH.with(|bench| *bench.borrow_mut() = Some(ProcessingBench::new(channel_id)));
    }

    pub(crate) fn set_report_interval(interval: Duration) {
        let milliseconds = u64::try_from(interval.as_millis())
            .unwrap_or(u64::MAX)
            .max(1);
        REPORT_INTERVAL_MILLIS.store(milliseconds, Ordering::Relaxed);
    }

    pub(crate) fn finish() {
        BENCH.with(|bench| {
            if let Some(bench) = bench.borrow_mut().as_mut() {
                bench.report(true);
            }
        });
    }

    pub(crate) fn measure<T>(stage: Stage, operation: impl FnOnce() -> T) -> T {
        let started_at = Instant::now();
        let result = operation();
        let elapsed = started_at.elapsed();

        BENCH.with(|bench| {
            if let Some(bench) = bench.borrow_mut().as_mut() {
                bench.record(stage, elapsed, None);
                if bench.due() {
                    bench.report(false);
                }
            }
        });

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
            BENCH.with(|bench| {
                if let Some(bench) = bench.borrow_mut().as_mut() {
                    bench.record(stage, elapsed, None);
                    if bench.due() {
                        bench.report(false);
                    }
                }
            });
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

        BENCH.with(|bench| {
            if let Some(bench) = bench.borrow_mut().as_mut() {
                bench.record(stage, elapsed, Some((width, height)));
                if bench.due() {
                    bench.report(false);
                }
            }
        });

        result
    }
}

#[cfg(feature = "processing-bench")]
pub(crate) use enabled::{
    finish, measure, measure_overlay, measure_success, set_report_interval, start,
};

#[cfg(not(feature = "processing-bench"))]
#[inline]
pub(crate) fn start(_channel_id: Option<i32>) {}

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
