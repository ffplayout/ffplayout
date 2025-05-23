use std::sync::LazyLock;

use actix_web::{Error, HttpMessage, dev::ServiceRequest};
use actix_web_grants::authorities::AttachAuthorities;
use actix_web_httpauth::extractors::bearer::BearerAuth;
use clap::Parser;

pub mod api;
pub mod db;
pub mod file;
pub mod macros;
pub mod player;
pub mod sse;
pub mod utils;

use api::auth;
use db::models::UserMeta;
use utils::advanced_config::AdvancedConfig;
use utils::args_parse::Args;

pub static ARGS: LazyLock<Args> = LazyLock::new(Args::parse);

pub async fn validator(
    req: ServiceRequest,
    credentials: BearerAuth,
) -> Result<ServiceRequest, (Error, ServiceRequest)> {
    // We just get permissions from JWT
    match auth::decode_jwt(credentials.token()).await {
        Ok(claims) => {
            req.attach(vec![claims.role]);

            req.extensions_mut()
                .insert(UserMeta::new(claims.id, claims.channels));

            Ok(req)
        }
        Err(e) => Err((e, req)),
    }
}
