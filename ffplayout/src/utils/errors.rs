use std::io;

use actix_web::{error::ResponseError, Error, HttpResponse};
use derive_more::Display;
use ffprobe::FfProbeError;

#[derive(Debug, Display)]
pub enum ServiceError {
    #[display(fmt = "Internal Server Error")]
    InternalServerError,

    #[display(fmt = "BadRequest: {_0}")]
    BadRequest(String),

    #[display(fmt = "Conflict: {_0}")]
    Conflict(String),

    #[display(fmt = "Forbidden: {_0}")]
    Forbidden(String),

    #[display(fmt = "Unauthorized: {_0}")]
    Unauthorized(String),

    #[display(fmt = "NoContent: {_0}")]
    NoContent(String),

    #[display(fmt = "ServiceUnavailable: {_0}")]
    ServiceUnavailable(String),
}

// impl ResponseError trait allows to convert our errors into http responses with appropriate data
impl ResponseError for ServiceError {
    fn error_response(&self) -> HttpResponse {
        match self {
            ServiceError::InternalServerError => {
                HttpResponse::InternalServerError().json("Internal Server Error. Please try later.")
            }
            ServiceError::BadRequest(ref message) => HttpResponse::BadRequest().json(message),
            ServiceError::Conflict(ref message) => HttpResponse::Conflict().json(message),
            ServiceError::Forbidden(ref message) => HttpResponse::Forbidden().json(message),
            ServiceError::Unauthorized(ref message) => HttpResponse::Unauthorized().json(message),
            ServiceError::NoContent(ref message) => HttpResponse::NoContent().json(message),
            ServiceError::ServiceUnavailable(ref message) => {
                HttpResponse::ServiceUnavailable().json(message)
            }
        }
    }
}

impl From<String> for ServiceError {
    fn from(err: String) -> ServiceError {
        ServiceError::BadRequest(err)
    }
}

impl From<Error> for ServiceError {
    fn from(err: Error) -> ServiceError {
        ServiceError::BadRequest(err.to_string())
    }
}

impl From<actix_multipart::MultipartError> for ServiceError {
    fn from(err: actix_multipart::MultipartError) -> ServiceError {
        ServiceError::BadRequest(err.to_string())
    }
}

impl From<std::io::Error> for ServiceError {
    fn from(err: std::io::Error) -> ServiceError {
        ServiceError::NoContent(err.to_string())
    }
}

impl From<std::num::ParseIntError> for ServiceError {
    fn from(err: std::num::ParseIntError) -> ServiceError {
        ServiceError::BadRequest(err.to_string())
    }
}

impl From<actix_web::error::BlockingError> for ServiceError {
    fn from(err: actix_web::error::BlockingError) -> ServiceError {
        ServiceError::BadRequest(err.to_string())
    }
}

impl From<sqlx::Error> for ServiceError {
    fn from(err: sqlx::Error) -> ServiceError {
        ServiceError::BadRequest(err.to_string())
    }
}

impl From<tokio::task::JoinError> for ServiceError {
    fn from(err: tokio::task::JoinError) -> ServiceError {
        ServiceError::BadRequest(err.to_string())
    }
}

impl From<toml_edit::ser::Error> for ServiceError {
    fn from(err: toml_edit::ser::Error) -> ServiceError {
        ServiceError::BadRequest(err.to_string())
    }
}

impl From<uuid::Error> for ServiceError {
    fn from(err: uuid::Error) -> ServiceError {
        ServiceError::BadRequest(err.to_string())
    }
}

#[derive(Debug, Display)]
pub enum ProcessError {
    #[display(fmt = "Failed to spawn ffmpeg/ffprobe. {}", _0)]
    CommandSpawn(io::Error),
    #[display(fmt = "{}", _0)]
    Custom(String),
    #[display(fmt = "IO error: {}", _0)]
    IO(io::Error),
    #[display(fmt = "{}", _0)]
    Ffprobe(FfProbeError),
    #[display(fmt = "Regex compile error {}", _0)]
    Regex(String),
    #[display(fmt = "Thread error {}", _0)]
    Thread(String),
}

impl From<std::io::Error> for ProcessError {
    fn from(err: std::io::Error) -> ProcessError {
        ProcessError::IO(err)
    }
}

impl From<FfProbeError> for ProcessError {
    fn from(err: FfProbeError) -> Self {
        Self::Ffprobe(err)
    }
}

impl From<lettre::address::AddressError> for ProcessError {
    fn from(err: lettre::address::AddressError) -> ProcessError {
        ProcessError::Custom(err.to_string())
    }
}

impl From<lettre::transport::smtp::Error> for ProcessError {
    fn from(err: lettre::transport::smtp::Error) -> ProcessError {
        ProcessError::Custom(err.to_string())
    }
}

impl From<lettre::error::Error> for ProcessError {
    fn from(err: lettre::error::Error) -> ProcessError {
        ProcessError::Custom(err.to_string())
    }
}

impl<T> From<std::sync::PoisonError<T>> for ProcessError {
    fn from(err: std::sync::PoisonError<T>) -> ProcessError {
        ProcessError::Custom(err.to_string())
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
