use std::{
    env,
    path::{Path, PathBuf},
};

use chrono::{format::ParseErrorKind, prelude::*};
use chrono_tz::Tz;
use log::*;
use path_clean::PathClean;
use rand::RngExt;
use tokio::{fs, net::TcpListener};

use serde::{Deserialize, Deserializer, de};

pub mod args_parse;
pub mod channels;
pub mod config;
pub mod control;
pub mod errors;
pub mod generator;
pub mod logging;
pub mod mail;
pub mod playlist;
pub mod system;
pub mod task_runner;
pub mod text;
pub mod time_machine;

use crate::{
    ARGS,
    db::GLOBAL_SETTINGS,
    player::utils::time_to_sec,
    utils::{
        errors::ServiceError,
        logging::{log_file_path, remove_html, timestamps_to_timezone},
    },
};

pub fn public_path() -> PathBuf {
    let config = GLOBAL_SETTINGS.get().unwrap();
    let dev_path = env::current_dir()
        .unwrap_or_default()
        .join("frontend/.output/public/");
    let mut public_path = PathBuf::from(&config.public);

    if let Some(p) = &ARGS.public {
        // When public path is set as argument use this path for serving static files.
        // Works only when feature embed_frontend is not set.
        let public = PathBuf::from(p);

        public_path = if public.is_absolute() {
            public
        } else {
            env::current_dir().unwrap_or_default().join(public)
        }
        .clean();
    } else if cfg!(debug_assertions) && dev_path.is_dir() {
        public_path = dev_path;
    }

    public_path
}

pub async fn read_log_file(
    channel_id: &i32,
    date: &str,
    timezone: Tz,
    download: bool,
) -> Result<String, ServiceError> {
    let date_str = if date.is_empty() {
        String::new()
    } else {
        format!("_{date}")
    };

    let log_path = log_file_path()
        .join(format!("ffplayout_{channel_id}{date_str}.log"))
        .clean();
    let file_size = fs::metadata(&log_path).await?.len() as f64;

    let log_content = if download || file_size < 5000000.0 {
        let content = fs::read_to_string(log_path).await?;
        let content = timestamps_to_timezone(&content, timezone);

        if download {
            remove_html(&content)
        } else {
            content
        }
    } else {
        error!("Log file to big: {}", sizeof_fmt(file_size));
        format!(
            "The log file is larger ({}) than the hard limit of 5MB, the probability is very high that something is wrong with the playout.\nCheck this on the server with `less {log_path:?}`.",
            sizeof_fmt(file_size)
        )
    };

    Ok(log_content)
}

/// get human readable file size
pub fn sizeof_fmt(mut num: f64) -> String {
    let suffix = 'B';

    for unit in ["", "Ki", "Mi", "Gi", "Ti", "Pi", "Ei", "Zi"] {
        if num.abs() < 1024.0 {
            return format!("{num:.1}{unit}{suffix}");
        }
        num /= 1024.0;
    }

    format!("{num:.1}Yi{suffix}")
}

pub fn local_utc_offset() -> i32 {
    let mut offset = Local::now().format("%:z").to_string();
    let operator = offset.remove(0);
    let mut utc_offset = 0;

    if let Some((r, f)) = offset.split_once(':') {
        utc_offset = r.parse::<i32>().unwrap_or(0) * 60 + f.parse::<i32>().unwrap_or(0);

        if operator == '-' && utc_offset > 0 {
            utc_offset = -utc_offset;
        }
    }

    utc_offset
}

pub fn naive_date_time_from_str<'de, D>(deserializer: D) -> Result<NaiveDateTime, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;

    match NaiveDateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M:%S") {
        Ok(date_time) => Ok(date_time),
        Err(e) => {
            if e.kind() == ParseErrorKind::TooShort {
                NaiveDateTime::parse_from_str(&format!("{s}T00:00:00"), "%Y-%m-%dT%H:%M:%S")
                    .map_err(de::Error::custom)
            } else {
                NaiveDateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M:%S%#z").map_err(de::Error::custom)
            }
        }
    }
}

/// get a free tcp socket
pub async fn gen_tcp_socket(exclude_socket: &str) -> Option<String> {
    for _ in 0..100 {
        let port = rand::rng().random_range(45321..54268);
        let socket = format!("127.0.0.1:{port}");

        if socket != exclude_socket && TcpListener::bind(("127.0.0.1", port)).await.is_ok() {
            return Some(socket);
        }
    }

    None
}

pub fn round_to_nearest_ten(num: i64) -> i64 {
    if num % 10 >= 5 {
        ((num / 10) + 1) * 10
    } else {
        (num / 10) * 10
    }
}

/// Combined function to check if the program is running inside a container.
/// Returns `true` if running inside a container, otherwise `false`.
pub fn is_running_in_container() -> bool {
    // Check for Docker or Podman specific files
    if Path::new("/.dockerenv").exists() || Path::new("/run/.containerenv").exists() {
        return true;
    }

    false
}
