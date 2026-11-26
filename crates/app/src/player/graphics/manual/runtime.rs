use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Mutex as StdMutex, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
};

use ff_engine::{ClipGraphics, ManualChange};
use log::warn;
use serde_json::Value;
use tokio::{
    sync::{Mutex, Notify},
    task::JoinHandle,
};

use crate::{
    db::{handles, models::GraphicLayer},
    player::controller::ChannelManager,
    utils::errors::ServiceError,
};

use super::{super::microseconds, preparation, schedule};

#[derive(Debug, Default)]
pub(crate) struct ManualState {
    pub(super) operations: Mutex<()>,
    pub(super) epoch: AtomicUsize,
    pub(super) schedule_version: AtomicUsize,
    pub(super) schedule_changed: Notify,
    restore_task: Mutex<Option<JoinHandle<()>>>,
    loaded: StdMutex<BTreeMap<i64, Value>>,
    scheduled: StdMutex<BTreeSet<i64>>,
    errors: StdMutex<BTreeMap<i64, String>>,
}

impl ManualState {
    pub(super) fn wake_schedule(&self) {
        self.schedule_version.fetch_add(1, Ordering::Release);
        self.schedule_changed.notify_one();
    }

    pub(super) fn loaded(&self, row: &GraphicLayer) -> bool {
        self.loaded
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&row.layer_id)
            == Some(&row.settings.0)
    }

    pub(super) fn scheduled(&self, id: i64) -> bool {
        self.scheduled
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(&id)
    }

    pub(super) fn mark_scheduled(&self, id: i64) {
        self.scheduled
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id);
    }

    pub(super) fn error(&self, id: i64) -> Option<String> {
        self.errors
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&id)
            .cloned()
    }

    pub(super) fn set_error(&self, id: i64, error: &ServiceError) {
        self.errors
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, error.to_string());
    }

    pub(super) fn clear_error(&self, id: i64) {
        self.errors
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&id);
    }

    pub(super) fn forget(&self, id: Option<i64>) {
        let mut loaded = self.loaded.lock().unwrap_or_else(PoisonError::into_inner);
        let mut errors = self.errors.lock().unwrap_or_else(PoisonError::into_inner);
        let mut scheduled = self
            .scheduled
            .lock()
            .unwrap_or_else(PoisonError::into_inner);

        if let Some(id) = id {
            loaded.remove(&id);
            errors.remove(&id);
            scheduled.remove(&id);
        } else {
            loaded.clear();
            errors.clear();
            scheduled.clear();
        }
    }

    pub(super) fn update_layout(&self, row: &GraphicLayer) {
        let mut loaded = self.loaded.lock().unwrap_or_else(PoisonError::into_inner);

        if let Some(settings) = loaded.get_mut(&row.layer_id) {
            for key in ["position", "opacity", "z_index"] {
                if let Some(value) = row.settings.get(key) {
                    settings[key] = value.clone();
                }
            }
        }
    }
}

pub(super) fn change(
    manager: &ChannelManager,
    row: &GraphicLayer,
    operation: &str,
) -> Result<(), ServiceError> {
    // Playback controls must work even when saved source/layout settings became unusable.
    let settings = (operation == "update").then_some(&row.settings);
    let position = settings
        .and_then(|settings| settings.get("position"))
        .map(|value| serde_json::from_value(value.clone()))
        .transpose()?;
    let opacity = settings
        .and_then(|settings| settings.get("opacity"))
        .map(|value| serde_json::from_value(value.clone()))
        .transpose()?;
    let z = settings
        .and_then(|settings| settings.get("z_index"))
        .map(|value| serde_json::from_value(value.clone()))
        .transpose()?;
    // Invalid stored fade values must not prevent Stop/Hide from working.
    let fade_time = |key| {
        row.settings.get(key).map_or(Some(0), |value| {
            value
                .as_f64()
                .and_then(|seconds| microseconds(seconds).ok())
                .map(|time| time as u64)
        })
    };
    let fade_times_us = fade_time("fade_in").zip(fade_time("fade_out"));
    manager
        .manual_graphics
        .change(
            manager.manual_graphics.revision(),
            ManualChange {
                id: Some(row.layer_id as u64),
                operation,
                position,
                opacity,
                z,
                visible: Some(row.visible),
                fade_times_us,
            },
        )
        .map_err(|error| ServiceError::Conflict(error.to_string()))?;

    Ok(())
}

