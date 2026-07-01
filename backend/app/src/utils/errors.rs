use std::io;

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use derive_more::Display;

use crate::player::utils::probe::FfProbeError;

#[derive(Debug, Display)]
pub enum ServiceError {
    #[display("Internal Server Error")]
    InternalServerError,

    #[display("BadRequest: {_0}")]
    BadRequest(String),

    #[display("Conflict: {_0}")]
    Conflict(String),

    #[display("Forbidden: {_0}")]
    Forbidden(String),

    #[display("Unauthorized: {_0}")]
    Unauthorized(String),

    #[display("NoContent")]
    NoContent(),

    #[display("NotFound: {_0}")]
    NotFound(String),

    #[display("ServiceUnavailable: {_0}")]
    ServiceUnavailable(String),

    // 429 Too Many Requests
    #[display("ToManyRequests")]
    ToManyRequests,
}

impl IntoResponse for ServiceError {
    fn into_response(self) -> Response {
        match self {
            Self::InternalServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!("Internal Server Error.")),
            )
                .into_response(),
            Self::BadRequest(message) => {
                (StatusCode::BAD_REQUEST, Json(serde_json::json!(message))).into_response()
            }
            Self::Conflict(message) => {
                (StatusCode::CONFLICT, Json(serde_json::json!(message))).into_response()
            }
            Self::Forbidden(message) => {
                (StatusCode::FORBIDDEN, Json(serde_json::json!(message))).into_response()
            }
            Self::Unauthorized(message) => {
                (StatusCode::UNAUTHORIZED, Json(serde_json::json!(message))).into_response()
            }
            Self::NoContent() => StatusCode::NO_CONTENT.into_response(),
            Self::NotFound(message) => {
                (StatusCode::NOT_FOUND, Json(serde_json::json!(message))).into_response()
            }
            Self::ServiceUnavailable(message) => (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!(message)),
            )
                .into_response(),

            Self::ToManyRequests => (
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!("Too Many Requests.")),
            )
                .into_response(),
        }
    }
}

impl From<String> for ServiceError {
    fn from(err: String) -> Self {
        Self::BadRequest(err)
    }
}

impl From<axum::extract::multipart::MultipartError> for ServiceError {
    fn from(err: axum::extract::multipart::MultipartError) -> Self {
        Self::BadRequest(err.to_string())
    }
}

impl From<std::io::Error> for ServiceError {
    fn from(err: std::io::Error) -> Self {
        Self::Conflict(err.to_string())
    }
}
impl From<chrono::ParseError> for ServiceError {
    fn from(err: chrono::ParseError) -> Self {
        Self::Conflict(err.to_string())
    }
}

impl From<std::num::ParseIntError> for ServiceError {
    fn from(err: std::num::ParseIntError) -> Self {
        Self::BadRequest(err.to_string())
    }
}

impl From<jsonwebtoken::errors::Error> for ServiceError {
    fn from(err: jsonwebtoken::errors::Error) -> Self {
        Self::Unauthorized(err.to_string())
    }
}

impl From<sqlx::Error> for ServiceError {
    fn from(err: sqlx::Error) -> Self {
        Self::BadRequest(err.to_string())
    }
}

impl From<tokio::task::JoinError> for ServiceError {
    fn from(err: tokio::task::JoinError) -> Self {
        Self::BadRequest(err.to_string())
    }
}

impl From<toml_edit::ser::Error> for ServiceError {
    fn from(err: toml_edit::ser::Error) -> Self {
        Self::BadRequest(err.to_string())
    }
}

impl From<toml_edit::TomlError> for ServiceError {
    fn from(err: toml_edit::TomlError) -> Self {
        Self::BadRequest(err.to_string())
    }
}

impl From<uuid::Error> for ServiceError {
    fn from(err: uuid::Error) -> Self {
        Self::BadRequest(err.to_string())
    }
}

impl From<serde_json::Error> for ServiceError {
    fn from(err: serde_json::Error) -> Self {
        Self::BadRequest(err.to_string())
    }
}

impl From<&str> for ServiceError {
    fn from(err: &str) -> Self {
        Self::BadRequest(err.to_string())
    }
}

impl From<ProcessError> for ServiceError {
    fn from(err: ProcessError) -> Self {
        Self::BadRequest(err.to_string())
    }
}

#[derive(Debug, Display)]
pub enum ProcessError {
    #[display("Failed to spawn command: {_0}")]
    CommandSpawn(io::Error),
    #[display("{_0}")]
    Custom(String),
    #[display("DB error: {_0}")]
    DB(String),
    #[display("Input error: {_0}")]
    Input(String),
    #[display("IO error: {_0}")]
    IO(String),
    #[display("{_0}")]
    Ffprobe(String),
    #[display("Mail error: {_0}")]
    Mail(String),
    #[display("Regex compile error {_0}")]
    Regex(String),
}

impl From<std::io::Error> for ProcessError {
    fn from(err: std::io::Error) -> Self {
        Self::IO(err.to_string())
    }
}

impl From<FfProbeError> for ProcessError {
    fn from(err: FfProbeError) -> Self {
        Self::Ffprobe(err.to_string())
    }
}

impl From<lettre::address::AddressError> for ProcessError {
    fn from(err: lettre::address::AddressError) -> Self {
        Self::Mail(err.to_string())
    }
}

impl From<lettre::transport::smtp::Error> for ProcessError {
    fn from(err: lettre::transport::smtp::Error) -> Self {
        Self::Mail(err.to_string())
    }
}

impl From<lettre::error::Error> for ProcessError {
    fn from(err: lettre::error::Error) -> Self {
        Self::Mail(err.to_string())
    }
}

impl From<regex::Error> for ProcessError {
    fn from(err: regex::Error) -> Self {
        Self::Regex(err.to_string())
    }
}

impl From<serde_json::Error> for ProcessError {
    fn from(err: serde_json::Error) -> Self {
        Self::Custom(err.to_string())
    }
}

impl From<sqlx::Error> for ProcessError {
    fn from(err: sqlx::Error) -> Self {
        Self::DB(err.to_string())
    }
}

impl From<sqlx::migrate::MigrateError> for ProcessError {
    fn from(err: sqlx::migrate::MigrateError) -> Self {
        Self::DB(err.to_string())
    }
}

impl From<&str> for ProcessError {
    fn from(err: &str) -> Self {
        Self::Custom(err.to_string())
    }
}

impl From<inquire::InquireError> for ProcessError {
    fn from(err: inquire::InquireError) -> Self {
        Self::Input(err.to_string())
    }
}

impl From<ServiceError> for ProcessError {
    fn from(err: ServiceError) -> Self {
        Self::Custom(err.to_string())
    }
}
