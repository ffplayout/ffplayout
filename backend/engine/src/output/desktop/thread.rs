//! The process-lifetime OS thread dedicated to desktop output.
//!
//! Every desktop-output session (across restarts and channels) is executed
//! as a job on this single dedicated thread, so window and audio resources
//! always live on the same thread for the whole process lifetime. Jobs run one at a time, so only
//! one desktop-output session can be active at once - reasonable, since
//! there is only one local display anyway.

#[cfg(all(feature = "desktop", feature = "tokio"))]
use std::{
    sync::{OnceLock, mpsc},
    thread,
};

#[cfg(all(feature = "desktop", feature = "tokio"))]
type Job = Box<dyn FnOnce() + Send>;

#[cfg(all(feature = "desktop", feature = "tokio"))]
static DESKTOP_THREAD: OnceLock<mpsc::SyncSender<Job>> = OnceLock::new();

#[cfg(all(feature = "desktop", feature = "tokio"))]
fn desktop_thread() -> &'static mpsc::SyncSender<Job> {
    DESKTOP_THREAD.get_or_init(|| {
        // Rendezvous channel: `send` blocks until the dedicated thread has
        // picked up the job, which keeps queuing semantics simple (jobs run
        // strictly one after another, in submission order).
        let (tx, rx) = mpsc::sync_channel::<Job>(0);

        thread::Builder::new()
            .name("ffplayout-desktop".to_string())
            .spawn(move || {
                for job in rx {
                    job();
                }
            })
            .expect("failed to spawn dedicated desktop thread");

        tx
    })
}

/// Runs `job` on the dedicated desktop thread. Blocks the caller until
/// the job has been *accepted* by that thread (not until it finishes) -
/// callers that need to wait for completion should signal it themselves,
/// e.g. via a channel captured in the closure.
#[cfg(all(feature = "desktop", feature = "tokio"))]
pub(crate) fn spawn(job: impl FnOnce() + Send + 'static) {
    desktop_thread()
        .send(Box::new(job))
        .expect("dedicated desktop thread is gone");
}
