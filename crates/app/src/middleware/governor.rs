use axum::{
    body::Body,
    http::{Method, Request, Response},
    middleware::Next,
};
use lazy_limit::HttpMethod;
use real::RealIp;

use crate::utils::errors::ServiceError;

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
/// This runs before authentication, so it protects the login, verification
/// and refresh routes. Requests are allowed only if `lazy_limit` permits them;
/// otherwise a `TooManyRequests` error is returned.
pub async fn rate_limit(
    real_ip: RealIp,
    req: Request<Body>,
    next: Next,
) -> Result<Response<Body>, ServiceError> {
    let method = req.method().clone();
    let ip_str = real_ip.ip().to_string();
    let path = req.uri().path().to_string();

    if lazy_limit::limit_override!(&ip_str, &path, map_method(method)).await {
        Ok(next.run(req).await)
    } else {
        Err(ServiceError::ToManyRequests)
    }
}
