//! Persist desired layer settings independently of transient playback resources.
use std::sync::atomic::Ordering;

use ff_engine::ClipGraphics;
use serde_json::{Value, json};
use sqlx::types::Json;

mod preparation;
mod runtime;
mod schedule;
mod settings;

#[cfg(test)]
mod tests;

use crate::{
    db::{
        handles,
        models::{GraphicAppointment, GraphicLayer},
    },
    player::controller::ChannelManager,
    utils::{errors::ServiceError, time_machine::time_now},
};

pub(crate) use runtime::{ManualState, begin_restore, stop_runtime};

pub(crate) async fn status(manager: &ChannelManager) -> Result<Value, ServiceError> {
    let config = match manager.running_config() {
        Some(config) => (*config).clone(),
        None => manager.config.read().await.clone(),
    };
    let canvas = config
        .program
        .as_ref()
        .map_or((config.output.width, config.output.height), |program| {
            (program.width, program.height)
        });
    let (revision, rows, mut schedules) =
        handles::select_graphics(&manager.db_pool, manager.id).await?;
    let now = time_now(&manager.config.read().await.channel.timezone);
    let (_, live) = manager.manual_graphics.status();
    let layers: Vec<_> = rows
        .into_iter()
        .map(|row| {
            let appointments = schedules.remove(&row.layer_id).unwrap_or_default();
            let timing = schedule::timing(&appointments, &now);
            let active = live
                .iter()
                .any(|layer| layer.id == row.layer_id as u64 && layer.active);

            json!({
                "layer": row.layer_id,
                "name": row.name,
                "settings": row.settings,
                "running": row.running,
                "visible": row.visible,
                "active": active,
                "appointments": appointments,
                "schedule_active_until": timing.active_until,
                "schedule_next_start": timing.next_start,
                "position": row.settings.get("position").cloned().unwrap_or(json!([0,0])),
                "opacity": row.settings.get("opacity").cloned().unwrap_or(json!(1.0)),
                "z_index": row.settings.get("z_index").cloned().unwrap_or(json!(0)),
                "error": manager.manual_state.error(row.layer_id),
            })
        })
        .collect();

    Ok(json!({
        "revision": revision,
        "running": manager.is_alive.load(Ordering::SeqCst),
        "canvas": canvas,
        "timezone": now.time_zone().iana_name().unwrap_or("UTC"),
        "layers": layers,
    }))
}

pub(crate) async fn command(
    manager: ChannelManager,
    expected: u64,
    data: Value,
) -> Result<u64, ServiceError> {
    // Serialize state changes and runtime handoff. Media preparation releases the lock.
    let mut operation = manager.manual_state.operations.lock().await;
    let (revision, mut rows) = handles::select_graphic_layers(&manager.db_pool, manager.id).await?;

    if u64::try_from(revision).ok() != Some(expected) {
        return Err(ServiceError::Conflict(
            "Graphics revision changed; refresh before retrying".into(),
        ));
    }

    let action = data
        .get("action")
        .and_then(Value::as_str)
        .ok_or_else(|| ServiceError::BadRequest("Missing action".into()))?;

    if action == "clear_all" {
        let revision = persist(&manager, revision, &[]).await?;
        manager.manual_graphics.clear();
        manager.manual_state.forget(None);

        return Ok(revision);
    }

    let id = data
        .get("layer")
        .and_then(Value::as_i64)
        .filter(|id| (0..=9_007_199_254_740_991).contains(id))
        .ok_or_else(|| ServiceError::BadRequest("Invalid or missing layer ID".into()))?;
    let index = rows.iter().position(|row| row.layer_id == id);

    if action == "schedule" {
        index.ok_or_else(|| ServiceError::NotFound("Unknown graphic layer".into()))?;
        let appointments: Vec<GraphicAppointment> =
            serde_json::from_value(data.get("appointments").cloned().unwrap_or(Value::Null))
                .map_err(|error| {
                    ServiceError::BadRequest(format!("Invalid graphics schedule: {error}"))
                })?;

        if let Some(index) = appointments
            .iter()
            .position(|appointment| !appointment.is_valid())
        {
            return Err(ServiceError::BadRequest(format!(
                "Invalid graphic appointment {}",
                index + 1
            )));
        }

        let baseline: Option<Vec<GraphicAppointment>> = data
            .get("expected_appointments")
            .map(|expected| serde_json::from_value(expected.clone()))
            .transpose()
            .map_err(|error| {
                ServiceError::BadRequest(format!("Invalid graphics schedule baseline: {error}"))
            })?;

        let revision = handles::store_graphic_schedule(
            &manager.db_pool,
            manager.id,
            revision,
            id,
            &appointments,
            baseline.as_deref(),
        )
        .await
        .map_err(|error| {
            ServiceError::Conflict(format!("Could not save graphics schedule: {error}"))
        })?;

        manager.manual_state.wake_schedule();

        return Ok(revision as u64);
    }

    if action == "clear" {
        let index = index.ok_or_else(|| ServiceError::NotFound("Unknown graphic layer".into()))?;
        let row = rows.remove(index);
        let revision = persist(&manager, revision, &rows).await?;

        if has_runtime(&manager, id) {
            runtime::change(&manager, &row, "clear")?;
        }

        manager.manual_state.forget(Some(id));

        return Ok(revision);
    }

    let mut row = match index {
        Some(index) => rows[index].clone(),
        None if action == "take" || action == "save" => {
            if rows.len() >= 8 {
                return Err(ServiceError::Conflict(
                    "Manual layer limit exceeded (8)".into(),
                ));
            }

            GraphicLayer {
                layer_id: id,
                name: String::new(),
                settings: Json(json!({})),
                running: false,
                visible: true,
            }
        }
        None => return Err(ServiceError::NotFound("Unknown graphic layer".into())),
    };
    configure_row(&mut row, action, &data)?;

    if action != "stop" && !(action == "visibility" && !row.visible) {
        drop(operation);

        let root = manager.config.read().await.storage.path.clone();
        let settings = row.settings.clone();

        tokio::task::spawn_blocking(move || settings::validate(&settings, &root))
            .await?
            .map_err(|error| {
                ServiceError::BadRequest(format!("Invalid graphic settings: {error:#}"))
            })?;

        operation = manager.manual_state.operations.lock().await;
    }

    let alive = manager.is_alive.load(Ordering::SeqCst);

    if action == "take" && !alive {
        return Err(ServiceError::Conflict(
            "Channel is not running; save settings first".into(),
        ));
    }

    let epoch = manager.manual_state.epoch.load(Ordering::Acquire);
    let activate = alive
        && (matches!(action, "take" | "start")
            || (action == "visibility"
                && row.running
                && row.visible
                && !has_runtime(&manager, id)));
    let prepared = if activate
        && (action == "take" || !manager.manual_state.loaded(&row) || !has_runtime(&manager, id))
    {
        drop(operation);

        let prepared = preparation::prepare(&manager, row.settings.0.clone()).await?;
        operation = manager.manual_state.operations.lock().await;

        // The revision checked by persist still protects the snapshot taken before preparation.
        Some(prepared)
    } else {
        None
    };

    if activate
        && (!manager.is_alive.load(Ordering::SeqCst)
            || manager.manual_state.epoch.load(Ordering::Acquire) != epoch)
    {
        return Err(ServiceError::Conflict(
            "Channel stopped during graphics preparation".into(),
        ));
    }

    if let Some(index) = index {
        rows[index] = row.clone();
    } else {
        rows.push(row.clone());
    }

    let revision = persist(&manager, revision, &rows).await?;
    let applied = apply(&manager, &row, action, activate, prepared);

    if let Err(error) = applied {
        manager.manual_state.set_error(id, &error);
        return Err(error);
    }

    drop(operation);

    Ok(revision)
}

