use actix_web::{get, post, web, Responder};
use actix_web_grants::proc_macro::protect;
use serde::{Deserialize, Serialize};

use super::{check_uuid, prune_uuids, AuthState, UuidData};
use crate::utils::{errors::ServiceError, Role};

#[derive(Deserialize, Serialize)]
struct User {
    uuid: String,
}

impl User {
    fn new(uuid: String) -> Self {
        Self { uuid }
    }
}

/// **Get generated UUID**
///
/// ```BASH
/// curl -X GET 'http://127.0.0.1:8787/api/generate-uuid' -H 'Authorization: Bearer <TOKEN>'
/// ```
#[post("/generate-uuid")]
#[protect(any("Role::Admin", "Role::User"), ty = "Role")]
async fn generate_uuid(data: web::Data<AuthState>) -> Result<impl Responder, ServiceError> {
    let mut uuids = data.uuids.lock().map_err(|e| e.to_string())?;
    let new_uuid = UuidData::new();
    let user_auth = User::new(new_uuid.uuid.to_string());

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
    let mut uuids = data.uuids.lock().map_err(|e| e.to_string())?;

    match check_uuid(&mut uuids, user.uuid.as_str()) {
        Ok(s) => Ok(web::Json(s)),
        Err(e) => Err(e),
    }
}
