use std::{
    env, fmt,
    net::TcpListener,
    path::{Path, PathBuf},
};

#[cfg(target_family = "unix")]
use std::{
    io::{Error, ErrorKind},
    os::unix::fs::MetadataExt,
};

use chrono::{format::ParseErrorKind, prelude::*};
use faccess::PathExt;
use log::*;
use path_clean::PathClean;
use rand::Rng;
use regex::Regex;
use tokio::{fs, process::Command};

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
pub mod time_machine;

use crate::db::models::GlobalSettings;
use crate::player::utils::time_to_sec;
use crate::utils::{errors::ServiceError, logging::log_file_path};
use crate::ARGS;
use serde_json::json;

pub const S3_INDICATOR: &str = "s3://";

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
    let config = GlobalSettings::global();
    let dev_path = env::current_dir()
        .unwrap_or_default()
        .join("frontend/.output/public/");
    let mut public_path = PathBuf::from(&config.public);

    if let Some(p) = &ARGS.public {
        // When public path is set as argument use this path for serving static files.
        // Works only when feature embed_frontend is not set.
        let public = PathBuf::from(p);

        public_path = if public.is_absolute() {
            public.to_path_buf()
        } else {
            env::current_dir().unwrap_or_default().join(public)
        }
        .clean();
    } else if cfg!(debug_assertions) && dev_path.is_dir() {
        public_path = dev_path;
    }

    public_path
}

pub async fn read_log_file(channel_id: &i32, date: &str) -> Result<String, ServiceError> {
    let date_str = if date.is_empty() {
        "".to_string()
    } else {
        format!("_{date}")
    };

    let log_path = log_file_path()
        .join(format!("ffplayout_{channel_id}{date_str}.log"))
        .clean();
    let file_size = fs::metadata(&log_path).await?.len() as f64;

    let log_content = if file_size > 5000000.0 {
        error!("Log file to big: {}", sizeof_fmt(file_size));
        format!("The log file is larger ({}) than the hard limit of 5MB, the probability is very high that something is wrong with the playout.\nCheck this on the server with `less {log_path:?}`.", sizeof_fmt(file_size))
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
pub fn gen_tcp_socket(exclude_socket: String) -> Option<String> {
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

pub async fn copy_assets(storage_path: &Path) -> Result<(), std::io::Error> {
    if storage_path.is_dir() {
        let target = storage_path.join("00-assets");
        let mut dummy_source = Path::new("/usr/share/ffplayout/dummy.vtt");
        let mut font_source = Path::new("/usr/share/ffplayout/DejaVuSans.ttf");
        let mut logo_source = Path::new("/usr/share/ffplayout/logo.png");

        if !dummy_source.is_file() {
            dummy_source = Path::new("./assets/dummy.vtt")
        }
        if !font_source.is_file() {
            font_source = Path::new("./assets/DejaVuSans.ttf")
        }
        if !logo_source.is_file() {
            logo_source = Path::new("./assets/logo.png")
        }

        if !target.is_dir() {
            let dummy_target = target.join("dummy.vtt");
            let font_target = target.join("DejaVuSans.ttf");
            let logo_target = target.join("logo.png");

            fs::create_dir_all(&target).await?;
            fs::copy(&dummy_source, &dummy_target).await?;
            fs::copy(&font_source, &font_target).await?;
            fs::copy(&logo_source, &logo_target).await?;

            #[cfg(target_family = "unix")]
            {
                let uid = nix::unistd::Uid::current();
                let parent_owner = storage_path.metadata().unwrap().uid();

                if uid.is_root() && uid.to_string() != parent_owner.to_string() {
                    let user = nix::unistd::User::from_uid(parent_owner.into())
                        .unwrap_or_default()
                        .unwrap();

                    nix::unistd::chown(&target, Some(user.uid), Some(user.gid))?;

                    if dummy_target.is_file() {
                        nix::unistd::chown(&dummy_target, Some(user.uid), Some(user.gid))?;
                    }
                    if font_target.is_file() {
                        nix::unistd::chown(&font_target, Some(user.uid), Some(user.gid))?;
                    }
                    if logo_target.is_file() {
                        nix::unistd::chown(&logo_target, Some(user.uid), Some(user.gid))?;
                    }
                }
            }
        }
    } else {
        error!("Storage path {storage_path:?} not exists!");
    }

    Ok(())
}

/// Combined function to check if the program is running inside a container.
/// Returns `true` if running inside a container, otherwise `false`.
pub async fn is_running_in_container() -> bool {
    // Check for Docker or Podman specific files
    if Path::new("/.dockerenv").exists() || Path::new("/run/.containerenv").exists() {
        return true;
    }

    // Run `systemd-detect-virt -c` to check if we are in a container
    if let Ok(output) = Command::new("systemd-detect-virt").arg("-c").output().await {
        return output.status.success();
    }

    false
}

pub fn parse_s3_string(
    // to_do: maybe should change the snippet to get better understanding
    s3_str: &str,
) -> Result<(aws_sdk_s3::config::Credentials, String, String), std::io::Error> {
    // Define the regex pattern for /: delimiter
    // The pattern : s3://{bucket_name}/:{endpoint_url}/:{access_key}/:{secret_key}
    let pattern = format!(r"{}([^/]+)/:(.*?)/:([^/]+)/:([^/]+)", S3_INDICATOR);

    let re = Regex::new(&pattern)
        .map_err(|_| Error::new(ErrorKind::InvalidInput, "Failed to compile regex"))?;

    // Match the input string against the regex
    if let Some(captures) = re.captures(s3_str) {
        let access_key = captures[3].to_string();
        let secret_key = captures[4].to_string();
        let mut endpoint = captures[2].to_string();

        if !endpoint.starts_with("http://") && !endpoint.starts_with("https://") {
            endpoint = format!("http://{}", endpoint);
        }

        Ok((
            aws_sdk_s3::config::Credentials::new(access_key, secret_key, None, None, "None"), // Credential
            captures[1].to_string(), // bucket-name
            endpoint,                // endpoint-url
        ))
    } else {
        Err(Error::new(
            ErrorKind::InvalidInput,
            format!(
                "Input S3 string does not match the expected format: {}",
                s3_str
            ),
        ))
    }
}
