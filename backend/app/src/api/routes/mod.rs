use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::{
    Json,
    body::Body,
    extract::FromRequestParts,
    http::{
        HeaderMap, StatusCode,
        header::{
            ACCEPT_RANGES, AUTHORIZATION, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE,
            CONTENT_TYPE, RANGE,
        },
        request::Parts,
    },
    response::{IntoResponse, Response},
};
use chrono::NaiveDateTime;
use chrono_tz::Tz;
use protect_axum::authorities::{AuthDetails, AuthoritiesCheck};
use serde::{Deserialize, Serialize};
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncSeekExt},
    sync::Mutex,
};
use tokio_util::io::ReaderStream;

use super::auth::decode_jwt;
use crate::db::models::Role;
use crate::utils::mail::MailQueue;
use crate::utils::{config::Template, errors::ServiceError, optional_naive_date_time_from_str};

mod channel;
mod control;
mod file;
mod global;
mod log;
mod playlist;
mod playout_config;
mod presets;
mod program;
mod public;
mod setup;
mod system;
mod user;

pub use channel::*;
pub use control::*;
pub use file::*;
pub use global::*;
pub use log::*;
pub use playlist::*;
pub use playout_config::*;
pub use presets::*;
pub use program::*;
pub use public::*;
pub use setup::*;
pub use system::*;
pub use user::*;

pub type MailQueues = Arc<Mutex<Vec<Arc<Mutex<MailQueue>>>>>;

/// Streams a file to the client instead of loading it fully into memory, and
/// honours a single HTTP `Range` request so media players can seek. This keeps
/// memory usage bounded even for multi-gigabyte video files under many
/// concurrent requests.
pub async fn stream_file(path: &Path, headers: &HeaderMap) -> Result<Response, ServiceError> {
    let metadata = tokio::fs::metadata(path).await?;
    let total_size = metadata.len();

    let range = headers
        .get(RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| parse_range(value, total_size));

    let mut file = File::open(path).await?;

    let mut response_headers = HeaderMap::new();
    response_headers.insert(CONTENT_TYPE, "application/octet-stream".parse().unwrap());
    response_headers.insert(CONTENT_DISPOSITION, "attachment".parse().unwrap());
    response_headers.insert(ACCEPT_RANGES, "bytes".parse().unwrap());

    let (status, start, length) = match range {
        Some((start, end)) => {
            file.seek(std::io::SeekFrom::Start(start)).await?;
            response_headers.insert(
                CONTENT_RANGE,
                format!("bytes {start}-{end}/{total_size}").parse().unwrap(),
            );
            (StatusCode::PARTIAL_CONTENT, start, end - start + 1)
        }
        None => (StatusCode::OK, 0, total_size),
    };

    response_headers.insert(CONTENT_LENGTH, length.into());
    let _ = start;

    let stream = ReaderStream::new(file.take(length));
    Ok((status, response_headers, Body::from_stream(stream)).into_response())
}

/// Parses a single `bytes=start-end` range against the known file size.
/// Returns the inclusive `(start, end)` byte offsets, or `None` when the range
/// is unsatisfiable or uses an unsupported (multi-range) form.
fn parse_range(value: &str, total_size: u64) -> Option<(u64, u64)> {
    if total_size == 0 {
        return None;
    }

    let spec = value.strip_prefix("bytes=")?;
    if spec.contains(',') {
        return None;
    }

    let (start, end) = spec.split_once('-')?;
    let last = total_size - 1;

    let (start, end) = match (start.trim(), end.trim()) {
        ("", "") => return None,
        // Suffix range: last N bytes.
        ("", suffix) => {
            let suffix: u64 = suffix.parse().ok()?;
            if suffix == 0 {
                return None;
            }
            (total_size.saturating_sub(suffix), last)
        }
        (start, "") => (start.parse().ok()?, last),
        (start, end) => (start.parse().ok()?, end.parse::<u64>().ok()?.min(last)),
    };

    if start > end || start > last {
        return None;
    }

    Some((start, end))
}

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
    #[serde(default, deserialize_with = "optional_naive_date_time_from_str")]
    start_after: Option<NaiveDateTime>,
    #[serde(default, deserialize_with = "optional_naive_date_time_from_str")]
    start_before: Option<NaiveDateTime>,
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
