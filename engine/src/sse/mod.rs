use std::{
    collections::HashSet,
    fmt,
    str::FromStr,
    time::{Duration, SystemTime},
};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::utils::errors::ServiceError;

pub mod broadcast;
pub mod routes;

#[derive(Debug, Clone, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Endpoint {
    Playout,
    System,
}

impl Default for Endpoint {
    fn default() -> Self {
        Self::System
    }
}

impl FromStr for Endpoint {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "playout" => Ok(Self::Playout),
            "system" => Ok(Self::System),
            _ => Err("Missing endpoint".to_string()),
        }
    }
}

impl fmt::Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Self::Playout => write!(f, "playout"),
            Self::System => write!(f, "system"),
        }
    }
}

#[derive(Debug, Eq, Hash, PartialEq, Clone, Copy)]
pub struct UuidData {
    pub uuid: Uuid,
    pub expiration: SystemTime,
}

impl UuidData {
    pub fn new() -> Self {
        Self {
            uuid: Uuid::new_v4(),
            expiration: SystemTime::now() + Duration::from_secs(2 * 3600), // 2 hours
        }
    }
}

impl Default for UuidData {
    fn default() -> Self {
        Self::new()
    }
}

pub struct SseAuthState {
    pub uuids: Mutex<HashSet<UuidData>>,
}

/// Remove all UUIDs from HashSet which are older the expiration time.
pub fn prune_uuids(uuids: &mut HashSet<UuidData>) {
    uuids.retain(|entry| entry.expiration > SystemTime::now());
}

pub fn check_uuid(uuids: &mut HashSet<UuidData>, uuid: &str) -> Result<&'static str, ServiceError> {
    let client_uuid = Uuid::parse_str(uuid)?;

    prune_uuids(uuids);

    match uuids.iter().find(|entry| entry.uuid == client_uuid) {
        Some(_) => Ok("UUID is valid"),
        None => Err(ServiceError::Unauthorized(
            "Invalid or expired UUID".to_string(),
        )),
    }
}
