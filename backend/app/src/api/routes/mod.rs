use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::{
    Json,
    body::Body,
    extract::FromRequestParts,
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{
            ACCEPT_RANGES, AUTHORIZATION, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE,
            CONTENT_TYPE, RANGE,
        },
        request::Parts,
    },
    response::{IntoResponse, Response},
};
use chrono_tz::Tz;
use protect_axum::authorities::{AuthDetails, AuthoritiesCheck};
use serde::{Deserialize, Serialize};
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncSeekExt},
    sync::Mutex,
};
use tokio_util::io::ReaderStream;

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

use crate::{
    db::models::Role,
    utils::{config::Template, errors::ServiceError, mail::MailQueue},
};

use super::auth::decode_jwt;

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
        .map(|value| parse_range(value, total_size))
        .unwrap_or(RangeSelection::Full);

    let mut file = File::open(path).await?;

    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    response_headers.insert(CONTENT_DISPOSITION, HeaderValue::from_static("attachment"));
    response_headers.insert(ACCEPT_RANGES, HeaderValue::from_static("bytes"));

    let (status, length) = match range {
        RangeSelection::Partial { start, end } => {
            file.seek(std::io::SeekFrom::Start(start)).await?;
            response_headers.insert(
                CONTENT_RANGE,
                HeaderValue::from_str(&format!("bytes {start}-{end}/{total_size}"))
                    .map_err(|_| ServiceError::InternalServerError)?,
            );

            (StatusCode::PARTIAL_CONTENT, end - start + 1)
        }
        RangeSelection::Full => (StatusCode::OK, total_size),
        RangeSelection::Unsatisfiable => {
            response_headers.insert(
                CONTENT_RANGE,
                HeaderValue::from_str(&format!("bytes */{total_size}"))
                    .map_err(|_| ServiceError::InternalServerError)?,
            );
            response_headers.insert(CONTENT_LENGTH, HeaderValue::from_static("0"));

            return Ok((
                StatusCode::RANGE_NOT_SATISFIABLE,
                response_headers,
                Body::empty(),
            )
                .into_response());
        }
    };

    response_headers.insert(CONTENT_LENGTH, length.into());

    let stream = ReaderStream::new(file.take(length));

    Ok((status, response_headers, Body::from_stream(stream)).into_response())
}

#[derive(Debug, PartialEq, Eq)]
enum RangeSelection {
    Full,
    Partial { start: u64, end: u64 },
    Unsatisfiable,
}

