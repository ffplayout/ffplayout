use std::{
    env,
    path::{Path, PathBuf},
};

use jiff::{
    civil::{Date, DateTime},
    fmt::temporal::DateTimeParser,
    tz::TimeZone,
};
use log::*;
use path_clean::PathClean;
use tokio::fs;

use serde::{Deserialize, Deserializer, de};

pub mod args_parse;
pub mod channels;
pub mod config;
pub mod config_dump;
pub mod control;
pub mod errors;
pub mod generator;
pub mod logging;
pub mod mail;
pub(crate) mod moq_codecs;
pub mod notification;
pub mod paths;
pub mod playlist;
pub mod program;
pub mod setup;
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

/// Build the log file name of a channel, for an empty or `YYYY-MM-DD` date.
///
/// The date comes from the request, so anything else is rejected. This keeps
/// the name free from path separators and from other channels' log names.
fn log_file_name(channel_id: i32, date: &str) -> Result<String, ServiceError> {
    if date.is_empty() {
        return Ok(format!("ffplayout_{channel_id}.log"));
    }

    let invalid_date = || ServiceError::BadRequest("Invalid log date; expected YYYY-MM-DD".into());
    let parsed = Date::strptime("%Y-%m-%d", date).map_err(|_| invalid_date())?;
    let normalized = parsed.strftime("%Y-%m-%d").to_string();

    if date != normalized {
        return Err(invalid_date());
    }

    Ok(format!("ffplayout_{channel_id}_{normalized}.log"))
}

pub async fn read_log_file(
    channel_id: &i32,
    date: &str,
    timezone: TimeZone,
    download: bool,
) -> Result<String, ServiceError> {
    let log_name = log_file_name(*channel_id, date)?;
    let log_path = log_file_path().join(log_name).clean();
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

pub fn naive_date_time_from_str<'de, D>(deserializer: D) -> Result<DateTime, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;

    parse_naive_date_time(&s).map_err(de::Error::custom)
}

pub fn optional_naive_date_time_from_str<'de, D>(
    deserializer: D,
) -> Result<Option<DateTime>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;

    parse_naive_date_time(&s)
        .map(Some)
        .map_err(de::Error::custom)
}

fn parse_naive_date_time(s: &str) -> Result<DateTime, jiff::Error> {
    DateTime::strptime("%Y-%m-%dT%H:%M:%S", s)
        .or_else(|_| Date::strptime("%Y-%m-%d", s).map(|date| date.at(0, 0, 0, 0)))
        .or_else(|_| DateTimeParser::new().parse_datetime(s))
        .or_else(|_| DateTime::strptime("%Y-%m-%dT%H:%M:%SZ", s))
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
    Path::new("/.dockerenv").exists() || Path::new("/run/.containerenv").exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_file_name_accepts_only_empty_or_normalized_dates() {
        assert_eq!(log_file_name(1, "").unwrap(), "ffplayout_1.log");
        assert_eq!(
            log_file_name(2, "2024-02-29").unwrap(),
            "ffplayout_2_2024-02-29.log"
        );

        for date in [
            "x/../../../var/log/auth",
            "x/../ffplayout_2",
            "/etc/passwd",
            "2024-2-9",
            "2023-02-29",
            "2024-02-29/..",
            " 2024-02-29",
        ] {
            assert!(log_file_name(1, date).is_err(), "{date:?} must be rejected");
        }
    }
}
