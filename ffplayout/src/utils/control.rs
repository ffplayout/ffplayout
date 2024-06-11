use std::{
    error::Error,
    fmt,
    str::FromStr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};

use log::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sqlx::{Pool, Sqlite};
use tokio::process::Child;
use zeromq::{Socket, SocketRecv, SocketSend, ZmqMessage};

use crate::db::handles;
use crate::player::{
    controller::{ChannelManager, ProcessUnit::*},
    utils::{get_delta, get_media_map},
};
use crate::utils::{config::OutputMode::*, errors::ServiceError, TextFilter};

#[derive(Debug, Deserialize, Serialize, Clone)]
struct TextParams {
    control: String,
    message: TextFilter,
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

async fn zmq_send(msg: &str, socket_addr: &str) -> Result<String, Box<dyn Error>> {
    let mut socket = zeromq::ReqSocket::new();
    socket.connect(&format!("tcp://{socket_addr}")).await?;
    socket.send(msg.into()).await?;
    let repl: ZmqMessage = socket.recv().await?;
    let response = String::from_utf8(repl.into_vec()[0].to_vec())?;

    Ok(response)
}

pub async fn send_message(
    manager: ChannelManager,
    message: TextFilter,
) -> Result<Map<String, Value>, ServiceError> {
    let filter = message.to_string();
    let mut data_map = Map::new();
    let config = manager.config.lock().unwrap().clone();

    if config.text.zmq_stream_socket.is_some() {
        if let Some(clips_filter) = manager.filter_chain.clone() {
            *clips_filter.lock().unwrap() = vec![filter.clone()];
        }

        if config.output.mode == HLS {
            if manager.ingest_is_running.load(Ordering::SeqCst) {
                let filter_server = format!("drawtext@dyntext reinit {filter}");

                if let Ok(reply) = zmq_send(
                    &filter_server,
                    &config.text.zmq_server_socket.clone().unwrap(),
                )
                .await
                {
                    data_map.insert("message".to_string(), json!(reply));
                    return Ok(data_map);
                };
            } else if let Err(e) = manager.stop(Ingest) {
                error!("Ingest {e:?}")
            }
        }

        if config.output.mode != HLS || !manager.ingest_is_running.load(Ordering::SeqCst) {
            let filter_stream = format!("drawtext@dyntext reinit {filter}");

            if let Ok(reply) = zmq_send(
                &filter_stream,
                &config.text.zmq_stream_socket.clone().unwrap(),
            )
            .await
            {
                data_map.insert("message".to_string(), json!(reply));
                return Ok(data_map);
            };
        }
    }

    Err(ServiceError::ServiceUnavailable(
        "text message missing!".to_string(),
    ))
}

pub async fn control_state(
    conn: &Pool<Sqlite>,
    manager: ChannelManager,
    command: &str,
) -> Result<Map<String, Value>, ServiceError> {
    let config = manager.config.lock().unwrap().clone();
    let current_date = manager.current_date.lock().unwrap().clone();
    let current_list = manager.current_list.lock().unwrap().clone();
    let mut date = manager.current_date.lock().unwrap().clone();
    let index = manager.current_index.load(Ordering::SeqCst);

    match command {
        "back" => {
            if index > 1 && current_list.len() > 1 {
                if let Some(proc) = manager.decoder.lock().unwrap().as_mut() {
                    if let Err(e) = proc.kill() {
                        error!("Decoder {e:?}")
                    };

                    if let Err(e) = proc.wait() {
                        error!("Decoder {e:?}")
                    };
                } else {
                    return Err(ServiceError::InternalServerError);
                }

                info!("Move to last clip");
                let mut data_map = Map::new();
                let mut media = current_list[index - 2].clone();
                manager.current_index.fetch_sub(2, Ordering::SeqCst);

                if let Err(e) = media.add_probe(false) {
                    error!("{e:?}");
                };

                let (delta, _) = get_delta(&config, &media.begin.unwrap_or(0.0));
                manager.channel.lock().unwrap().time_shift = delta;
                date.clone_from(&current_date);
                handles::update_stat(conn, config.general.channel_id, current_date, delta).await?;

                data_map.insert("operation".to_string(), json!("move_to_last"));
                data_map.insert("shifted_seconds".to_string(), json!(delta));
                data_map.insert("media".to_string(), get_media_map(media));

                return Ok(data_map);
            }
        }

        "next" => {
            if index < current_list.len() {
                if let Some(proc) = manager.decoder.lock().unwrap().as_mut() {
                    if let Err(e) = proc.kill() {
                        error!("Decoder {e:?}")
                    };

                    if let Err(e) = proc.wait() {
                        error!("Decoder {e:?}")
                    };
                } else {
                    return Err(ServiceError::InternalServerError);
                }

                info!("Move to next clip");

                let mut data_map = Map::new();
                let mut media = current_list[index].clone();

                if let Err(e) = media.add_probe(false) {
                    error!("{e:?}");
                };

                let (delta, _) = get_delta(&config, &media.begin.unwrap_or(0.0));
                manager.channel.lock().unwrap().time_shift = delta;
                date.clone_from(&current_date);
                handles::update_stat(conn, config.general.channel_id, current_date, delta).await?;

                data_map.insert("operation".to_string(), json!("move_to_next"));
                data_map.insert("shifted_seconds".to_string(), json!(delta));
                data_map.insert("media".to_string(), get_media_map(media));

                return Ok(data_map);
            }
        }

        "reset" => {
            if let Some(proc) = manager.decoder.lock().unwrap().as_mut() {
                if let Err(e) = proc.kill() {
                    error!("Decoder {e:?}")
                };

                if let Err(e) = proc.wait() {
                    error!("Decoder {e:?}")
                };
            } else {
                return Err(ServiceError::InternalServerError);
            }

            info!("Reset playout to original state");
            let mut data_map = Map::new();
            manager.channel.lock().unwrap().time_shift = 0.0;
            date.clone_from(&current_date);
            manager.list_init.store(true, Ordering::SeqCst);

            handles::update_stat(conn, config.general.channel_id, current_date, 0.0).await?;

            data_map.insert("operation".to_string(), json!("reset_playout_state"));

            return Ok(data_map);
        }

        "stop_all" => {
            manager.stop_all();

            let mut data_map = Map::new();
            data_map.insert("message".to_string(), json!("Stop playout!"));

            return Ok(data_map);
        }

        _ => {
            return Err(ServiceError::ServiceUnavailable(
                "Command not found!".to_string(),
            ))
        }
    }

    Ok(Map::new())
}
