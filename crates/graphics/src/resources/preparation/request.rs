//! Cancellation and weak-result request lifetime; independent of worker scheduling.
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Default)]
pub struct Cancellation(Arc<AtomicBool>);

impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub(super) fn reset(&self) {
        self.0.store(false, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

pub(super) struct Completion<T> {
    pub(super) cancelled: Cancellation,
    pub(super) ready: OnceLock<std::result::Result<T, String>>,
}

/// Dropping a request invalidates queued/running work without waiting for it.
pub struct Preparation<T>(pub(super) Arc<Completion<T>>);

impl<T> Preparation<T> {
    pub fn result(&self) -> Option<&std::result::Result<T, String>> {
        self.0.ready.get()
    }
}

impl<T> Drop for Preparation<T> {
    fn drop(&mut self) {
        self.0.cancelled.cancel();
    }
}
