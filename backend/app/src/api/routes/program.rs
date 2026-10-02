use axum::{
    Json,
    extract::{Path, Query, State},
};
use protect_axum::authorities::AuthDetails;

use crate::{
    api::{
        routes::{AuthUser, ensure_any_authority},
        state::AppState,
    },
    db::models::Role,
    utils::{errors::ServiceError, program::build_program},
};

pub use crate::utils::program::{ProgramItem, ProgramObj};

/// **Program info**
///
/// Get program infos about given date, or current day
///
/// Examples:
///
/// * get program from current day
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/program/1 -H 'Authorization: Bearer <TOKEN>'
/// ```
///
/// * get a program range between two dates
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/program/1?start_after=2022-11-13T12:00:00&start_before=2022-11-20T11:59:59 \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
///
/// * get program from give day
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/program/1?start_after=2022-11-13T10:00:00 \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn get_program(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Query(obj): Query<ProgramObj>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<Vec<ProgramItem>>, ServiceError> {
    ensure_any_authority(
        &details,
        &[&Role::GlobalAdmin, &Role::ChannelAdmin, &Role::User],
    )?;
    user.ensure_channel_or_admin(id)?;

    let manager = {
        let guard = state.controller.read().await;
        guard.get(id)
    }
    .ok_or_else(|| ServiceError::BadRequest(format!("Channel {id} not found!")))?;

    let config = manager.config.read().await.clone();
    let program = build_program(&config, obj).await?;

    Ok(Json(program))
}
