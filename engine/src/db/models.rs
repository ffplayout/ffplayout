use std::{error::Error, fmt, str::FromStr};

use chrono_tz::Tz;
use regex::Regex;
use serde::{
    de::{self, Visitor},
    Deserialize, Serialize,
};
use sqlx::{sqlite::SqliteRow, FromRow, Pool, Row, Sqlite};

use crate::db::handles;
use crate::utils::config::PlayoutConfig;

#[derive(Clone, Default, Debug, Deserialize, Serialize, sqlx::FromRow)]
pub struct GlobalSettings {
    pub id: i32,
    pub secret: Option<String>,
    pub logs: String,
    pub playlists: String,
    pub public: String,
    pub storage: String,
    pub shared: bool,
    pub smtp_server: String,
    pub smtp_user: String,
    pub smtp_password: String,
    pub smtp_starttls: bool,
    pub smtp_port: u16,
}

impl GlobalSettings {
    pub async fn new(conn: &Pool<Sqlite>) -> Self {
        let global_settings = handles::select_global(conn);

        match global_settings.await {
            Ok(g) => g,
            Err(_) => Self {
                id: 0,
                secret: None,
                logs: String::new(),
                playlists: String::new(),
                public: String::new(),
                storage: String::new(),
                shared: false,
                smtp_server: String::new(),
                smtp_user: String::new(),
                smtp_password: String::new(),
                smtp_starttls: false,
                smtp_port: 465,
            },
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Channel {
    #[serde(default = "default_id", skip_deserializing)]
    pub id: i32,
    pub name: String,
    pub preview_url: String,
    pub extra_extensions: String,
    pub active: bool,
    pub public: String,
    pub playlists: String,
    pub storage: String,
    pub last_date: Option<String>,
    pub time_shift: f64,
    #[serde(default)]
    pub timezone: Option<Tz>,
    #[serde(default)]
    pub advanced_id: Option<i32>,
}

impl FromRow<'_, SqliteRow> for Channel {
    fn from_row(row: &SqliteRow) -> sqlx::Result<Self> {
        let mut timezone = None;

        if let Some(tz) = row
            .try_get::<String, _>("timezone")
            .ok()
            .and_then(|t: String| Tz::from_str(&t).ok())
        {
            timezone = Some(tz);
        } else if let Some(tz) = iana_time_zone::get_timezone()
            .ok()
            .and_then(|t: String| Tz::from_str(&t).ok())
        {
            timezone = Some(tz);
        }

        Ok(Self {
            id: row.try_get("id").unwrap_or_default(),
            name: row.try_get("name").unwrap_or_default(),
            preview_url: row.try_get("preview_url").unwrap_or_default(),
            extra_extensions: row.try_get("extra_extensions").unwrap_or_default(),
            active: row.try_get("active").unwrap_or_default(),
            public: row.try_get("public").unwrap_or_default(),
            playlists: row.try_get("playlists").unwrap_or_default(),
            storage: row.try_get("storage").unwrap_or_default(),
            last_date: row.try_get("last_date").unwrap_or_default(),
            time_shift: row.try_get("time_shift").unwrap_or_default(),
            timezone,
            advanced_id: row.try_get("advanced_id").unwrap_or_default(),
        })
    }
}

fn default_id() -> i32 {
    1
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct User {
    #[serde(skip_deserializing)]
    pub id: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mail: Option<String>,
    pub username: String,
    #[serde(skip_serializing, default = "String::new")]
    pub password: String,
    pub role_id: Option<i32>,
    pub channel_ids: Option<Vec<i32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

impl FromRow<'_, SqliteRow> for User {
    fn from_row(row: &SqliteRow) -> sqlx::Result<Self> {
        Ok(Self {
            id: row.try_get("id").unwrap_or_default(),
            mail: row.try_get("mail").unwrap_or_default(),
            username: row.try_get("username").unwrap_or_default(),
            password: row.try_get("password").unwrap_or_default(),
            role_id: row.try_get("role_id").unwrap_or_default(),
            channel_ids: Some(
                row.try_get::<String, &str>("channel_ids")
                    .unwrap_or_default()
                    .split(',')
                    .map(|i| i.parse::<i32>().unwrap_or_default())
                    .collect(),
            ),
            token: None,
        })
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct UserMeta {
    pub id: i32,
    pub channels: Vec<i32>,
}

impl UserMeta {
    pub fn new(id: i32, channels: Vec<i32>) -> Self {
        Self { id, channels }
    }
}

#[derive(Clone, Debug, Default, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    GlobalAdmin,
    ChannelAdmin,
    User,
    #[default]
    Guest,
}

impl Role {
    pub fn set_role(role: &str) -> Self {
        role.parse().unwrap_or(Self::Guest)
    }
}

impl FromStr for Role {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "global_admin" => Ok(Self::GlobalAdmin),
            "channel_admin" => Ok(Self::ChannelAdmin),
            "user" => Ok(Self::User),
            _ => Ok(Self::Guest),
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Self::GlobalAdmin => write!(f, "global_admin"),
            Self::ChannelAdmin => write!(f, "channel_admin"),
            Self::User => write!(f, "user"),
            Self::Guest => write!(f, "guest"),
        }
    }
}

impl<'r> sqlx::decode::Decode<'r, ::sqlx::Sqlite> for Role
where
    &'r str: sqlx::decode::Decode<'r, sqlx::Sqlite>,
{
    fn decode(
        value: sqlx::sqlite::SqliteValueRef<'r>,
    ) -> Result<Self, Box<dyn Error + 'static + Send + Sync>> {
        let value = <&str as sqlx::decode::Decode<sqlx::Sqlite>>::decode(value)?;

        Ok(value.parse()?)
    }
}

impl FromRow<'_, SqliteRow> for Role {
    fn from_row(row: &SqliteRow) -> sqlx::Result<Self> {
        match row.get("name") {
            "global_admin" => Ok(Self::GlobalAdmin),
            "channel_admin" => Ok(Self::ChannelAdmin),
            "user" => Ok(Self::User),
            _ => Ok(Self::Guest),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, sqlx::FromRow)]
pub struct TextPreset {
    #[sqlx(default)]
    #[serde(skip_deserializing)]
    pub id: i32,
    pub channel_id: i32,
    pub name: String,
    pub text: String,
    pub x: String,
    pub y: String,
    #[serde(deserialize_with = "deserialize_number_or_string")]
    pub fontsize: String,
    #[serde(deserialize_with = "deserialize_number_or_string")]
    pub line_spacing: String,
    pub fontcolor: String,
    pub r#box: String,
    pub boxcolor: String,
    #[serde(deserialize_with = "deserialize_number_or_string")]
    pub boxborderw: String,
    #[serde(deserialize_with = "deserialize_number_or_string")]
    pub alpha: String,
}

/// Deserialize number or string
pub fn deserialize_number_or_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct StringOrNumberVisitor;

