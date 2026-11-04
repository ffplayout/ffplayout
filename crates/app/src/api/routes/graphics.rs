use axum::{
    Json,
    extract::{Path, State},
};
use protect_axum::authorities::AuthDetails;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    api::{
        routes::{AuthUser, ensure_any_authority},
        state::AppState,
    },
    db::models::Role,
    player::{controller::ChannelManager, graphics::manual},
    utils::errors::ServiceError,
};

#[derive(Deserialize)]
pub struct GraphicsRequest {
    revision: u64,
    #[serde(flatten)]
    data: Value,
}

async fn channel_manager(
    state: &AppState,
    id: i32,
    user: &AuthUser,
    details: &AuthDetails<Role>,
) -> Result<ChannelManager, ServiceError> {
    ensure_any_authority(
        details,
        &[&Role::GlobalAdmin, &Role::ChannelAdmin, &Role::User],
    )?;
    user.ensure_channel_or_admin(id)?;

    state
        .controller
        .read()
        .await
        .get(id)
        .ok_or_else(|| ServiceError::BadRequest(format!("Channel {id} not found")))
}

pub async fn graphics_status(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<Value>, ServiceError> {
    let manager = channel_manager(&state, id, &user, &details).await?;

    Ok(Json(manual::status(&manager).await?))
}

pub async fn graphics_command(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
    Json(request): Json<GraphicsRequest>,
) -> Result<Json<Value>, ServiceError> {
    let manager = channel_manager(&state, id, &user, &details).await?;
    let revision = manual::command(manager, request.revision, request.data).await?;

    Ok(Json(json!({"revision": revision})))
}
