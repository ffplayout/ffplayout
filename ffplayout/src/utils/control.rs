use std::{collections::HashMap, fmt, str::FromStr, sync::atomic::AtomicBool};

use reqwest::{header::AUTHORIZATION, Client, Response};
use serde::{Deserialize, Serialize};
use tokio::{process::Child, sync::Mutex};

use crate::utils::{config::PlayoutConfig, errors::ServiceError};

#[derive(Debug, Deserialize, Serialize, Clone)]
struct TextParams {
    control: String,
    message: HashMap<String, String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ControlParams {
    pub control: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct MediaParams {
    media: String,
}

/// ffplayout engine process
///
/// When running not on Linux, or with environment variable `PIGGYBACK_MODE=true`,
/// the engine get startet and controlled from ffpapi
pub struct ProcessControl {
    pub engine_child: Mutex<Option<Child>>,
    pub is_running: AtomicBool,
    pub piggyback: AtomicBool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ServiceCmd {
    Enable,
    Disable,
    Start,
    Stop,
    Restart,
    Status,
}

impl FromStr for ServiceCmd {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input.to_lowercase().as_str() {
            "enable" => Ok(Self::Enable),
            "disable" => Ok(Self::Disable),
            "start" => Ok(Self::Start),
            "stop" => Ok(Self::Stop),
            "restart" => Ok(Self::Restart),
            "status" => Ok(Self::Status),
            _ => Err(format!("Command '{input}' not found!")),
        }
    }
}

impl fmt::Display for ServiceCmd {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Self::Enable => write!(f, "enable"),
            Self::Disable => write!(f, "disable"),
            Self::Start => write!(f, "start"),
            Self::Stop => write!(f, "stop"),
            Self::Restart => write!(f, "restart"),
            Self::Status => write!(f, "status"),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Process {
    pub command: ServiceCmd,
}

async fn post_request<T>(config: &PlayoutConfig, obj: T) -> Result<Response, ServiceError>
where
    T: Serialize,
{
    let url = format!("http://{}", config.rpc_server.address);
    let client = Client::new();

    match client
        .post(&url)
        .header(AUTHORIZATION, &config.rpc_server.authorization)
        .json(&obj)
        .send()
        .await
    {
        Ok(result) => Ok(result),
        Err(e) => Err(ServiceError::ServiceUnavailable(e.to_string())),
    }
}

pub async fn send_message(
    config: &PlayoutConfig,
    message: HashMap<String, String>,
) -> Result<Response, ServiceError> {
    let json_obj = TextParams {
        control: "text".into(),
        message,
    };

    post_request(config, json_obj).await
}

pub async fn control_state(
    config: &PlayoutConfig,
    command: &str,
) -> Result<Response, ServiceError> {
    let json_obj = ControlParams {
        control: command.to_owned(),
    };

    post_request(config, json_obj).await
}

pub async fn media_info(config: &PlayoutConfig, command: String) -> Result<Response, ServiceError> {
    let json_obj = MediaParams { media: command };

    post_request(config, json_obj).await
}
