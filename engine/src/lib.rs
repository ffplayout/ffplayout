use std::sync::{Arc, LazyLock, Mutex};

use actix_web::{dev::ServiceRequest, Error, HttpMessage};
use actix_web_grants::authorities::AttachAuthorities;
use actix_web_httpauth::extractors::bearer::BearerAuth;
use clap::Parser;
use sysinfo::{Disks, Networks, System};

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
pub static DISKS: LazyLock<Arc<Mutex<Disks>>> =
    LazyLock::new(|| Arc::new(Mutex::new(Disks::new_with_refreshed_list())));
pub static NETWORKS: LazyLock<Arc<Mutex<Networks>>> =
    LazyLock::new(|| Arc::new(Mutex::new(Networks::new_with_refreshed_list())));
pub static SYS: LazyLock<Arc<Mutex<System>>> =
    LazyLock::new(|| Arc::new(Mutex::new(System::new_all())));

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