pub(super) fn activate(
    manager: &ChannelManager,
    row: &GraphicLayer,
    prepared: Option<ClipGraphics>,
) -> Result<(), ServiceError> {
    if let Some(prepared) = prepared {
        manager
            .manual_graphics
            .take(
                manager.manual_graphics.revision(),
                row.layer_id as u64,
                prepared,
            )
            .map_err(|error| ServiceError::Conflict(error.to_string()))?;
    } else {
        change(manager, row, "start")?;
    }

    if !row.visible {
        change(manager, row, "visibility")?;
    }

    manager
        .manual_state
        .loaded
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(row.layer_id, row.settings.0.clone());
    manager.manual_state.clear_error(row.layer_id);
    manager
        .manual_state
        .scheduled
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&row.layer_id);

    Ok(())
}

/// A tracked background task restores each layer without delaying program video.
pub(crate) async fn begin_restore(manager: &ChannelManager) {
    let mut task = manager.manual_state.restore_task.lock().await;
    let epoch = manager.manual_state.epoch.fetch_add(1, Ordering::AcqRel) + 1;
    manager.manual_state.wake_schedule();

    if let Some(previous) = task.take() {
        let _ = previous.await;
    }

    let manager = manager.clone();
    *task = Some(tokio::spawn(async move {
        tokio::join!(
            restore_layers(&manager, epoch),
            schedule::run(&manager, epoch)
        );
    }));
}

async fn restore_layers(manager: &ChannelManager, epoch: usize) {
    let rows = match handles::select_graphic_layers(&manager.db_pool, manager.id).await {
        Ok((_, rows)) => rows,
        Err(error) => {
            warn!(channel = manager.id; "Could not load saved graphics: {error}");

            return;
        }
    };

    for saved in rows {
        if !epoch_is_running(manager, epoch) {
            break;
        }

        restore_layer(manager, saved.layer_id, epoch).await;
    }
}

async fn restore_layer(manager: &ChannelManager, id: i64, epoch: usize) {
    let operation = manager.manual_state.operations.lock().await;
    let Some(mut row) = restorable_layer(manager, id).await else {
        return;
    };
    drop(operation);

    while epoch_is_running(manager, epoch) {
        let prepared = preparation::prepare_scheduled(manager, row.settings.0.clone()).await;
        let _operation = manager.manual_state.operations.lock().await;

        if !epoch_is_running(manager, epoch) {
            return;
        }

        // A command may have changed, hidden or started the layer during preparation.
        let Some(current) = restorable_layer(manager, id).await else {
            return;
        };

        if current.settings != row.settings {
            row = current;
            continue;
        }

        if let Err(error) =
            prepared.and_then(|graphics| activate(manager, &current, Some(graphics)))
        {
            manager.manual_state.set_error(id, &error);
            warn!(channel = manager.id; "Could not restore graphic layer {id}: {error}");
        }

        return;
    }
}

pub(super) fn epoch_is_running(manager: &ChannelManager, epoch: usize) -> bool {
    manager.is_alive.load(Ordering::SeqCst)
        && manager.manual_state.epoch.load(Ordering::Acquire) == epoch
}

async fn restorable_layer(manager: &ChannelManager, id: i64) -> Option<GraphicLayer> {
    match handles::select_graphic_layers(&manager.db_pool, manager.id).await {
        Ok((_, rows)) => rows.into_iter().find(|row| {
            row.layer_id == id && row.running && row.visible && !manager.manual_state.loaded(row)
        }),
        Err(error) => {
            warn!(channel = manager.id; "Could not restore graphic layer {id}: {error}");

            None
        }
    }
}

pub(crate) async fn stop_runtime(manager: &ChannelManager) {
    manager.manual_state.epoch.fetch_add(1, Ordering::AcqRel);
    manager.manual_state.wake_schedule();
    let mut task = manager.manual_state.restore_task.lock().await;

    if let Some(task) = task.take() {
        let _ = task.await;
    }

    let _operation = manager.manual_state.operations.lock().await;
    manager.manual_graphics.clear();
    manager.manual_state.forget(None);
}
