use std::{fmt, str::FromStr, sync::atomic::Ordering};

use log::*;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sqlx::{Pool, Sqlite};

use crate::{
    db::{handles, models::TextPreset},
    player::{
        controller::ChannelManager,
        utils::{get_delta, get_media_map},
    },
    utils::{errors::ServiceError, text::text_config},
};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ControlParams {
    pub control: PlayerCtl,
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

pub async fn send_message(
    manager: ChannelManager,
    message: TextPreset,
) -> Result<Map<String, Value>, ServiceError> {
    let mut data_map = Map::new();

    let text = (!message.text.trim().is_empty()).then(|| message.text.clone());
    if text.is_none() && !message.use_filename {
        manager.text_overlay.clear();
        data_map.insert("message".to_string(), json!("text overlay cleared"));
        return Ok(data_map);
    }

    manager
        .text_overlay
        .set(Some(text_config(&message, text, message.use_filename)));
    data_map.insert("message".to_string(), json!("text overlay updated"));
    Ok(data_map)
}

pub async fn control_state(
    conn: &Pool<Sqlite>,
    manager: &ChannelManager,
    command: &PlayerCtl,
) -> Result<Map<String, Value>, ServiceError> {
    let config = manager.config.read().await.clone();
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

                info!(channel = id; "Move to last clip");

                manager.current_index.fetch_sub(2, Ordering::SeqCst);

                if let Err(e) = media.add_probe(false).await {
                    error!(channel = id; "{e:?}");
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

                info!(channel = id; "Move to next clip");

                if let Err(e) = media.add_probe(false).await {
                    error!(channel = id; "{e:?}");
                };

                data_map.insert("operation".to_string(), json!("move_to_next"));
                data_map.insert("shifted_seconds".to_string(), json!(shift));
                data_map.insert("media".to_string(), get_media_map(media));
            }
        }

        PlayerCtl::Reset => {
            info!(channel = id; "Reset playout to original state");

            manager.list_init.store(true, Ordering::SeqCst);

            data_map.insert("operation".to_string(), json!("reset_playout_state"));
        }
    }

    manager.channel.lock().await.time_shift = shift;
    handles::update_stat(conn, id, &Some(current_date), shift).await?;
    manager.playback_control.lock().await.skip_current();

    Ok(data_map)
}
