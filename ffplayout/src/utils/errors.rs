use actix_web::{error::ResponseError, Error, HttpResponse};
use derive_more::Display;

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
