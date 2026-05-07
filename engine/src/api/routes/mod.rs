use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    Json,
    extract::FromRequestParts,
    http::{StatusCode, header::AUTHORIZATION, request::Parts},
};
use chrono::{Datelike, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use protect_axum::authorities::{AuthDetails, AuthoritiesCheck};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use super::auth::decode_jwt;
use crate::db::models::Role;
use crate::utils::mail::MailQueue;
use crate::utils::{config::Template, errors::ServiceError, naive_date_time_from_str};

mod channel;
mod control;
mod file;
mod log;
mod playlist;
mod playout_advanced;
mod playout_config;
mod presets;
mod program;
mod public;
mod system;
mod user;

pub use channel::*;
pub use control::*;
pub use file::*;
pub use log::*;
pub use playlist::*;
pub use playout_advanced::*;
pub use playout_config::*;
pub use presets::*;
pub use program::*;
pub use public::*;
pub use system::*;
pub use user::*;

pub type MailQueues = Arc<Mutex<Vec<Arc<Mutex<MailQueue>>>>>;

pub fn ensure_any_authority(
    details: &AuthDetails<Role>,
    roles: &[&Role],
) -> Result<(), ServiceError> {
    if details.has_any_authority(roles) {
        Ok(())
    } else {
        Err(ServiceError::Forbidden(
            "Insufficient permissions".to_string(),
        ))
    }
}

#[derive(Clone, Debug)]
pub struct AuthUser {
    pub id: i32,
    pub channels: Vec<i32>,
    pub role: Role,
}

impl AuthUser {
    pub fn is_global_admin(&self) -> bool {
        self.role == Role::GlobalAdmin
    }

    pub fn ensure_channel_or_admin(
        &self,
        channel_id: i32,
    ) -> Result<(), crate::utils::errors::ServiceError> {
        if self.is_global_admin() || self.channels.contains(&channel_id) {
            Ok(())
        } else {
            Err(crate::utils::errors::ServiceError::Forbidden(
                "Forbidden for channel".to_string(),
            ))
        }
    }

    pub fn ensure_self_or_admin(
        &self,
        user_id: i32,
    ) -> Result<(), crate::utils::errors::ServiceError> {
        if self.is_global_admin() || self.id == user_id {
            Ok(())
        } else {
            Err(crate::utils::errors::ServiceError::Forbidden(
                "Forbidden for user".to_string(),
            ))
        }
    }
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, Json<serde_json::Value>);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let Some(value) = parts.headers.get(AUTHORIZATION) else {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "detail": "Missing authorization header" })),
            ));
        };

        let Ok(token) = value.to_str() else {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "detail": "Invalid authorization header" })),
            ));
        };

        let Some(token) = token.strip_prefix("Bearer ") else {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "detail": "Missing bearer token" })),
            ));
        };

        let claims = decode_jwt(token).await.map_err(|e| {
            (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "detail": e.to_string() })),
            )
        })?;

        Ok(Self {
            id: claims.id,
            channels: claims.channels,
            role: claims.role,
        })
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DateObj {
    #[serde(default)]
    date: String,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct PathsObj {
    #[serde(default)]
    paths: Option<Vec<String>>,
    template: Option<Template>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct FileObj {
    #[serde(default)]
    path: PathBuf,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LogReq {
    #[serde(default)]
    date: String,
    #[serde(default)]
    timezone: Tz,
    #[serde(default)]
    download: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ImportObj {
    #[serde(default)]
    file: PathBuf,
    #[serde(default)]
    date: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ProgramObj {
    #[serde(default = "time_after", deserialize_with = "naive_date_time_from_str")]
    start_after: NaiveDateTime,
    #[serde(default = "time_before", deserialize_with = "naive_date_time_from_str")]
    start_before: NaiveDateTime,
}

fn time_after() -> NaiveDateTime {
    let today = Utc::now();

    chrono::Local
        .with_ymd_and_hms(today.year(), today.month(), today.day(), 0, 0, 0)
        .unwrap()
        .naive_local()
}

fn time_before() -> NaiveDateTime {
    let today = Utc::now();

    chrono::Local
        .with_ymd_and_hms(today.year(), today.month(), today.day(), 23, 59, 59)
        .unwrap()
        .naive_local()
}

#[derive(Debug, Serialize)]
pub struct ProgramItem {
    source: String,
    start: String,
    title: Option<String>,
    r#in: f64,
    out: f64,
    duration: f64,
    category: String,
}
