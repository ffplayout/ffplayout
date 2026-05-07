use std::{collections::HashSet, sync::LazyLock};

use axum::{
    extract::Request,
    response::{IntoResponse, Response},
};
use clap::Parser;
use log::{error, warn};

pub mod api;
pub mod db;
pub mod file;
pub mod macros;
pub mod middleware;
pub mod player;
pub mod sse;
pub mod utils;

#[cfg(not(debug_assertions))]
pub mod serve;

use api::auth;
use db::models::{Role, UserMeta};
use utils::{advanced_config::AdvancedConfig, args_parse::Args, errors::ServiceError};

pub static ARGS: LazyLock<Args> = LazyLock::new(Args::parse);

pub async fn extract(req: &mut Request) -> Result<HashSet<Role>, Response> {
    let Some(auth) = req.headers().get("authorization") else {
        req.extensions_mut().insert(UserMeta::new(-1, vec![]));
        return Ok(HashSet::from([Role::Guest]));
    };

    let Some((scheme, token)) = auth
        .to_str()
        .ok()
        .and_then(|value| value.trim().split_once(' '))
    else {
        warn!("Malformed or invalid authorization header");
        return Err(ServiceError::Unauthorized("Unauthorized".to_string()).into_response());
    };

    if !scheme.eq_ignore_ascii_case("bearer") {
        warn!("Unsupported authorization scheme: {scheme}");
        return Err(ServiceError::Unauthorized("Unauthorized".to_string()).into_response());
    }

    match auth::decode_jwt(token).await {
        Ok(claims) => {
            let mut authorities = HashSet::with_capacity(1);
            authorities.insert(claims.role.clone());

            req.extensions_mut()
                .insert(UserMeta::new(claims.id, claims.channels));

            Ok(authorities)
        }
        Err(e) => {
            error!("JWT decode error: {e:?}");
            Err(ServiceError::Unauthorized("Unauthorized".to_string()).into_response())
        }
    }
}
