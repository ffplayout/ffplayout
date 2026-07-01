use axum::{
    body::Body,
    http::{Method, Request, Response},
    middleware::Next,
};
use lazy_limit::HttpMethod;
use real::RealIp;

use crate::{db::models::UserMeta, utils::errors::ServiceError};

fn map_method(m: Method) -> HttpMethod {
    match m {
        Method::GET => HttpMethod::GET,
        Method::POST => HttpMethod::POST,
        Method::PUT => HttpMethod::PUT,
        Method::DELETE => HttpMethod::DELETE,
        Method::PATCH => HttpMethod::PATCH,
        Method::HEAD => HttpMethod::HEAD,
        Method::OPTIONS => HttpMethod::OPTIONS,
        Method::CONNECT => HttpMethod::CONNECT,
        Method::TRACE => HttpMethod::TRACE,
        _ => HttpMethod::OTHER,
    }
}

/// Applies request rate limiting based on client IP, path and HTTP method.
///
/// Authenticated users with a positive id bypass this check. Unauthenticated
/// requests are allowed only if `lazy_limit` permits the request; otherwise a
/// `TooManyRequests` error is returned.
pub async fn rate_limit(
    real_ip: RealIp,
    req: Request<Body>,
    next: Next,
) -> Result<Response<Body>, ServiceError> {
    let auth = req.extensions().get::<UserMeta>();
    let method = req.method().clone();

    let ip_str = real_ip.ip().to_string();
    let path = req.uri().path().to_string();

    if auth.is_some_and(|a| a.id > 0)
        || lazy_limit::limit_override!(&ip_str, &path, map_method(method)).await
    {
        let response = next.run(req).await;
        Ok(response)
    } else {
        Err(ServiceError::ToManyRequests)
    }
}
