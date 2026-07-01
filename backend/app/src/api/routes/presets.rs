use axum::{
    Json,
    extract::{Path, State},
};
use protect_axum::authorities::AuthDetails;

use crate::{
    api::{
        routes::{AuthUser, ensure_any_authority},
        state::AppState,
    },
    db::{
        handles,
        models::{Role, TextPreset},
    },
    utils::errors::ServiceError,
};

/// #### Text Presets
///
/// Text presets are made for sending text messages to the ffplayout engine, to overlay them as a lower third.
///
/// **Get all Presets**
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/presets/1 -H 'Content-Type: application/json' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn get_presets(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<Vec<TextPreset>>, ServiceError> {
    ensure_any_authority(
        &details,
        &[&Role::GlobalAdmin, &Role::ChannelAdmin, &Role::User],
    )?;
    user.ensure_channel_or_admin(id)?;

    if let Ok(presets) = handles::select_presets(&state.pool, id).await {
        return Ok(Json(presets));
    }

    Err(ServiceError::InternalServerError)
}

/// **Update Preset**
///
/// ```BASH
/// curl -X PUT http://127.0.0.1:8787/api/presets/1 -H 'Content-Type: application/json' \
/// -d '{ "name": "<PRESET NAME>", "text": "<TEXT>", "x": "<X>", "y": "<Y>", "fontsize": 24, "line_spacing": 4, "fontcolor": "#ffffff", "box": 1, "boxcolor": "#000000", "boxborderw": 4, "alpha": 1.0, "channel_id": 1 }' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn update_preset(
    State(state): State<AppState>,
    Path((channel, id)): Path<(i32, i32)>,
    user: AuthUser,
    details: AuthDetails<Role>,
    Json(data): Json<TextPreset>,
) -> Result<&'static str, ServiceError> {
    ensure_any_authority(
        &details,
        &[&Role::GlobalAdmin, &Role::ChannelAdmin, &Role::User],
    )?;
    user.ensure_channel_or_admin(channel)?;

    if handles::update_preset(&state.pool, &id, data).await.is_ok() {
        return Ok("Update Success");
    }

    Err(ServiceError::InternalServerError)
}

/// **Add new Preset**
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/presets/1 -H 'Content-Type: application/json' \
/// -d '{ "name": "<PRESET NAME>", "text": "TEXT>", "x": "<X>", "y": "<Y>", "fontsize": 24, "line_spacing": 4, "fontcolor": "#ffffff", "box": 1, "boxcolor": "#000000", "boxborderw": 4, "alpha": 1.0, "channel_id": 1 }' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn add_preset(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
    Json(data): Json<TextPreset>,
) -> Result<&'static str, ServiceError> {
    ensure_any_authority(
        &details,
        &[&Role::GlobalAdmin, &Role::ChannelAdmin, &Role::User],
    )?;
    user.ensure_channel_or_admin(id)?;

    if handles::insert_preset(&state.pool, data).await.is_ok() {
        return Ok("Add preset Success");
    }

    Err(ServiceError::InternalServerError)
}

/// **Delete Preset**
///
/// ```BASH
/// curl -X DELETE http://127.0.0.1:8787/api/presets/1/1 -H 'Content-Type: application/json' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn delete_preset(
    State(state): State<AppState>,
    Path((channel, id)): Path<(i32, i32)>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<&'static str, ServiceError> {
    ensure_any_authority(
        &details,
        &[&Role::GlobalAdmin, &Role::ChannelAdmin, &Role::User],
    )?;
    user.ensure_channel_or_admin(channel)?;

    if handles::delete_preset(&state.pool, &id).await.is_ok() {
        return Ok("Delete preset Success");
    }

    Err(ServiceError::InternalServerError)
}
