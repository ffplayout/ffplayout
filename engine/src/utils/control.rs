use std::{error::Error, fmt, str::FromStr, sync::atomic::Ordering};

use log::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sqlx::{Pool, Sqlite};
use zeromq::{Socket, SocketRecv, SocketSend, ZmqMessage};

use crate::db::handles;
use crate::player::{
    controller::{ChannelManager, ProcessUnit::*},
    utils::{get_delta, get_media_map},
};
use crate::utils::{config::OutputMode::*, errors::ServiceError, logging::Target, TextFilter};

#[derive(Debug, Deserialize, Serialize, Clone)]
struct TextParams {
    control: String,
    message: TextFilter,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ControlParams {
    pub control: PlayerCtl,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct MediaParams {
    media: String,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ProcessCtl {
    #[default]
    Status,
    Start,
    Stop,
    Restart,
}

impl FromStr for ProcessCtl {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input.to_lowercase().as_str() {
            "status" => Ok(Self::Status),
            "start" => Ok(Self::Start),
            "stop" => Ok(Self::Stop),
            "restart" => Ok(Self::Restart),
            _ => Err(format!("Command '{input}' not found!")),
        }
    }
}

impl fmt::Display for ProcessCtl {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Self::Status => write!(f, "status"),
            Self::Start => write!(f, "start"),
            Self::Stop => write!(f, "stop"),
            Self::Restart => write!(f, "restart"),
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize, Clone, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PlayerCtl {
    Back,
    Next,
    #[default]
    Reset,
}

impl FromStr for PlayerCtl {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input.to_lowercase().as_str() {
            "back" => Ok(Self::Back),
            "next" => Ok(Self::Next),
            "reset" => Ok(Self::Reset),
            _ => Err(format!("Command '{input}' not found!")),
        }
    }
}

impl fmt::Display for PlayerCtl {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Self::Back => write!(f, "back"),
            Self::Next => write!(f, "next"),
            Self::Reset => write!(f, "reset"),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Process {
    pub command: ProcessCtl,
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
    let config = manager.config.lock().await.clone();

    if config.text.zmq_stream_socket.is_some() {
        if let Some(clips_filter) = manager.filter_chain.clone() {
            *clips_filter.lock().await = vec![filter.clone()];
        }

        if config.output.mode == HLS {
            if manager.ingest_is_alive.load(Ordering::SeqCst) {
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
            } else {
                manager.stop(Ingest).await;
            }
        }

        if config.output.mode != HLS || !manager.ingest_is_alive.load(Ordering::SeqCst) {
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
    manager: &ChannelManager,
    command: &PlayerCtl,
) -> Result<Map<String, Value>, ServiceError> {
    let config = manager.config.lock().await.clone();
    let id = config.general.channel_id;
    let current_date = manager.current_date.lock().await.clone();
    let current_list = manager.current_list.lock().await.clone();
    let index = manager.current_index.load(Ordering::SeqCst);
    let mut data_map = Map::new();
    let mut shift = 0.0;

    match command {
        PlayerCtl::Back => {
            if index > 1 && current_list.len() > 1 {
                let mut media = current_list[index - 2].clone();
                (shift, _) = get_delta(&config, &media.begin.unwrap_or(0.0));

                info!(target: Target::file_mail(), channel = id; "Move to last clip");

                manager.current_index.fetch_sub(2, Ordering::SeqCst);

                if let Err(e) = media.add_probe(false).await {
                    error!(target: Target::file_mail(), channel = id; "{e:?}");
                };

                data_map.insert("operation".to_string(), json!("move_to_last"));
                data_map.insert("shifted_seconds".to_string(), json!(shift));
                data_map.insert("media".to_string(), get_media_map(media));
            }
        }

        PlayerCtl::Next => {
            if index < current_list.len() {
                let mut media = current_list[index].clone();
                (shift, _) = get_delta(&config, &media.begin.unwrap_or(0.0));

                info!(target: Target::file_mail(), channel = id; "Move to next clip");

                if let Err(e) = media.add_probe(false).await {
                    error!(target: Target::file_mail(), channel = id; "{e:?}");
                };

                data_map.insert("operation".to_string(), json!("move_to_next"));
                data_map.insert("shifted_seconds".to_string(), json!(shift));
                data_map.insert("media".to_string(), get_media_map(media));
            }
        }

        PlayerCtl::Reset => {
            info!(target: Target::file_mail(), channel = id; "Reset playout to original state");

            manager.list_init.store(true, Ordering::SeqCst);

            data_map.insert("operation".to_string(), json!("reset_playout_state"));
        }
    }

    manager.channel.lock().await.time_shift = shift;
    handles::update_stat(conn, id, &Some(current_date), shift).await?;
    manager.stop(Decoder).await;

    Ok(data_map)
}