    impl Visitor<'_> for StringOrNumberVisitor {
        type Value = String;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string or a number")
        }

        fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
            let re = Regex::new(r"0,([0-9]+)").unwrap();
            let clean_string = re.replace_all(value, "0.$1").to_string();
            Ok(clean_string)
        }

        fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
            Ok(value.to_string())
        }

        fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
            Ok(value.to_string())
        }

        fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
            Ok(value.to_string())
        }
    }

    deserializer.deserialize_any(StringOrNumberVisitor)
}

#[derive(Clone, Debug, Deserialize, Serialize, sqlx::FromRow)]
pub struct Configuration {
    pub id: i32,
    pub channel_id: i32,
    pub general_stop_threshold: f64,

    pub mail_subject: String,
    pub mail_recipient: String,
    pub mail_level: String,
    pub mail_interval: i64,

    pub logging_ffmpeg_level: String,
    pub logging_ingest_level: String,
    pub logging_detect_silence: bool,
    #[serde(default)]
    pub logging_ignore: String,

    pub processing_mode: String,
    pub processing_audio_only: bool,
    pub processing_copy_audio: bool,
    pub processing_copy_video: bool,
    pub processing_width: i64,
    pub processing_height: i64,
    pub processing_aspect: f64,
    pub processing_fps: f64,
    pub processing_add_logo: bool,
    pub processing_logo: String,
    pub processing_logo_scale: String,
    pub processing_logo_opacity: f64,
    pub processing_logo_position: String,
    #[serde(default = "default_tracks")]
    pub processing_audio_tracks: i32,
    #[serde(default = "default_track_index")]
    pub processing_audio_track_index: i32,
    #[serde(default = "default_channels")]
    pub processing_audio_channels: u8,
    pub processing_volume: f64,
    #[serde(default)]
    pub processing_filter: String,
    #[serde(default)]
    pub processing_override_filter: bool,
    #[serde(default)]
    pub processing_vtt_enable: bool,
    #[serde(default)]
    pub processing_vtt_dummy: Option<String>,

    pub ingest_enable: bool,
    pub ingest_param: String,
    #[serde(default)]
    pub ingest_filter: String,

