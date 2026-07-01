use axum::{
    Json,
    extract::{Path, State},
};
use log::*;
use protect_axum::authorities::AuthDetails;

use crate::{
    AdvancedConfig,
    api::{
        routes::{AuthUser, ensure_any_authority},
        state::AppState,
    },
    db::{handles, models::Role},
    utils::{config::get_config, errors::ServiceError},
};

/// #### ffplayout Config
///
/// **Get Advanced Config**
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/playout/advanced/1 -H 'Authorization: Bearer <TOKEN>'
/// ```
///
/// Response is a JSON object
pub async fn get_advanced_config(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<AdvancedConfig>, ServiceError> {
    ensure_any_authority(&details, &[&Role::GlobalAdmin, &Role::ChannelAdmin])?;
    user.ensure_channel_or_admin(id)?;

    let manager = {
        let guard = state.controller.read().await;
        guard.get(id)
    }
    .ok_or_else(|| ServiceError::BadRequest(format!("Channel {id} not found!")))?;

    let config = manager.config.read().await.advanced.clone();

    Ok(Json(config))
}

/// **Get related Advanced Config**
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/playout/advanced/1 -H 'Authorization: Bearer <TOKEN>'
/// ```
///
/// Response is a JSON object
pub async fn get_related_advanced_config(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<Vec<AdvancedConfig>>, ServiceError> {
    ensure_any_authority(&details, &[&Role::GlobalAdmin, &Role::ChannelAdmin])?;
    user.ensure_channel_or_admin(id)?;

    match handles::select_related_advanced_configuration(&state.pool, id).await {
        Ok(configs) => Ok(Json(
            configs
                .iter()
                .map(|c| AdvancedConfig::new(c.clone()))
                .collect::<Vec<_>>(),
        )),
        Err(e) => {
            error!("Advanced config: {e}");

            Err(ServiceError::InternalServerError)
        }
    }
}

/// **Delete Advanced Config**
///
/// ```BASH
/// curl -X DELETE http://127.0.0.1:8787/api/playout/advanced -H 'Authorization: Bearer <TOKEN>'
/// ```
///
/// Response is a JSON object
pub async fn remove_related_advanced_config(
    State(state): State<AppState>,
    Path((channel, id)): Path<(i32, i32)>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<&'static str, ServiceError> {
    ensure_any_authority(&details, &[&Role::GlobalAdmin, &Role::ChannelAdmin])?;
    user.ensure_channel_or_admin(channel)?;

    let manager = {
        let guard = state.controller.read().await;
        guard.get(id)
    }
    .ok_or_else(|| ServiceError::BadRequest(format!("Channel {id} not found!")))?;

    if handles::delete_advanced_configuration(&state.pool, id)
        .await
        .is_ok()
    {
        let new_config = get_config(&state.pool, id).await?;
        manager.update_config(new_config).await;

        return Ok("Delete advanced configuration Success");
    }

    Err(ServiceError::InternalServerError)
}

/// **Update Advanced Config**
///
/// ```BASH
/// curl -X PUT http://127.0.0.1:8787/api/playout/advanced/1 -H "Content-Type: application/json" \
/// -d { <CONFIG DATA> } -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn update_advanced_config(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
    Json(data): Json<AdvancedConfig>,
) -> Result<Json<&'static str>, ServiceError> {
    ensure_any_authority(&details, &[&Role::GlobalAdmin, &Role::ChannelAdmin])?;
    user.ensure_channel_or_admin(id)?;

    let manager = {
        let guard = state.controller.read().await;
        guard.get(id)
    }
    .ok_or_else(|| ServiceError::BadRequest(format!("Channel {id} not found!")))?;

    handles::update_advanced_configuration(&state.pool, id, data).await?;
    let new_config = get_config(&state.pool, id).await?;

    manager.update_config(new_config).await;

    Ok(Json("Update success"))
}

/// **Add Advanced Config**
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/playout/advanced/1 -H "Content-Type: application/json" \
/// -d { <CONFIG DATA> } -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn add_advanced_config(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
    Json(data): Json<AdvancedConfig>,
) -> Result<Json<&'static str>, ServiceError> {
    ensure_any_authority(&details, &[&Role::GlobalAdmin, &Role::ChannelAdmin])?;
    user.ensure_channel_or_admin(id)?;

    let manager = {
        let guard = state.controller.read().await;
        guard.get(id)
    }
    .ok_or_else(|| ServiceError::BadRequest(format!("Channel {id} not found!")))?;

    handles::insert_advanced_configuration(&state.pool, id, None, data).await?;
    let new_config = get_config(&state.pool, id).await?;

    manager.update_config(new_config).await;

    Ok(Json("Update success"))
}
