use std::sync::Mutex;

use actix_web::{get, post, web, Responder};
use actix_web_grants::proc_macro::protect;
use serde::{Deserialize, Serialize};

use super::{check_uuid, prune_uuids, AuthState, UuidData};
use crate::db::models::Role;
use crate::player::controller::ChannelController;
use crate::sse::broadcast::Broadcaster;
use crate::utils::errors::ServiceError;

#[derive(Deserialize, Serialize)]
struct User {
    #[serde(default, skip_serializing)]
    endpoint: String,
    uuid: String,
}

impl User {
    fn new(endpoint: String, uuid: String) -> Self {
        Self { endpoint, uuid }
    }
}

/// **Get generated UUID**
///
/// ```BASH
/// curl -X GET 'http://127.0.0.1:8787/api/generate-uuid' -H 'Authorization: Bearer <TOKEN>'
/// ```
#[post("/generate-uuid")]
#[protect(any("Role::GlobalAdmin", "Role::User"), ty = "Role")]
async fn generate_uuid(data: web::Data<AuthState>) -> Result<impl Responder, ServiceError> {
    let mut uuids = data.uuids.lock().await;
    let new_uuid = UuidData::new();
    let user_auth = User::new(String::new(), new_uuid.uuid.to_string());

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
    data: web::Data<AuthState>,
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
    data: web::Data<AuthState>,
    id: web::Path<i32>,
    user: web::Query<User>,
    controllers: web::Data<Mutex<ChannelController>>,
) -> Result<impl Responder, ServiceError> {
    let mut uuids = data.uuids.lock().await;

    check_uuid(&mut uuids, user.uuid.as_str())?;

    let manager = controllers.lock().unwrap().get(*id).unwrap();

    Ok(broadcaster
        .new_client(manager.clone(), user.endpoint.clone())
        .await)
}
