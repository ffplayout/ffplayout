#[derive(Clone, Copy)]
pub(crate) enum Stage {
    Decode,
    Scale,
    LogoOverlay,
    TextOverlay,
    Vtt,
    EncodeMux,
}

#[cfg(feature = "processing-bench")]
impl Stage {
    const COUNT: usize = 6;

    const fn index(self) -> usize {
        match self {
            Self::Decode => 0,
            Self::Scale => 1,
            Self::LogoOverlay => 2,
            Self::TextOverlay => 3,
            Self::Vtt => 4,
            Self::EncodeMux => 5,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Decode => "decode",
            Self::Scale => "scale",
            Self::LogoOverlay => "logo",
            Self::TextOverlay => "text",
            Self::Vtt => "vtt",
            Self::EncodeMux => "encode_mux",
        }
    }

    const ALL: [Self; Self::COUNT] = [
        Self::Decode,
        Self::Scale,
        Self::LogoOverlay,
        Self::TextOverlay,
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
    }

    impl StageStats {
        fn record(&mut self, elapsed: Duration) {
            self.calls += 1;
            self.total += elapsed;
            self.max = self.max.max(elapsed);
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

        fn record(&mut self, stage: Stage, elapsed: Duration) {
            self.stats[stage.index()].record(elapsed);
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
                "\n    stage              total   share      average      maximum    calls\n",
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
                let _ = write!(
                    stages,
                    "    {:<12} {:>10.3}s {:>6.1}% {:>10.2}ms {:>10.2}ms {:>8}\n",
                    stage.name(),
                    stats.total.as_secs_f64(),
                    share,
                    average_ms,
                    max_ms,
                    stats.calls
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
                bench.record(stage, elapsed);
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
                    bench.record(stage, elapsed);
                    if bench.due() {
                        bench.report(false);
                    }
                }
            });
        }

        result
    }
}

#[cfg(feature = "processing-bench")]
pub(crate) use enabled::{finish, measure, measure_success, set_report_interval, start};

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
pub(crate) fn measure_success<T, E>(
    _stage: Stage,
    operation: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    operation()
}
