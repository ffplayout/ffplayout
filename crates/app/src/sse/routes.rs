use axum::{
    Json, Router,
    extract::{Path, Query, State},
    response::IntoResponse,
    routing::{get, post},
};
use log::warn;
use protect_axum::authorities::AuthDetails;
use real::RealIp;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::{
    api::{
        routes::{AuthUser, ensure_any_authority},
        state::AppState,
    },
    db::{handles, models::Role},
    sse::{Endpoint, UuidData, check_uuid, prune_uuids, validated_uuid},
    utils::errors::ServiceError,
};

#[derive(Deserialize, Serialize)]
pub struct User {
    #[serde(default, skip_serializing)]
    endpoint: Endpoint,
    #[serde(default)]
    audio_meter: bool,
    uuid: String,
}

impl User {
    fn new(uuid: String) -> Self {
        Self {
            endpoint: Endpoint::default(),
            audio_meter: false,
            uuid,
        }
    }
}

pub fn api_routes() -> Router<AppState> {
    Router::new().route("/generate-uuid", post(generate_uuid))
}

pub fn data_routes() -> Router<AppState> {
    Router::new()
        .route("/validate", get(validate_uuid))
        .route("/event/{id}", get(event_stream))
}

pub async fn generate_uuid(
    real_ip: RealIp,
    State(state): State<AppState>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<User>, ServiceError> {
    ensure_any_authority(
        &details,
        &[&Role::GlobalAdmin, &Role::ChannelAdmin, &Role::User],
    )?;

    let mut uuids = state.auth_state.uuids.lock().await;
    let ip_address = real_ip.ip().to_string();
    let user_id = (user.id > 0).then_some(user.id);
    let new_uuid = UuidData::new(ip_address, user_id);
    let user_auth = User::new(new_uuid.uuid.to_string());

    prune_uuids(&mut uuids);
    uuids.insert(new_uuid);

    Ok(Json(user_auth))
}

pub async fn validate_uuid(
    real_ip: RealIp,
    State(state): State<AppState>,
    Query(user): Query<User>,
) -> Result<Json<&'static str>, ServiceError> {
    let mut uuids = state.auth_state.uuids.lock().await;
    let ip_address = real_ip.ip().to_string();

    check_uuid(&mut uuids, user.uuid.as_str(), &ip_address)?;

    Ok(Json("UUID is valid"))
}

pub async fn event_stream(
    real_ip: RealIp,
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Query(user): Query<User>,
) -> Result<impl IntoResponse, ServiceError> {
    let ip_address = real_ip.ip().to_string();
    let user_id = {
        let mut uuids = state.auth_state.uuids.lock().await;
        let entry = validated_uuid(&mut uuids, user.uuid.as_str(), &ip_address)?;

        entry
            .user_id
            .ok_or_else(|| ServiceError::Forbidden("SSE UUID has no user".to_string()))?
    };
    ensure_event_access(&state.pool, user_id, id).await?;

    let manager = {
        let guard = state.controller.read().await;
        guard.get(id)
    }
    .ok_or_else(|| ServiceError::BadRequest(format!("Channel {id} not found!")))?;

    let mut response = state
        .broadcaster
        .new_client(manager.clone(), user.endpoint.clone(), user.audio_meter)
        .await
        .into_response();

    response.headers_mut().insert(
        "X-Accel-Buffering",
        "no".parse()
            .map_err(|_| ServiceError::InternalServerError)?,
    );
    response.headers_mut().insert(
        "Cache-Control",
        "no-cache"
            .parse()
            .map_err(|_| ServiceError::InternalServerError)?,
    );
    response.headers_mut().insert(
        "Content-Type",
        "text/event-stream"
            .parse()
            .map_err(|_| ServiceError::InternalServerError)?,
    );

    Ok(response)
}

async fn ensure_event_access(
    pool: &SqlitePool,
    user_id: i32,
    channel_id: i32,
) -> Result<(), ServiceError> {
    let user = handles::select_user(pool, user_id).await.map_err(|error| {
        warn!("Cannot authorize SSE user {user_id}: {error}");
        ServiceError::Forbidden("SSE user is unavailable".to_string())
    })?;
    let role_id = user
        .role_id
        .ok_or_else(|| ServiceError::Forbidden("SSE user has no role".to_string()))?;
    let role = handles::select_role(pool, &role_id).await?;

    if role == Role::Guest {
        return Err(ServiceError::Forbidden(
            "SSE requires an authenticated user".to_string(),
        ));
    }

    let auth_user = AuthUser {
        id: user.id,
        channels: user.channel_ids.unwrap_or_default(),
        role,
    };

    auth_user.ensure_channel_or_admin(channel_id)
}

