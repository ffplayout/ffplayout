//! Main-thread job host for desktop output.
//!
//! Winit requires the event loop to be created on the process main thread on
//! macOS and recommends the same on other platforms. The application starts
//! Tokio on a background thread and runs this host on its main thread. Desktop
//! jobs are still serialized, but are executed by that main thread.

use std::{
    sync::{OnceLock, mpsc},
    thread,
    time::Duration,
};

use anyhow::{Result, anyhow};

type Job = Box<dyn FnOnce() + Send>;

static DESKTOP_MAIN_THREAD: OnceLock<mpsc::SyncSender<Job>> = OnceLock::new();

/// Runs the application work on a background thread while this (calling)
/// thread owns all desktop window jobs. Must be called from `main` before a
/// desktop `AsyncPlayout` is opened.
pub fn run_on_main_thread<R: Send + 'static>(background: impl FnOnce() -> R + Send + 'static) -> R {
    let (jobs_tx, jobs_rx) = mpsc::sync_channel::<Job>(0);
    if DESKTOP_MAIN_THREAD.set(jobs_tx).is_err() {
        panic!("desktop main-thread host was initialized more than once");
    }

    let (done_tx, done_rx) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("ffplayout-runtime".to_string())
        .spawn(move || {
            let _ = done_tx.send(background());
        })
        .expect("failed to start ffplayout runtime thread");

    loop {
        match jobs_rx.recv_timeout(Duration::from_millis(10)) {
            Ok(job) => job(),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return done_rx
                    .recv()
                    .expect("ffplayout runtime stopped without a result");
            }
        }

        if let Ok(result) = done_rx.try_recv() {
            return result;
        }
    }
}

/// Schedules a desktop job for execution by the process main thread.
pub(crate) fn spawn(job: impl FnOnce() + Send + 'static) -> Result<()> {
    DESKTOP_MAIN_THREAD
        .get()
        .ok_or_else(|| anyhow!("desktop main-thread host is not running"))?
        .send(Box::new(job))
        .map_err(|_| anyhow!("desktop main-thread host stopped"))
}