/// Unsupported or malformed ranges are ignored; valid ranges that cannot
/// select any bytes produce a 416 response.
fn parse_range(value: &str, total_size: u64) -> RangeSelection {
    let Some(spec) = value.strip_prefix("bytes=") else {
        return RangeSelection::Full;
    };

    if spec.contains(',') {
        return RangeSelection::Full;
    }

    let Some((start, end)) = spec.split_once('-') else {
        return RangeSelection::Full;
    };
    let (start, end) = (start.trim(), end.trim());
    let bounds = match (start, end) {
        ("", "") => return RangeSelection::Full,
        ("", suffix) => {
            let Ok(suffix) = suffix.parse::<u64>() else {
                return RangeSelection::Full;
            };

            if suffix == 0 || total_size == 0 {
                return RangeSelection::Unsatisfiable;
            }

            (total_size.saturating_sub(suffix), total_size - 1)
        }
        (start, end) => {
            let Ok(start) = start.parse::<u64>() else {
                return RangeSelection::Full;
            };
            let end = if end.is_empty() {
                total_size.saturating_sub(1)
            } else {
                let Ok(end) = end.parse::<u64>() else {
                    return RangeSelection::Full;
                };

                end
            };

            if total_size == 0 || start >= total_size || start > end {
                return RangeSelection::Unsatisfiable;
            }

            (start, end.min(total_size - 1))
        }
    };

    RangeSelection::Partial {
        start: bounds.0,
        end: bounds.1,
    }
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

    pub fn ensure_channel_or_admin(&self, channel_id: i32) -> Result<(), ServiceError> {
        if self.is_global_admin() || self.channels.contains(&channel_id) {
            Ok(())
        } else {
            Err(ServiceError::Forbidden("Forbidden for channel".to_string()))
        }
    }

    pub fn ensure_self_or_admin(&self, user_id: i32) -> Result<(), ServiceError> {
        if self.is_global_admin() || self.id == user_id {
            Ok(())
        } else {
            Err(ServiceError::Forbidden("Forbidden for user".to_string()))
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
    #[serde(default)]
    shuffle: bool,
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

#[cfg(test)]
mod tests {
    use axum::body::to_bytes;

    use super::*;

    #[test]
    fn parses_closed_open_and_suffix_ranges_and_ignores_unsupported_headers() {
        for (header, expected) in [
            ("bytes=2-4", RangeSelection::Partial { start: 2, end: 4 }),
            ("bytes=2-", RangeSelection::Partial { start: 2, end: 9 }),
            ("bytes=-3", RangeSelection::Partial { start: 7, end: 9 }),
            ("bytes=-99", RangeSelection::Partial { start: 0, end: 9 }),
            ("bytes=2-99", RangeSelection::Partial { start: 2, end: 9 }),
            ("bytes=10-", RangeSelection::Unsatisfiable),
            ("bytes=4-2", RangeSelection::Unsatisfiable),
            ("bytes=-0", RangeSelection::Unsatisfiable),
            ("bytes=-", RangeSelection::Full),
            ("bytes=abc-def", RangeSelection::Full),
            ("bytes=0-1,4-5", RangeSelection::Full),
            ("other=0-1", RangeSelection::Full),
        ] {
            assert_eq!(parse_range(header, 10), expected, "{header}");
        }

        assert_eq!(parse_range("bytes=0-", 0), RangeSelection::Unsatisfiable);
        assert_eq!(parse_range("bytes=-1", 0), RangeSelection::Unsatisfiable);
    }

    #[tokio::test]
    async fn streams_exact_range_bytes_headers_and_empty_responses() {
        let path = std::env::temp_dir().join(format!("ffplayout-range-{}", uuid::Uuid::new_v4()));
        tokio::fs::write(&path, b"0123456789").await.unwrap();

        for (range, status, expected, content_range) in [
            (None, StatusCode::OK, "0123456789", None),
            (
                Some("bytes=2-4"),
                StatusCode::PARTIAL_CONTENT,
                "234",
                Some("bytes 2-4/10"),
            ),
            (
                Some("bytes=7-"),
                StatusCode::PARTIAL_CONTENT,
                "789",
                Some("bytes 7-9/10"),
            ),
            (
                Some("bytes=-2"),
                StatusCode::PARTIAL_CONTENT,
                "89",
                Some("bytes 8-9/10"),
            ),
            (
                Some("bytes=10-"),
                StatusCode::RANGE_NOT_SATISFIABLE,
                "",
                Some("bytes */10"),
            ),
            (Some("bytes=bad"), StatusCode::OK, "0123456789", None),
            (Some("bytes=0-1,4-5"), StatusCode::OK, "0123456789", None),
        ] {
            let mut headers = HeaderMap::new();

            if let Some(range) = range {
                headers.insert(RANGE, HeaderValue::from_static(range));
            }

            let response = stream_file(&path, &headers).await.unwrap();

            assert_eq!(response.status(), status);
            assert_eq!(
                response.headers()[CONTENT_LENGTH],
                expected.len().to_string()
            );
            assert_eq!(response.headers()[ACCEPT_RANGES], "bytes");
            assert_eq!(
                response
                    .headers()
                    .get(CONTENT_RANGE)
                    .map(|value| value.to_str().unwrap()),
                content_range
            );
            assert_eq!(
                to_bytes(response.into_body(), 100).await.unwrap().as_ref(),
                expected.as_bytes()
            );
        }

        tokio::fs::write(&path, b"").await.unwrap();
        let response = stream_file(&path, &HeaderMap::new()).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CONTENT_LENGTH], "0");
        let mut headers = HeaderMap::new();
        headers.insert(RANGE, HeaderValue::from_static("bytes=0-"));
        let response = stream_file(&path, &headers).await.unwrap();

        assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(response.headers()[CONTENT_RANGE], "bytes */0");
        assert!(
            to_bytes(response.into_body(), 100)
                .await
                .unwrap()
                .is_empty()
        );
        tokio::fs::remove_file(path).await.unwrap();
    }
}
