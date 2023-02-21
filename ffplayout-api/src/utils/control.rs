use std::{
    collections::HashMap,
    env, fmt,
    str::FromStr,
    sync::atomic::{AtomicBool, Ordering},
};

use actix_web::web;
use reqwest::{
    header::{HeaderMap, AUTHORIZATION, CONTENT_TYPE},
    Client, Response,
};
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};
use tokio::{
    process::{Child, Command},
    sync::Mutex,
};

use crate::db::handles::select_channel;
use crate::utils::{errors::ServiceError, playout_config};
use ffplayout_lib::vec_strings;

#[derive(Debug, Deserialize, Serialize, Clone)]
struct RpcObj<T> {
    jsonrpc: String,
    id: i32,
    method: String,
    params: T,
}

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

impl<T> RpcObj<T> {
    fn new(id: i32, method: String, params: T) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            method,
            params,
        }
    }
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

impl ProcessControl {
    pub fn new() -> Self {
        let piggyback = if env::consts::OS != "linux" || env::var("PIGGYBACK_MODE").is_ok() {
            AtomicBool::new(true)
        } else {
            AtomicBool::new(false)
        };

        Self {
            engine_child: Mutex::new(None),
            is_running: AtomicBool::new(false),
            piggyback,
        }
    }
}

impl ProcessControl {
    pub async fn start(&self) -> Result<String, ServiceError> {
        #[cfg(not(debug_assertions))]
        let engine_path = "ffplayout";

        #[cfg(debug_assertions)]
        let engine_path = "./target/debug/ffplayout";

        match Command::new(engine_path).kill_on_drop(true).spawn() {
            Ok(proc) => *self.engine_child.lock().await = Some(proc),
            Err(_) => return Err(ServiceError::InternalServerError),
        };

        self.is_running.store(true, Ordering::SeqCst);

        Ok("Success".to_string())
    }

    pub async fn stop(&self) -> Result<String, ServiceError> {
        if let Some(proc) = self.engine_child.lock().await.as_mut() {
            if proc.kill().await.is_err() {
                return Err(ServiceError::InternalServerError);
            };
        }

        self.wait().await?;
        self.is_running.store(false, Ordering::SeqCst);

        Ok("Success".to_string())
    }

    pub async fn restart(&self) -> Result<String, ServiceError> {
        self.stop().await?;
        self.start().await?;

        self.is_running.store(true, Ordering::SeqCst);

        Ok("Success".to_string())
    }

    /// Wait for process to proper close.
    /// This prevents orphaned/zombi processes in system
    pub async fn wait(&self) -> Result<String, ServiceError> {
        if let Some(proc) = self.engine_child.lock().await.as_mut() {
            if proc.wait().await.is_err() {
                return Err(ServiceError::InternalServerError);
            };
        }

        Ok("Success".to_string())
    }

    pub fn status(&self) -> Result<String, ServiceError> {
        if self.is_running.load(Ordering::SeqCst) {
            Ok("active".to_string())
        } else {
            Ok("not running".to_string())
        }
    }
}

impl Default for ProcessControl {
    fn default() -> Self {
        Self::new()
    }
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

struct SystemD {
    service: String,
    cmd: Vec<String>,
}

impl SystemD {
    async fn new(conn: &Pool<Sqlite>, id: i32) -> Result<Self, ServiceError> {
        let channel = select_channel(conn, &id).await?;

        Ok(Self {
            service: channel.service,
            cmd: vec_strings!["/usr/bin/systemctl"],
        })
    }

    fn enable(mut self) -> Result<String, ServiceError> {
        self.cmd
            .append(&mut vec!["enable".to_string(), self.service]);

        Command::new("sudo").args(self.cmd).spawn()?;

        Ok("Success".to_string())
    }

    fn disable(mut self) -> Result<String, ServiceError> {
        self.cmd
            .append(&mut vec!["disable".to_string(), self.service]);

        Command::new("sudo").args(self.cmd).spawn()?;

        Ok("Success".to_string())
    }

    fn start(mut self) -> Result<String, ServiceError> {
        self.cmd
            .append(&mut vec!["start".to_string(), self.service]);

        Command::new("sudo").args(self.cmd).spawn()?;

        Ok("Success".to_string())
    }

