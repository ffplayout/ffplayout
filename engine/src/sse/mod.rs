use std::{
    collections::HashSet,
    fmt,
    str::FromStr,
    sync::Arc,
    time::{Duration, SystemTime},
};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use ts_rs::TS;
use uuid::Uuid;

use crate::utils::errors::ServiceError;

pub mod broadcast;
pub mod routes;

#[derive(Debug, Clone, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Endpoint {
    Playout,
    #[default]
    System,
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

#[derive(Debug, Eq, Hash, PartialEq, Clone)]
pub struct UuidData {
    pub uuid: Uuid,
    pub expiration: SystemTime,
    pub ip_address: String,
    pub user_id: Option<i32>,
}

impl UuidData {
    pub fn new(ip_address: String, user_id: Option<i32>) -> Self {
        Self {
            uuid: Uuid::new_v4(),
            expiration: SystemTime::now() + Duration::from_secs(30 * 60),
            ip_address,
            user_id,
        }
    }
}

impl Default for UuidData {
    fn default() -> Self {
        Self::new(String::from("127.0.0.1"), None)
    }
}

#[derive(Debug, Default, Clone, TS)]
#[ts(export, export_to = "sse.d.ts")]
pub enum SSELevel {
    Error,
    #[default]
    Info,
    Success,
    Warning,
}

impl FromStr for SSELevel {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "error" => Ok(Self::Error),
            "info" => Ok(Self::Info),
            "success" => Ok(Self::Success),
            "warning" => Ok(Self::Warning),
            _ => Err(format!("Field '{input}' not found!")),
        }
    }
}

impl fmt::Display for SSELevel {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Self::Error => write!(f, "error"),
            Self::Info => write!(f, "info"),
            Self::Success => write!(f, "success"),
            Self::Warning => write!(f, "warning"),
        }
    }
}

#[derive(Debug, Clone, TS)]
#[ts(export, export_to = "sse.d.ts")]
pub struct SSEMessage {
    pub variance: SSELevel,
    pub text: String,
}

impl SSEMessage {
    pub fn new(variance: SSELevel, text: &str) -> Self {
        Self {
            variance,
            text: text.to_owned(),
        }
    }
}

impl fmt::Display for SSEMessage {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            r#"{{ "variance": "{}", "text": "{}"}}"#,
            self.variance, self.text
        )
    }
}

#[derive(Debug, Clone, Default)]
pub struct SseAuthState {
    pub uuids: Arc<Mutex<HashSet<UuidData>>>,
}

/// Remove all UUIDs from HashSet which are older the expiration time.
pub fn prune_uuids(uuids: &mut HashSet<UuidData>) {
    uuids.retain(|entry| entry.expiration > SystemTime::now());
}

pub fn check_uuid(
    uuids: &mut HashSet<UuidData>,
    uuid: &str,
    ip_address: &str,
) -> Result<&'static str, ServiceError> {
    let client_uuid =
        Uuid::parse_str(uuid).map_err(|_| ServiceError::Forbidden("Invalid UUID".to_string()))?;

    prune_uuids(uuids);

    match uuids.iter().find(|entry| entry.uuid == client_uuid) {
        Some(entry) => {
            if entry.ip_address != ip_address {
                return Err(ServiceError::Forbidden(
                    "UUID IP address mismatch".to_string(),
                ));
            }

            Ok("UUID is valid")
        }
        None => Err(ServiceError::Forbidden(
            "Invalid or expired UUID".to_string(),
        )),
    }
}