#[cfg(test)]
mod tests {
    use std::{net::Ipv4Addr, sync::Arc};

    use sqlx::sqlite::SqlitePoolOptions;
    use tokio::sync::{Mutex, RwLock};
    use tokio_util::sync::CancellationToken;

    use crate::{
        api::file_access::FileAccessState,
        player::controller::ChannelController,
        sse::{SseAuthState, broadcast::Broadcaster},
        utils::system::SystemStat,
    };

    use super::*;

    async fn pool_with_user(role_id: i32) -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        handles::db_migrate(&pool).await.unwrap();
        sqlx::query("INSERT INTO auth_user (id, mail, username, password, role_id) VALUES (1, 'sse@example.org', 'sse-user', 'unused', $1)")
            .bind(role_id)
            .execute(&pool)
            .await
            .unwrap();
        handles::insert_user_channel(&mut pool.acquire().await.unwrap(), 1, vec![1])
            .await
            .unwrap();

        pool
    }

    #[tokio::test]
    async fn event_access_checks_current_channel_assignments() {
        let pool = pool_with_user(3).await;

        assert!(ensure_event_access(&pool, 1, 1).await.is_ok());
        assert!(matches!(
            ensure_event_access(&pool, 1, 2).await,
            Err(ServiceError::Forbidden(_))
        ));
        handles::delete_user_channels(&pool, 1).await.unwrap();
        assert!(matches!(
            ensure_event_access(&pool, 1, 1).await,
            Err(ServiceError::Forbidden(_))
        ));
    }

    #[tokio::test]
    async fn event_access_checks_current_admin_role_and_user_existence() {
        let pool = pool_with_user(1).await;

        assert!(ensure_event_access(&pool, 1, 2).await.is_ok());
        sqlx::query("UPDATE auth_user SET role_id = 2 WHERE id = 1")
            .execute(&pool)
            .await
            .unwrap();
        assert!(ensure_event_access(&pool, 1, 1).await.is_ok());
        assert!(matches!(
            ensure_event_access(&pool, 1, 2).await,
            Err(ServiceError::Forbidden(_))
        ));
        sqlx::query("UPDATE auth_user SET role_id = 4 WHERE id = 1")
            .execute(&pool)
            .await
            .unwrap();
        assert!(matches!(
            ensure_event_access(&pool, 1, 1).await,
            Err(ServiceError::Forbidden(_))
        ));
        handles::delete_user(&pool, 1).await.unwrap();
        assert!(matches!(
            ensure_event_access(&pool, 1, 1).await,
            Err(ServiceError::Forbidden(_))
        ));
    }

    #[tokio::test]
    async fn event_stream_rejects_an_unassigned_channel_before_loading_it() {
        let pool = pool_with_user(3).await;
        let system = SystemStat::new();
        let state = AppState {
            auth_state: Arc::new(SseAuthState::default()),
            broadcaster: Broadcaster::create(system.clone()),
            controller: Arc::new(RwLock::new(ChannelController::new())),
            file_access: Arc::new(FileAccessState::default()),
            mail_queues: Arc::new(Mutex::new(Vec::new())),
            pool,
            shutdown: CancellationToken::new(),
            system,
        };
        let entry = UuidData::new(Ipv4Addr::LOCALHOST.to_string(), Some(1));
        let uuid = entry.uuid.to_string();
        state.auth_state.uuids.lock().await.insert(entry);
        let result = event_stream(
            RealIp(Ipv4Addr::LOCALHOST.into()),
            State(state.clone()),
            Path(2),
            Query(User::new(uuid.clone())),
        )
        .await;

        assert!(matches!(result, Err(ServiceError::Forbidden(_))));
        assert!(state.auth_state.uuids.try_lock().is_ok());
        let result = event_stream(
            RealIp(Ipv4Addr::LOCALHOST.into()),
            State(state),
            Path(1),
            Query(User::new(uuid)),
        )
        .await;

        // The assigned channel passes authorization and reaches the empty controller.
        assert!(matches!(result, Err(ServiceError::BadRequest(_))));
    }
}
