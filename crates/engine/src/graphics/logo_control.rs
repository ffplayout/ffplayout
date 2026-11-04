//! Prepare logo changes and retire old decoders away from program-frame processing.
use std::{
    fmt,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    thread::{self, Thread},
};

use anyhow::Result;
use ff_graphics::{
    preparation::{Cancellation, MediaTask, PreparationService},
    style::LogoStyle,
};
use log::warn;

use crate::utils::config::LogoConfig;

use super::source::{LogoSource, SourceKey};

#[derive(Clone)]
struct Request {
    revision: u64,
    config: Option<LogoConfig>,
    canvas: (u32, u32),
    channel: Option<i32>,
    preparation: PreparationService,
}

pub(super) struct Change {
    pub revision: u64,
    pub config: Option<LogoConfig>,
    pub key: Option<SourceKey>,
    pub source: Option<LogoSource>,
}

#[derive(Default)]
pub(super) struct State {
    pub revision: u64,
    epoch: u64,
    desired: Option<LogoConfig>,
    inflight: bool,
    requested: Option<Request>,
    pub pending: Option<Change>,
    pub active: Option<SourceKey>,
    pub retired: Option<LogoSource>,
}

impl State {
    pub(super) fn idle(&self) -> bool {
        self.requested.is_none()
            && !self.inflight
            && self.pending.is_none()
            && self.retired.is_none()
    }
}

#[derive(Default)]
struct Shared {
    pub state: Mutex<State>,
    wake: Mutex<Option<Thread>>,
}

#[derive(Clone, Default)]
pub struct LogoControl {
    shared: Arc<Shared>,
    task: Arc<Mutex<Option<MediaTask>>>,
}

impl fmt::Debug for LogoControl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LogoControl").finish_non_exhaustive()
    }
}

impl LogoControl {
    /// Called by the application or at clip setup, never during a video frame.
    pub fn request(
        &self,
        config: Option<LogoConfig>,
        canvas: (u32, u32),
        channel: Option<i32>,
        preparation: PreparationService,
    ) -> Result<()> {
        self.enqueue(config, canvas, channel, preparation, None)
    }

    fn enqueue(
        &self,
        config: Option<LogoConfig>,
        canvas: (u32, u32),
        channel: Option<i32>,
        preparation: PreparationService,
        expected: Option<u64>,
    ) -> Result<()> {
        if let Some(config) = &config {
            LogoStyle {
                scale: config.scale.clone(),
                opacity: config.opacity,
                position: config.position.clone(),
            }
            .validate(canvas)?;
        }

        let mut task = self.task.lock().unwrap_or_else(PoisonError::into_inner);

        if expected.is_some_and(|revision| {
            self.shared
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .revision
                != revision
        }) {
            return Ok(());
        }

        if task.is_none() {
            let shared = self.shared.clone();
            let epoch = {
                let mut state = shared.state.lock().unwrap_or_else(PoisonError::into_inner);
                state.epoch = state.epoch.saturating_add(1);
                state.inflight = false;
                state.epoch
            };
            *task = Some(
                preparation.spawn_media(move |cancelled| run_worker(shared, epoch, cancelled))?,
            );
        }

        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        state.revision = state.revision.saturating_add(1);
        state.desired = config.clone();
        state.requested = Some(Request {
            revision: state.revision,
            config,
            canvas,
            channel,
            preparation,
        });
        drop(state);

        if let Some(task) = task.as_ref() {
            task.wake();
        }

        Ok(())
    }

    pub fn stop(&self) {
        let mut task = self.task.lock().unwrap_or_else(PoisonError::into_inner);

        if let Some(task) = task.take() {
            task.cancel();
        }

        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        state.revision = state.revision.saturating_add(1);
        state.requested = None;
        // A new run gets its logo from the current channel configuration.
        state.desired = None;
        state.active = None;
        state.inflight = false;
    }

    pub(super) fn try_state(&self) -> Option<MutexGuard<'_, State>> {
        self.shared.state.try_lock().ok()
    }

    pub(super) fn wake(&self) {
        if let Ok(wake) = self.shared.wake.try_lock()
            && let Some(thread) = wake.as_ref()
        {
            thread.unpark();
        }
    }

    pub(super) fn refresh(
        &self,
        canvas: (u32, u32),
        channel: Option<i32>,
        preparation: PreparationService,
    ) {
        let (config, revision) = {
            let state = self
                .shared
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner);

            if !state.idle() {
                return;
            }

            (state.desired.clone(), state.revision)
        };

        if config.is_some() {
            let _ = self.enqueue(config, canvas, channel, preparation, Some(revision));
        }
    }
}

fn run_worker(shared: Arc<Shared>, epoch: u64, cancelled: Cancellation) {
    {
        let state = shared.state.lock().unwrap_or_else(PoisonError::into_inner);

        if state.epoch != epoch {
            return;
        }

        *shared.wake.lock().unwrap_or_else(PoisonError::into_inner) = Some(thread::current());
    }

    loop {
        let (request, obsolete, retired, active) = {
            let mut state = shared.state.lock().unwrap_or_else(PoisonError::into_inner);

            if state.epoch != epoch {
                return;
            }

            let requested = state.requested.take();
            state.inflight = requested.is_some();
            let obsolete = requested.as_ref().and_then(|_| state.pending.take());

            (
                requested,
                obsolete,
                state.retired.take(),
                state.active.clone(),
            )
        };
        drop(obsolete);
        drop(retired);

        if cancelled.is_cancelled() {
            let pending = {
                let mut state = shared.state.lock().unwrap_or_else(PoisonError::into_inner);

                if state.epoch == epoch {
                    state.pending.take()
                } else {
                    None
                }
            };
            drop(pending);
            break;
        }

        if let Some(request) = request {
            let result = prepare(&request, active.as_ref());

            match result {
                Ok(change) => {
                    let mut state = shared.state.lock().unwrap_or_else(PoisonError::into_inner);

                    if state.epoch == epoch {
                        state.inflight = false;
                    }

                    if state.epoch == epoch
                        && state.revision == change.revision
                        && !cancelled.is_cancelled()
                    {
                        state.pending = Some(change);
                    }
                }
                Err(error) => {
                    let mut state = shared.state.lock().unwrap_or_else(PoisonError::into_inner);

                    if state.epoch == epoch {
                        state.inflight = false;
                    }
                    warn!(channel = request.channel.unwrap_or_default(); "Logo update failed; retaining current logo: {error:#}");
                }
            }

            continue;
        }

        thread::park();
    }
}

fn prepare(request: &Request, active: Option<&SourceKey>) -> Result<Change> {
    let key = request
        .config
        .as_ref()
        .map(|config| SourceKey::new(config, request.canvas.0, request.canvas.1));
    let replace = key
        .as_ref()
        .is_some_and(|key| active.is_none_or(|active| !active.same_media(key)));
    let source = if replace {
        let mut config = request
            .config
            .clone()
            .expect("replacement requires a logo configuration");
        // Opacity is applied to the ready surface metadata, so zero opacity
        // never destroys the decoder or restarts its animation when shown again.
        config.opacity = 1.0;
        Some(LogoSource::open(
            config,
            request.canvas.0,
            request.canvas.1,
            request.channel,
            0,
            Some(request.preparation.budget()),
        )?)
    } else {
        None
    };

    Ok(Change {
        revision: request.revision,
        config: request.config.clone(),
        key,
        source,
    })
}