    pub playlist_day_start: String,
    pub playlist_length: String,
    pub playlist_infinit: bool,

    pub storage_filler: String,
    pub storage_extensions: String,
    pub storage_shuffle: bool,

    pub text_add: bool,
    pub text_from_filename: bool,
    pub text_font: String,
    pub text_style: String,
    pub text_regex: String,

    pub task_enable: bool,
    pub task_path: String,

    pub output_mode: String,
    pub output_param: String,
}

impl Configuration {
    pub fn from(id: i32, channel_id: i32, config: PlayoutConfig) -> Self {
        Self {
            id,
            channel_id,
            general_stop_threshold: config.general.stop_threshold,
            mail_subject: config.mail.subject,
            mail_recipient: config.mail.recipient,
            mail_level: config.mail.mail_level.to_string(),
            mail_interval: config.mail.interval,
            logging_ffmpeg_level: config.logging.ffmpeg_level,
            logging_ingest_level: config.logging.ingest_level,
            logging_detect_silence: config.logging.detect_silence,
            logging_ignore: config.logging.ignore_lines.join(";"),
            processing_mode: config.processing.mode.to_string(),
            processing_audio_only: config.processing.audio_only,
            processing_audio_track_index: config.processing.audio_track_index,
            processing_copy_audio: config.processing.copy_audio,
            processing_copy_video: config.processing.copy_video,
            processing_width: config.processing.width,
            processing_height: config.processing.height,
            processing_aspect: config.processing.aspect,
            processing_fps: config.processing.fps,
            processing_add_logo: config.processing.add_logo,
            processing_logo: config.processing.logo,
            processing_logo_scale: config.processing.logo_scale,
            processing_logo_opacity: config.processing.logo_opacity,
            processing_logo_position: config.processing.logo_position,
            processing_audio_tracks: config.processing.audio_tracks,
            processing_audio_channels: config.processing.audio_channels,
            processing_volume: config.processing.volume,
            processing_filter: config.processing.custom_filter,
            processing_override_filter: config.processing.override_filter,
            processing_vtt_enable: config.processing.vtt_enable,
            processing_vtt_dummy: config.processing.vtt_dummy,
            ingest_enable: config.ingest.enable,
            ingest_param: config.ingest.input_param,
            ingest_filter: config.ingest.custom_filter,
            playlist_day_start: config.playlist.day_start,
            playlist_length: config.playlist.length,
            playlist_infinit: config.playlist.infinit,
            storage_filler: config.storage.filler,
            storage_extensions: config.storage.extensions.join(";"),
            storage_shuffle: config.storage.shuffle,
            text_add: config.text.add_text,
            text_font: config.text.font,
            text_from_filename: config.text.text_from_filename,
            text_style: config.text.style,
            text_regex: config.text.regex,
            task_enable: config.task.enable,
            task_path: config.task.path.to_string_lossy().to_string(),
            output_mode: config.output.mode.to_string(),
            output_param: config.output.output_param,
        }
    }
}

fn default_track_index() -> i32 {
    -1
}

fn default_tracks() -> i32 {
    1
}

fn default_channels() -> u8 {
    2
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, sqlx::FromRow)]
pub struct AdvancedConfiguration {
    pub id: i32,
    pub channel_id: i32,
    pub decoder_input_param: Option<String>,
    pub decoder_output_param: Option<String>,
    pub encoder_input_param: Option<String>,
    pub ingest_input_param: Option<String>,
    pub filter_deinterlace: Option<String>,
    pub filter_pad_video: Option<String>,
    pub filter_fps: Option<String>,
    pub filter_scale: Option<String>,
    pub filter_set_dar: Option<String>,
    pub filter_fade_in: Option<String>,
    pub filter_fade_out: Option<String>,
    pub filter_logo: Option<String>,
    pub filter_overlay_logo_scale: Option<String>,
    pub filter_overlay_logo_fade_in: Option<String>,
    pub filter_overlay_logo_fade_out: Option<String>,
    pub filter_overlay_logo: Option<String>,
    pub filter_tpad: Option<String>,
    pub filter_drawtext_from_file: Option<String>,
    pub filter_drawtext_from_zmq: Option<String>,
    pub filter_aevalsrc: Option<String>,
    pub filter_afade_in: Option<String>,
    pub filter_afade_out: Option<String>,
    pub filter_apad: Option<String>,
    pub filter_volume: Option<String>,
    pub filter_split: Option<String>,
    pub name: Option<String>,
}
