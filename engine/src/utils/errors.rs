use std::io;

use actix_web::{error::ResponseError, Error, HttpResponse};
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

    #[display("NoContent: {_0}")]
    NoContent(String),

    #[display("ServiceUnavailable: {_0}")]
    ServiceUnavailable(String),
}

// impl ResponseError trait allows to convert our errors into http responses with appropriate data
impl ResponseError for ServiceError {
    fn error_response(&self) -> HttpResponse {
        match self {
            Self::InternalServerError => {
                HttpResponse::InternalServerError().json("Internal Server Error. Please try later.")
            }
            Self::BadRequest(ref message) => HttpResponse::BadRequest().json(message),
            Self::Conflict(ref message) => HttpResponse::Conflict().json(message),
            Self::Forbidden(ref message) => HttpResponse::Forbidden().json(message),
            Self::Unauthorized(ref message) => HttpResponse::Unauthorized().json(message),
            Self::NoContent(ref message) => HttpResponse::NoContent().json(message),
            Self::ServiceUnavailable(ref message) => {
                HttpResponse::ServiceUnavailable().json(message)
            }
        }
    }
}

impl From<String> for ServiceError {
    fn from(err: String) -> Self {
        Self::BadRequest(err)
    }
}

impl From<Error> for ServiceError {
    fn from(err: Error) -> Self {
        Self::BadRequest(err.to_string())
    }
}

impl From<actix_multipart::MultipartError> for ServiceError {
    fn from(err: actix_multipart::MultipartError) -> Self {
        Self::BadRequest(err.to_string())
    }
}

impl From<std::io::Error> for ServiceError {
    fn from(err: std::io::Error) -> Self {
        Self::NoContent(err.to_string())
    }
}
impl From<chrono::ParseError> for ServiceError {
    fn from(err: chrono::ParseError) -> Self {
        Self::NoContent(err.to_string())
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

impl From<actix_web::error::BlockingError> for ServiceError {
    fn from(err: actix_web::error::BlockingError) -> Self {
        Self::BadRequest(err.to_string())
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

#[derive(Debug, Display)]
pub enum ProcessError {
    #[display("Failed to spawn command: {_0}")]
    CommandSpawn(io::Error),
    #[display("{_0}")]
    Custom(String),
    #[display("IO error: {_0}")]
    IO(io::Error),
    #[display("{_0}")]
    Ffprobe(String),
    #[display("Regex compile error {_0}")]
    Regex(String),
    #[display("Thread error {_0}")]
    Thread(String),
}

impl From<std::io::Error> for ProcessError {
    fn from(err: std::io::Error) -> Self {
        Self::IO(err)
    }
}

impl From<FfProbeError> for ProcessError {
    fn from(err: FfProbeError) -> Self {
        Self::Ffprobe(err.to_string())
    }
}

impl From<lettre::address::AddressError> for ProcessError {
    fn from(err: lettre::address::AddressError) -> Self {
        Self::Custom(err.to_string())
    }
}

impl From<lettre::transport::smtp::Error> for ProcessError {
    fn from(err: lettre::transport::smtp::Error) -> Self {
        Self::Custom(err.to_string())
    }
}

impl From<lettre::error::Error> for ProcessError {
    fn from(err: lettre::error::Error) -> Self {
        Self::Custom(err.to_string())
    }
}

impl<T> From<std::sync::PoisonError<T>> for ProcessError {
    fn from(err: std::sync::PoisonError<T>) -> Self {
        Self::Custom(err.to_string())
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

impl From<Box<dyn std::any::Any + std::marker::Send>> for ProcessError {
    fn from(err: Box<dyn std::any::Any + std::marker::Send>) -> Self {
        Self::Thread(format!("{err:?}"))
    }
}
