use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use actix_web::{dev::ServiceRequest, Error, HttpMessage};
use actix_web_grants::authorities::AttachAuthorities;
use actix_web_httpauth::extractors::bearer::BearerAuth;
use clap::Parser;
use lazy_static::lazy_static;
use log::kv::Source;
use sysinfo::{Disks, Networks, System};

pub mod api;
pub mod db;
pub mod macros;
pub mod player;
pub mod sse;
pub mod utils;

use api::auth;
use db::models::UserMeta;
use utils::args_parse::Args;
use utils::{advanced_config::AdvancedConfig, errors::ServiceError};

lazy_static! {
    pub static ref ARGS: Args = Args::parse();
    pub static ref DISKS: Arc<Mutex<Disks>> =
        Arc::new(Mutex::new(Disks::new_with_refreshed_list()));
    pub static ref NETWORKS: Arc<Mutex<Networks>> =
        Arc::new(Mutex::new(Networks::new_with_refreshed_list()));
    pub static ref SYS: Arc<Mutex<System>> = Arc::new(Mutex::new(System::new_all()));
}

pub type SharedDurationData = Arc<Mutex<HashMap<String, f64>>>;

struct SharedDur {
    dur_data: SharedDurationData,
    limit_size: u64,
}

impl SharedDur {
    pub fn create(limit: u64) -> Self {
        Self {
            dur_data: Arc::new(Mutex::new(HashMap::new())),
            limit_size: limit,
        }
    }

    pub fn add_item(&self, key: String, value: f64) {
        let mut map = self.dur_data.lock().unwrap();
        if map.len() <= self.limit_size as usize {
            map.insert(key, value);
            map
        } else {
        }
    }
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
