//! Manual graphics admission/preparation; HTTP is only a transport adapter.
use std::{path::Path, sync::atomic::Ordering};

use anyhow::{Context, Result};
use ff_engine::{ClipGraphics, ManualChange};
use ff_graphics::{
    playback::Transition,
    playlist::{Action, ClipSchedule, Event, Limits},
    preparation::PreparationService,
};
use serde_json::{Value, json};

use crate::{player::controller::ChannelManager, utils::errors::ServiceError};

use super::{microseconds, prepare_scene_budgeted};

pub(crate) async fn status(manager: &ChannelManager) -> Value {
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
    let (revision, layers) = manager.manual_graphics.status();
    let layers: Vec<_> = layers
        .into_iter()
        .map(|layer| {
            json!({
                "layer": layer.id,
                "visible": layer.visible,
                "active": layer.active,
                "position": layer.position,
                "opacity": layer.opacity,
                "z_index": layer.z,
            })
        })
        .collect();

    json!({
        "revision": revision,
        "running": manager.is_alive.load(Ordering::SeqCst),
        "canvas": canvas,
        "layers": layers,
    })
}

pub(crate) async fn command(
    manager: ChannelManager,
    expected: u64,
    data: Value,
) -> Result<u64, ServiceError> {
    if !manager.is_alive.load(Ordering::SeqCst) {
        return Err(ServiceError::Conflict("Channel is not running".into()));
    }

    let operation = data
        .get("action")
        .and_then(Value::as_str)
        .ok_or_else(|| ServiceError::BadRequest("Missing action".into()))?;
    let layer = data.get("layer").and_then(Value::as_u64);
    let revision = if operation == "take" {
        let layer = layer.ok_or_else(|| ServiceError::BadRequest("Missing layer".into()))?;
        let graphics = prepare_take(&manager, expected, data).await?;

        manager.manual_graphics.take(expected, layer, graphics)
    } else {
        let position = data
            .get("position")
            .map(|value| serde_json::from_value(value.clone()))
            .transpose()
            .map_err(|_| ServiceError::BadRequest("Invalid position".into()))?;
        let opacity = data
            .get("opacity")
            .map(|value| serde_json::from_value(value.clone()))
            .transpose()
            .map_err(|_| ServiceError::BadRequest("Invalid opacity".into()))?;
        let z = data
            .get("z_index")
            .map(|value| serde_json::from_value(value.clone()))
            .transpose()
            .map_err(|_| ServiceError::BadRequest("Invalid z-index".into()))?;
        let visible = data
            .get("visible")
            .map(|value| serde_json::from_value(value.clone()))
            .transpose()
            .map_err(|_| ServiceError::BadRequest("Invalid visibility".into()))?;

        manager.manual_graphics.change(
            expected,
            ManualChange {
                id: layer,
                operation,
                position,
                opacity,
                z,
                visible,
            },
        )
    }
    .map_err(|error| ServiceError::Conflict(error.to_string()))?;

    Ok(revision)
}

async fn prepare_take(
    manager: &ChannelManager,
    expected: u64,
    data: Value,
) -> Result<ClipGraphics, ServiceError> {
    let permit = manager
        .manual_jobs
        .clone()
        .try_acquire_owned()
        .map_err(|_| ServiceError::Conflict("Graphics preparation busy; retry shortly".into()))?;

    if manager.manual_graphics.revision() != expected {
        return Err(ServiceError::Conflict(
            "Graphics revision changed; refresh before retrying".into(),
        ));
    }

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
    let control = manager.manual_graphics.clone();
    let alive = manager.is_alive.clone();
    let preparation = manager.graphics.preparation();
    let mut data = data;
    data["at"] = json!(0);
    let graphics = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        prepare_manual(
            data,
            &config.storage.path,
            canvas,
            config.general.channel_id,
            || !alive.load(Ordering::SeqCst) || control.revision() != expected,
            preparation,
        )
    })
    .await
    .map_err(|error| ServiceError::BadRequest(format!("Graphics preparation failed: {error}")))?
    .map_err(|error| ServiceError::BadRequest(format!("Cannot prepare graphic: {error:#}")))?;

    Ok(graphics)
}

/// Reuse playlist validation/preparation for one manual program layer. Unlike a
/// clip schedule, an untimed manual graphic holds until explicitly stopped.
fn prepare_manual(
    data: Value,
    root: &Path,
    canvas: (u32, u32),
    channel: i32,
    cancelled: impl Fn() -> bool,
    preparation: PreparationService,
) -> Result<ClipGraphics> {
    let (renderer, mut schedule) = prepare_scene_budgeted(
        &Value::Array(vec![data.clone()]),
        root,
        canvas,
        i64::MAX / 4,
        channel,
        cancelled,
        Some(preparation),
    )?;

    if data.get("duration").is_none_or(Value::is_null)
        && data.get("hold").is_none_or(Value::is_null)
    {
        let scene = schedule
            .frame_at(Some(0))
            .context("manual scene unavailable")?
            .scene;
        let layer = scene
            .layers
            .first()
            .context("graphic source could not be prepared")?;
        let transition = Transition {
            fade_in_us: microseconds(data.get("fade_in").and_then(Value::as_f64).unwrap_or(0.0))?
                as u64,
            fade_out_us: microseconds(data.get("fade_out").and_then(Value::as_f64).unwrap_or(0.0))?
                as u64,
            ..Transition::default()
        };
        schedule = ClipSchedule::compile(
            i64::MAX / 4,
            &[Event {
                at_us: 0,
                action: Action::Take {
                    layer: layer.spec,
                    transition,
                },
            }],
            std::slice::from_ref(&layer.resource),
            Limits::default(),
        )?;
    }

    Ok(ClipGraphics::new(renderer, schedule))
}
