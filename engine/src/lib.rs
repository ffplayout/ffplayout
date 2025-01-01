use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use actix_web::{dev::ServiceRequest, Error, HttpMessage};
use actix_web_grants::authorities::AttachAuthorities;
use actix_web_httpauth::extractors::bearer::BearerAuth;
use clap::Parser;
use lazy_static::lazy_static;
use sysinfo::{Disks, Networks, System};

pub mod api;
pub mod db;
pub mod macros;
pub mod player;
pub mod sse;
pub mod utils;

use api::auth;
use db::models::UserMeta;
use utils::advanced_config::AdvancedConfig;
use utils::args_parse::Args;

lazy_static! {
    pub static ref ARGS: Args = Args::parse();
    pub static ref DISKS: Arc<Mutex<Disks>> =
        Arc::new(Mutex::new(Disks::new_with_refreshed_list()));
    pub static ref NETWORKS: Arc<Mutex<Networks>> =
        Arc::new(Mutex::new(Networks::new_with_refreshed_list()));
    pub static ref SYS: Arc<Mutex<System>> = Arc::new(Mutex::new(System::new_all()));
}

pub type SharedDurationData = Arc<Mutex<HashMap<String, f64>>>;

pub fn create_shared_dur_data() -> SharedDurationData {
    Arc::new(Mutex::new(HashMap::new()))
}

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
