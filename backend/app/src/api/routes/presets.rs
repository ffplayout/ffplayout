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

pub async fn get_font_families(
    _user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<Vec<String>>, ServiceError> {
    ensure_any_authority(
        &details,
        &[&Role::GlobalAdmin, &Role::ChannelAdmin, &Role::User],
    )?;
    let families = tokio::task::spawn_blocking(ff_engine::available_font_families)
        .await
        .map_err(|_| ServiceError::InternalServerError)?;
    Ok(Json(families))
}

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
/// -d '{ "name": "Lower third", "text": "Message", "position_x": "center", "position_y": "end:72", "font_size": 24, "text_color": "#ffffff", "background_enabled": true, "channel_id": 1 }' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn update_preset(
    State(state): State<AppState>,
    Path((channel, id)): Path<(i32, i32)>,
    user: AuthUser,
    details: AuthDetails<Role>,
    Json(mut data): Json<TextPreset>,
) -> Result<&'static str, ServiceError> {
    ensure_any_authority(
        &details,
        &[&Role::GlobalAdmin, &Role::ChannelAdmin, &Role::User],
    )?;
    user.ensure_channel_or_admin(channel)?;
    data.channel_id = channel;
    data.validate().map_err(ServiceError::BadRequest)?;

    if handles::update_preset(&state.pool, channel, &id, data)
        .await
        .is_ok()
    {
        return Ok("Update Success");
    }

    Err(ServiceError::InternalServerError)
}

/// **Add new Preset**
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/presets/1 -H 'Content-Type: application/json' \
/// -d '{ "name": "Lower third", "text": "Message", "position_x": "center", "position_y": "end:72", "font_size": 24, "text_color": "#ffffff", "background_enabled": true, "channel_id": 1 }' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn add_preset(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
    Json(mut data): Json<TextPreset>,
) -> Result<&'static str, ServiceError> {
    ensure_any_authority(
        &details,
        &[&Role::GlobalAdmin, &Role::ChannelAdmin, &Role::User],
    )?;
    user.ensure_channel_or_admin(id)?;
    data.channel_id = id;
    data.validate().map_err(ServiceError::BadRequest)?;

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

    if handles::delete_preset(&state.pool, channel, &id)
        .await
        .is_ok()
    {
        return Ok("Delete preset Success");
    }

    Err(ServiceError::InternalServerError)
}
