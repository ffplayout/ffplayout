//! Persist desired layer settings independently of transient playback resources.
use std::sync::atomic::Ordering;

use ff_engine::ClipGraphics;
use serde_json::{Value, json};
use sqlx::types::Json;

mod preparation;
mod runtime;
mod settings;

#[cfg(test)]
mod tests;

use crate::{
    db::{handles, models::GraphicLayer},
    player::controller::ChannelManager,
    utils::errors::ServiceError,
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
    let (revision, rows) = handles::select_graphic_layers(&manager.db_pool, manager.id).await?;
    let (_, live) = manager.manual_graphics.status();
    let layers: Vec<_> = rows
        .into_iter()
        .map(|row| {
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
        "layers": layers,
    }))
}

pub(crate) async fn command(
    manager: ChannelManager,
    expected: u64,
    data: Value,
) -> Result<u64, ServiceError> {
    // Serialize DB state changes and runtime handoff, never frame rendering.
    let _operation = manager.manual_state.operations.lock().await;
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
        let config = manager.config.read().await.clone();
        settings::validate(&row.settings, &config.storage.path).map_err(|error| {
            ServiceError::BadRequest(format!("Invalid graphic settings: {error:#}"))
        })?;
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
        Some(preparation::prepare(&manager, row.settings.0.clone()).await?)
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

    Ok(revision)
}

async fn persist(
    manager: &ChannelManager,
    revision: i64,
    rows: &[GraphicLayer],
) -> Result<u64, ServiceError> {
    handles::store_graphic_layers(&manager.db_pool, manager.id, revision, rows)
        .await
        .map(|revision| revision as u64)
        .map_err(|error| ServiceError::Conflict(format!("Could not save graphic layers: {error}")))
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
