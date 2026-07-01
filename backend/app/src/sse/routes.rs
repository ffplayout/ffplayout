use axum::{
    Json, Router,
    extract::{Path, Query, State},
    response::IntoResponse,
    routing::{get, post},
};
use protect_axum::authorities::AuthDetails;
use real::RealIp;
use serde::{Deserialize, Serialize};

use crate::{
    api::{
        routes::{AuthUser, ensure_any_authority},
        state::AppState,
    },
    db::models::Role,
    sse::{Endpoint, UuidData, check_uuid, prune_uuids},
    utils::errors::ServiceError,
};

#[derive(Deserialize, Serialize)]
pub struct User {
    #[serde(default, skip_serializing)]
    endpoint: Endpoint,
    uuid: String,
}

impl User {
    fn new(uuid: String) -> Self {
        Self {
            endpoint: Endpoint::default(),
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

    match check_uuid(&mut uuids, user.uuid.as_str(), &ip_address) {
        Ok(status) => Ok(Json(status)),
        Err(error) => Err(error),
    }
}

pub async fn event_stream(
    real_ip: RealIp,
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Query(user): Query<User>,
) -> Result<impl IntoResponse, ServiceError> {
    let mut uuids = state.auth_state.uuids.lock().await;
    let ip_address = real_ip.ip().to_string();

    check_uuid(&mut uuids, user.uuid.as_str(), &ip_address)?;

    let manager = {
        let guard = state.controller.read().await;
        guard.get(id)
    }
    .ok_or_else(|| ServiceError::BadRequest(format!("Channel {id} not found!")))?;

    let mut response = state
        .broadcaster
        .new_client(manager.clone(), user.endpoint.clone())
        .await
        .into_response();

    response.headers_mut().insert(
        "X-Accel-Buffering",
        "no".parse()
            .map_err(|_| ServiceError::InternalServerError)?,
    );
    response
        .headers_mut()
        .insert("Cache-Control", "no-cache".parse().unwrap());
    response.headers_mut().insert(
        "Content-Type",
        "text/event-stream"
            .parse()
            .map_err(|_| ServiceError::InternalServerError)?,
    );

    Ok(response)
}
