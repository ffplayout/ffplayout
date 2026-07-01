//! A single, process-lifetime OS thread dedicated to SDL/desktop output.
//!
//! SDL's Wayland backend uses libdecor for window decorations, and
//! libdecor's GTK plugin initializes GTK/GLib the first time it runs. That
//! initialization binds GTK to whichever OS thread called it; if a later
//! desktop-output session runs on a *different* thread, GTK/libdecor can fail
//! with "Failed to load plugin 'libdecor-gtk.so': failed to init".
//!
//! Every desktop-output session (across restarts and channels) is executed
//! as a job on this single dedicated thread, so SDL/GTK always see the same
//! thread for the whole process lifetime. Jobs run one at a time, so only
//! one desktop-output session can be active at once - reasonable, since
//! there is only one local display anyway.

use std::{
    sync::{OnceLock, mpsc},
    thread,
};

type Job = Box<dyn FnOnce() + Send>;

static SDL_THREAD: OnceLock<mpsc::SyncSender<Job>> = OnceLock::new();

fn sdl_thread() -> &'static mpsc::SyncSender<Job> {
    SDL_THREAD.get_or_init(|| {
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
            .expect("failed to spawn dedicated desktop/SDL thread");

        tx
    })
}

/// Runs `job` on the dedicated desktop/SDL thread. Blocks the caller until
/// the job has been *accepted* by that thread (not until it finishes) -
/// callers that need to wait for completion should signal it themselves,
/// e.g. via a channel captured in the closure.
pub(crate) fn spawn(job: impl FnOnce() + Send + 'static) {
    sdl_thread()
        .send(Box::new(job))
        .expect("dedicated desktop/SDL thread is gone");
}