    fn stop(mut self) -> Result<String, ServiceError> {
        self.cmd.append(&mut vec!["stop".to_string(), self.service]);

        Command::new("sudo").args(self.cmd).spawn()?;

        Ok("Success".to_string())
    }

    fn restart(mut self) -> Result<String, ServiceError> {
        self.cmd
            .append(&mut vec!["restart".to_string(), self.service]);

        Command::new("sudo").args(self.cmd).spawn()?;

        Ok("Success".to_string())
    }

    async fn status(mut self) -> Result<String, ServiceError> {
        self.cmd
            .append(&mut vec!["is-active".to_string(), self.service]);

        let output = Command::new("sudo").args(self.cmd).output().await?;

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

fn create_header(auth: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        CONTENT_TYPE,
        "Content-Type: application/json".parse().unwrap(),
    );
    headers.insert(AUTHORIZATION, auth.parse().unwrap());

    headers
}

async fn post_request<T>(
    conn: &Pool<Sqlite>,
    id: i32,
    obj: RpcObj<T>,
) -> Result<Response, ServiceError>
where
    T: Serialize,
{
    let (config, _) = playout_config(conn, &id).await?;
    let url = format!("http://{}", config.rpc_server.address);
    let client = Client::new();

    match client
        .post(&url)
        .headers(create_header(&config.rpc_server.authorization))
        .json(&obj)
        .send()
        .await
    {
        Ok(result) => Ok(result),
        Err(e) => Err(ServiceError::ServiceUnavailable(e.to_string())),
    }
}

pub async fn send_message(
    conn: &Pool<Sqlite>,
    id: i32,
    message: HashMap<String, String>,
) -> Result<Response, ServiceError> {
    let json_obj = RpcObj::new(
        id,
        "player".into(),
        TextParams {
            control: "text".into(),
            message,
        },
    );

    post_request(conn, id, json_obj).await
}

pub async fn control_state(
    conn: &Pool<Sqlite>,
    id: i32,
    command: &str,
) -> Result<Response, ServiceError> {
    let json_obj = RpcObj::new(
        id,
        "player".into(),
        ControlParams {
            control: command.to_owned(),
        },
    );

    post_request(conn, id, json_obj).await
}

pub async fn media_info(
    conn: &Pool<Sqlite>,
    id: i32,
    command: String,
) -> Result<Response, ServiceError> {
    let json_obj = RpcObj::new(id, "player".into(), MediaParams { media: command });

    post_request(conn, id, json_obj).await
}

pub async fn control_service(
    conn: &Pool<Sqlite>,
    id: i32,
    command: &ServiceCmd,
    engine: Option<web::Data<ProcessControl>>,
) -> Result<String, ServiceError> {
    if engine.is_some() && engine.as_ref().unwrap().piggyback.load(Ordering::SeqCst) {
        match command {
            ServiceCmd::Start => engine.unwrap().start().await,
            ServiceCmd::Stop => {
                if control_state(conn, id, "stop_all").await.is_ok() {
                    engine.unwrap().stop().await
                } else {
                    Err(ServiceError::NoContent("Nothing to stop".to_string()))
                }
            }
            ServiceCmd::Restart => {
                if control_state(conn, id, "stop_all").await.is_ok() {
                    engine.unwrap().restart().await
                } else {
                    Err(ServiceError::NoContent("Nothing to stop".to_string()))
                }
            }
            ServiceCmd::Status => engine.unwrap().status(),
            _ => Err(ServiceError::Conflict(
                "Engine runs in piggyback mode, in this mode this command is not allowed."
                    .to_string(),
            )),
        }
    } else {
        let system_d = SystemD::new(conn, id).await?;

        match command {
            ServiceCmd::Enable => system_d.enable(),
            ServiceCmd::Disable => system_d.disable(),
            ServiceCmd::Start => system_d.start(),
            ServiceCmd::Stop => system_d.stop(),
            ServiceCmd::Restart => system_d.restart(),
            ServiceCmd::Status => system_d.status().await,
        }
    }
}
