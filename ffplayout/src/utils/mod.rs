use std::{
    env, fmt,
    net::TcpListener,
    path::{Path, PathBuf},
};

use chrono::{format::ParseErrorKind, prelude::*};
use faccess::PathExt;
use log::*;
use path_clean::PathClean;
use rand::Rng;
use regex::Regex;
use tokio::fs;

use serde::{
    de::{self, Visitor},
    Deserialize, Deserializer, Serialize,
};

pub mod advanced_config;
pub mod args_parse;
pub mod channels;
pub mod config;
pub mod control;
pub mod errors;
pub mod files;
pub mod generator;
pub mod logging;
pub mod playlist;
pub mod system;
pub mod task_runner;

use crate::player::utils::time_to_sec;
use crate::utils::{errors::ServiceError, logging::log_file_path};
use crate::ARGS;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct TextFilter {
    pub text: Option<String>,
    #[serde(default, deserialize_with = "deserialize_number_or_string")]
    pub x: Option<String>,
    #[serde(default, deserialize_with = "deserialize_number_or_string")]
    pub y: Option<String>,
    #[serde(default, deserialize_with = "deserialize_number_or_string")]
    pub fontsize: Option<String>,
    #[serde(default, deserialize_with = "deserialize_number_or_string")]
    pub line_spacing: Option<String>,
    pub fontcolor: Option<String>,
    #[serde(default, deserialize_with = "deserialize_number_or_string")]
    pub alpha: Option<String>,
    #[serde(default, deserialize_with = "deserialize_number_or_string")]
    pub r#box: Option<String>,
    pub boxcolor: Option<String>,
    #[serde(default, deserialize_with = "deserialize_number_or_string")]
    pub boxborderw: Option<String>,
}

/// Deserialize number or string
pub fn deserialize_number_or_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct StringOrNumberVisitor;

    impl<'de> Visitor<'de> for StringOrNumberVisitor {
        type Value = Option<String>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string or a number")
        }

        fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
            let re = Regex::new(r"0,([0-9]+)").unwrap();
            let clean_string = re.replace_all(value, "0.$1").to_string();
            Ok(Some(clean_string))
        }

        fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
            Ok(Some(value.to_string()))
        }

        fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
            Ok(Some(value.to_string()))
        }

        fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
            Ok(Some(value.to_string()))
        }
    }

    deserializer.deserialize_any(StringOrNumberVisitor)
}

impl fmt::Display for TextFilter {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let escaped_text = self
            .text
            .clone()
            .unwrap_or_default()
            .replace('\'', "'\\\\\\''")
            .replace('\\', "\\\\\\\\")
            .replace('%', "\\\\\\%")
            .replace(':', "\\:");

        let mut s = format!("text='{escaped_text}'");

        if let Some(v) = &self.x {
            if !v.is_empty() {
                s.push_str(&format!(":x='{v}'"));
            }
        }
        if let Some(v) = &self.y {
            if !v.is_empty() {
                s.push_str(&format!(":y='{v}'"));
            }
        }
        if let Some(v) = &self.fontsize {
            if !v.is_empty() {
                s.push_str(&format!(":fontsize={v}"));
            }
        }
        if let Some(v) = &self.line_spacing {
            if !v.is_empty() {
                s.push_str(&format!(":line_spacing={v}"));
            }
        }
        if let Some(v) = &self.fontcolor {
            if !v.is_empty() {
                s.push_str(&format!(":fontcolor={v}"));
            }
        }
        if let Some(v) = &self.alpha {
            if !v.is_empty() {
                s.push_str(&format!(":alpha='{v}'"));
            }
        }
        if let Some(v) = &self.r#box {
            if !v.is_empty() {
                s.push_str(&format!(":box={v}"));
            }
        }
        if let Some(v) = &self.boxcolor {
            if !v.is_empty() {
                s.push_str(&format!(":boxcolor={v}"));
            }
        }
        if let Some(v) = &self.boxborderw {
            if !v.is_empty() {
                s.push_str(&format!(":boxborderw={v}"));
            }
        }

        write!(f, "{s}")
    }
}

pub fn db_path() -> Result<&'static str, Box<dyn std::error::Error>> {
    if let Some(path) = ARGS.db.clone() {
        let mut absolute_path = if path.is_absolute() {
            path
        } else {
            env::current_dir()?.join(path)
        }
        .clean();

        if absolute_path.is_dir() {
            absolute_path = absolute_path.join("ffplayout.db");
        }

        if let Some(abs_path) = absolute_path.parent() {
            if abs_path.writable() {
                return Ok(Box::leak(
                    absolute_path.to_string_lossy().to_string().into_boxed_str(),
                ));
            }

            error!("Given database path is not writable!");
        }
    }

    let sys_path = Path::new("/usr/share/ffplayout/db");
    let mut db_path = "./ffplayout.db";

    if sys_path.is_dir() && !sys_path.writable() {
        error!("Path {} is not writable!", sys_path.display());
    }

    if sys_path.is_dir() && sys_path.writable() {
        db_path = "/usr/share/ffplayout/db/ffplayout.db";
    } else if Path::new("./assets").is_dir() {
        db_path = "./assets/ffplayout.db";
    }

    Ok(db_path)
}

pub fn public_path() -> PathBuf {
    let path = PathBuf::from("./ffplayout-frontend/.output/public/");

    if cfg!(debug_assertions) && path.is_dir() {
        return path;
    }

    let path = PathBuf::from("/usr/share/ffplayout/public/");

    if path.is_dir() {
        return path;
    }

    PathBuf::from("./public/")
}

pub async fn read_log_file(channel_id: &i32, date: &str) -> Result<String, ServiceError> {
    let date_str = if date.is_empty() {
        "".to_string()
    } else {
        format!("_{date}")
    };

    let log_path = log_file_path().join(format!("ffplayout_{channel_id}{date_str}.log"));
    let file_size = fs::metadata(&log_path).await?.len() as f64;

    let log_content = if file_size > 5000000.0 {
        error!("Log file to big: {}", sizeof_fmt(file_size));
        format!("The log file is larger ({}) than the hard limit of 5MB, the probability is very high that something is wrong with the playout. Check this on the server with `less {log_path:?}`.", sizeof_fmt(file_size))
    } else {
        fs::read_to_string(log_path).await?
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
pub fn free_tcp_socket(exclude_socket: String) -> Option<String> {
    for _ in 0..100 {
        let port = rand::thread_rng().gen_range(45321..54268);
        let socket = format!("127.0.0.1:{port}");

        if socket != exclude_socket && TcpListener::bind(("127.0.0.1", port)).is_ok() {
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
