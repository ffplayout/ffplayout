//! One bounded executor for graphics preparation and compositing. Never queue
//! one channel behind another: callers without admission keep working serially.
use std::sync::{
    OnceLock,
    atomic::{AtomicBool, Ordering},
};

use rayon::{ThreadPool, ThreadPoolBuilder};

static POOL: OnceLock<Option<ThreadPool>> = OnceLock::new();
static BUSY: AtomicBool = AtomicBool::new(false);

pub(super) struct PoolPermit<'a>(&'a AtomicBool);

impl<'a> PoolPermit<'a> {
    pub(super) fn acquire(flag: &'a AtomicBool) -> Option<Self> {
        flag.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()
            .map(|_| Self(flag))
    }
}

impl Drop for PoolPermit<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

pub(super) struct SharedPool {
    pub pool: &'static ThreadPool,
    _permit: PoolPermit<'static>,
}

pub(super) fn try_shared() -> Option<SharedPool> {
    let permit = PoolPermit::acquire(&BUSY)?;
    let pool = pool()?;

    Some(SharedPool {
        pool,
        _permit: permit,
    })
}

pub(super) fn pool() -> Option<&'static ThreadPool> {
    POOL.get_or_init(|| {
        let threads = std::thread::available_parallelism()
            .map_or(1, usize::from)
            .min(4);

        if threads < 2 {
            return None;
        }

        match ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|i| format!("graphics-{i}"))
            .build()
        {
            Ok(pool) => Some(pool),
            Err(error) => {
                log::warn!("graphics worker pool unavailable; using serial processing: {error}");
                None
            }
        }
    })
    .as_ref()
}
