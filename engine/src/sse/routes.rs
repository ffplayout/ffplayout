use actix_web::{Responder, get, post, web};
use actix_web_grants::proc_macro::protect;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use super::{SseAuthState, UuidData, check_uuid, prune_uuids};
use crate::db::models::Role;
use crate::player::controller::ChannelController;
use crate::sse::{Endpoint, broadcast::Broadcaster};
use crate::utils::errors::ServiceError;

#[derive(Deserialize, Serialize)]
struct User {
    #[serde(default, skip_serializing)]
    endpoint: Endpoint,
    uuid: String,
}

impl User {
    fn new(endpoint: Endpoint, uuid: String) -> Self {
        Self { endpoint, uuid }
    }
}

/// **Get generated UUID**
///
/// ```BASH
/// curl -X GET 'http://127.0.0.1:8787/api/generate-uuid' -H 'Authorization: Bearer <TOKEN>'
/// ```
#[post("/generate-uuid")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role"
)]
async fn generate_uuid(data: web::Data<SseAuthState>) -> Result<impl Responder, ServiceError> {
    let mut uuids = data.uuids.lock().await;
    let new_uuid = UuidData::new();
    let user_auth = User::new(Endpoint::default(), new_uuid.uuid.to_string());

    prune_uuids(&mut uuids);

    uuids.insert(new_uuid);

    Ok(web::Json(user_auth))
}

/// **Validate UUID**
///
/// ```BASH
/// curl -X GET 'http://127.0.0.1:8787/data/validate?uuid=f2f8c29b-712a-48c5-8919-b535d3a05a3a'
/// ```
#[get("/validate")]
async fn validate_uuid(
    data: web::Data<SseAuthState>,
    user: web::Query<User>,
) -> Result<impl Responder, ServiceError> {
    let mut uuids = data.uuids.lock().await;

    match check_uuid(&mut uuids, user.uuid.as_str()) {
        Ok(s) => Ok(web::Json(s)),
        Err(e) => Err(e),
    }
}

/// **Connect to event handler**
///
/// ```BASH
/// curl -X GET 'http://127.0.0.1:8787/data/event/1?endpoint=system&uuid=f2f8c29b-712a-48c5-8919-b535d3a05a3a'
/// ```
#[get("/event/{id}")]
async fn event_stream(
    broadcaster: web::Data<Broadcaster>,
    data: web::Data<SseAuthState>,
    id: web::Path<i32>,
    user: web::Query<User>,
    controllers: web::Data<RwLock<ChannelController>>,
) -> Result<impl Responder, ServiceError> {
    let mut uuids = data.uuids.lock().await;

    check_uuid(&mut uuids, user.uuid.as_str())?;

    let manager = {
        let guard = controllers.read().await;
        guard.get(*id)
    }
    .ok_or_else(|| ServiceError::BadRequest(format!("Channel {id} not found!")))?;

    Ok(broadcaster
        .new_client(manager.clone(), user.endpoint.clone())
        .await)
}
