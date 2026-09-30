//! Main-thread job host for desktop output.
//!
//! Winit requires the event loop to be created on the process main thread on
//! macOS and recommends the same on other platforms. The application starts
//! Tokio on a background thread and runs this host on its main thread. Desktop
//! jobs are serialized and kept short. The desktop playout worker runs on its
//! own thread so audio and video scheduling continue while a platform window
//! enters a modal move/resize loop.

use std::sync::{OnceLock, mpsc};
#[cfg(feature = "tokio")]
use std::{thread, time::Duration};

#[cfg(feature = "tokio")]
use anyhow::Context;
use anyhow::{Result, anyhow};

type Job = Box<dyn FnOnce() + Send>;

static DESKTOP_MAIN_THREAD: OnceLock<mpsc::SyncSender<Job>> = OnceLock::new();

/// Runs the application work on a background thread while this (calling)
/// thread owns all desktop window jobs. Must be called from `main` before a
/// desktop `AsyncPlayout` is opened. Returns an error if the host has already
/// been initialized, the runtime thread cannot start, or it exits without a result.
#[cfg(feature = "tokio")]
pub fn run_on_main_thread<R: Send + 'static>(
    background: impl FnOnce() -> R + Send + 'static,
) -> Result<R> {
    let (jobs_tx, jobs_rx) = mpsc::sync_channel::<Job>(64);

    DESKTOP_MAIN_THREAD
        .set(jobs_tx)
        .map_err(|_| anyhow!("desktop main-thread host was initialized more than once"))?;

    let (done_tx, done_rx) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("ffplayout-runtime".to_string())
        .spawn(move || {
            let _ = done_tx.send(background());
        })
        .context("failed to start ffplayout runtime thread")?;

    loop {
        match jobs_rx.recv_timeout(Duration::from_millis(10)) {
            Ok(job) => job(),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let result = done_rx
                    .recv()
                    .context("ffplayout runtime stopped without a result");
                super::release_desktop_window();

                return result;
            }
        }

        super::pump_desktop_window_events();

        match done_rx.try_recv() {
            Ok(result) => {
                super::release_desktop_window();

                return Ok(result);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                super::release_desktop_window();

                return Err(anyhow!("ffplayout runtime stopped without a result"));
            }
            Err(mpsc::TryRecvError::Empty) => {}
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

pub(crate) fn call<T: Send + 'static>(operation: impl FnOnce() -> T + Send + 'static) -> Result<T> {
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    spawn(move || {
        let _ = result_tx.send(operation());
    })?;
    result_rx
        .recv()
        .map_err(|_| anyhow!("desktop main-thread operation stopped"))
}

pub(crate) fn is_running() -> bool {
    DESKTOP_MAIN_THREAD.get().is_some()
}