async fn persist(
    manager: &ChannelManager,
    revision: i64,
    rows: &[GraphicLayer],
) -> Result<u64, ServiceError> {
    let revision = handles::store_graphic_layers(&manager.db_pool, manager.id, revision, rows)
        .await
        .map_err(|error| {
            ServiceError::Conflict(format!("Could not save graphic layers: {error}"))
        })?;

    manager.manual_state.wake_schedule();

    Ok(revision as u64)
}

fn configure_row(row: &mut GraphicLayer, action: &str, data: &Value) -> Result<(), ServiceError> {
    match action {
        "take" | "save" => {
            row.settings = Json(
                data.get("settings")
                    .cloned()
                    .unwrap_or_else(|| data.clone()),
            );
            let object = row.settings.as_object_mut().ok_or_else(|| {
                ServiceError::BadRequest("Graphic settings must be an object".into())
            })?;
            if data.get("settings").is_none() {
                object.remove("revision");
                object.remove("name");
            }

            object.insert("action".into(), json!("take"));
            object.insert("at".into(), json!(0));
            object.insert("layer".into(), json!(row.layer_id));

            if let Some(name) = data.get("name") {
                row.name = name
                    .as_str()
                    .filter(|name| name.chars().count() <= 200)
                    .ok_or_else(|| ServiceError::BadRequest("Invalid graphic name".into()))?
                    .to_string();
            }

            if action == "take" {
                row.running = true;
                row.visible = true;
            }
        }
        "start" => {
            row.running = true;
            row.visible = true;
        }
        "stop" => row.running = false,
        "visibility" => {
            row.visible = data
                .get("visible")
                .and_then(Value::as_bool)
                .ok_or_else(|| ServiceError::BadRequest("Missing visibility".into()))?;
        }
        "update" => {
            for key in ["position", "opacity", "z_index", "fade_in", "fade_out"] {
                if let Some(value) = data.get(key) {
                    row.settings[key] = value.clone();
                }
            }
        }
        _ => return Err(ServiceError::BadRequest("Unknown graphics action".into())),
    }

    Ok(())
}

fn has_runtime(manager: &ChannelManager, id: i64) -> bool {
    manager
        .manual_graphics
        .status()
        .1
        .iter()
        .any(|layer| layer.id == id as u64)
}

fn apply(
    manager: &ChannelManager,
    row: &GraphicLayer,
    action: &str,
    activate: bool,
    prepared: Option<ClipGraphics>,
) -> Result<(), ServiceError> {
    if activate {
        return runtime::activate(manager, row, prepared);
    }

    if has_runtime(manager, row.layer_id) && matches!(action, "stop" | "visibility" | "update") {
        runtime::change(manager, row, action)?;

        if action == "update" {
            manager.manual_state.update_layout(row);
        }
    }

    Ok(())
}
