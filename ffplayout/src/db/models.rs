use std::{error::Error, fmt, str::FromStr};

use once_cell::sync::OnceCell;
use regex::Regex;
use serde::{
    de::{self, Visitor},
    Deserialize, Serialize,
};
use sqlx::{sqlite::SqliteRow, FromRow, Pool, Row, Sqlite};

use crate::db::handles;
use crate::utils::config::PlayoutConfig;

#[derive(Debug, Deserialize, Serialize, sqlx::FromRow)]
pub struct GlobalSettings {
    pub secret: Option<String>,
    pub hls_path: String,
    pub playlist_path: String,
    pub storage_path: String,
    pub logging_path: String,
}

impl GlobalSettings {
    pub async fn new(conn: &Pool<Sqlite>) -> Self {
        let global_settings = handles::select_global(conn);

        match global_settings.await {
            Ok(g) => g,
            Err(_) => GlobalSettings {
                secret: None,
                hls_path: String::new(),
                playlist_path: String::new(),
                storage_path: String::new(),
                logging_path: String::new(),
            },
        }
    }

    pub fn global() -> &'static GlobalSettings {
        INSTANCE.get().expect("Config is not initialized")
    }
}

static INSTANCE: OnceCell<GlobalSettings> = OnceCell::new();

pub async fn init_globales(conn: &Pool<Sqlite>) {
    let config = GlobalSettings::new(conn).await;
    INSTANCE.set(config).unwrap();
}

#[derive(Debug, Deserialize, Serialize, sqlx::FromRow)]
pub struct User {
    #[sqlx(default)]
    #[serde(skip_deserializing)]
    pub id: i32,
    #[sqlx(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mail: Option<String>,
    pub username: String,
    #[sqlx(default)]
    #[serde(skip_serializing, default = "empty_string")]
    pub password: String,
    #[sqlx(default)]
    #[serde(skip_serializing)]
    pub role_id: Option<i32>,
    #[sqlx(default)]
    #[serde(skip_serializing)]
    pub channel_id: Option<i32>,
    #[sqlx(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

fn empty_string() -> String {
    "".to_string()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LoginUser {
    pub id: i32,
    pub username: String,
}

impl LoginUser {
    pub fn new(id: i32, username: String) -> Self {
        Self { id, username }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub enum Role {
    GlobalAdmin,
    ChannelAdmin,
    User,
    Guest,
}

impl Role {
    pub fn set_role(role: &str) -> Self {
        match role {
            "global_admin" => Role::GlobalAdmin,
            "channel_admin" => Role::ChannelAdmin,
            "user" => Role::User,
            _ => Role::Guest,
        }
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
        value: <sqlx::Sqlite as sqlx::database::HasValueRef<'r>>::ValueRef,
    ) -> Result<Role, Box<dyn Error + 'static + Send + Sync>> {
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

    impl<'de> Visitor<'de> for StringOrNumberVisitor {
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

#[derive(Clone, Debug, Default, Deserialize, Serialize, sqlx::FromRow)]
pub struct Channel {
    #[serde(skip_deserializing)]
    pub id: i32,
    pub name: String,
    pub preview_url: String,
    pub extra_extensions: String,
    pub active: bool,
    pub last_date: Option<String>,
    pub time_shift: f64,

    #[sqlx(default)]
    #[serde(default)]
    pub utc_offset: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize, sqlx::FromRow)]
pub struct Configuration {
    pub id: i32,
    pub channel_id: i32,
    pub general_help: String,
    pub stop_threshold: f64,

    pub mail_help: String,
    pub subject: String,
    pub smtp_server: String,
    pub starttls: bool,
    pub sender_addr: String,
    pub sender_pass: String,
    pub recipient: String,
    pub mail_level: String,
    pub interval: i64,

    pub logging_help: String,
    pub ffmpeg_level: String,
    pub ingest_level: String,
    #[serde(default)]
    pub detect_silence: bool,
    #[serde(default)]
    pub ignore_lines: String,

    pub processing_help: String,
    pub processing_mode: String,
    #[serde(default)]
    pub audio_only: bool,
    #[serde(default = "default_track_index")]
    pub audio_track_index: i32,
    #[serde(default)]
    pub copy_audio: bool,
    #[serde(default)]
    pub copy_video: bool,
    pub width: i64,
    pub height: i64,
    pub aspect: f64,
    pub fps: f64,
    pub add_logo: bool,
    pub logo: String,
    pub logo_scale: String,
    pub logo_opacity: f32,
    pub logo_position: String,
    #[serde(default = "default_tracks")]
    pub audio_tracks: i32,
    #[serde(default = "default_channels")]
    pub audio_channels: u8,
    pub volume: f64,
    #[serde(default)]
    pub decoder_filter: String,

    pub ingest_help: String,
    pub ingest_enable: bool,
    pub ingest_param: String,
    #[serde(default)]
    pub ingest_filter: String,

    pub playlist_help: String,
    pub playlist_path: String,
    pub day_start: String,
    pub length: String,
    pub infinit: bool,

    pub storage_help: String,
    pub storage_path: String,
    #[serde(alias = "filler_clip")]
    pub filler: String,
    pub extensions: String,
    pub shuffle: bool,

    pub text_help: String,
    pub add_text: bool,
    pub fontfile: String,
    pub text_from_filename: bool,
    pub style: String,
    pub regex: String,

    pub task_help: String,
    pub task_enable: bool,
    pub task_path: String,

    pub output_help: String,
    pub output_mode: String,
    pub output_param: String,
}

impl Configuration {
    pub fn from(id: i32, channel_id: i32, config: PlayoutConfig) -> Self {
        Self {
            id,
            channel_id,
            general_help: config.general.help_text,
            stop_threshold: config.general.stop_threshold,
            mail_help: config.mail.help_text,
            subject: config.mail.subject,
            smtp_server: config.mail.smtp_server,
            starttls: config.mail.starttls,
            sender_addr: config.mail.sender_addr,
            sender_pass: config.mail.sender_pass,
            recipient: config.mail.recipient,
            mail_level: config.mail.mail_level.to_string(),
            interval: config.mail.interval as i64,
            logging_help: config.logging.help_text,
            ffmpeg_level: config.logging.ffmpeg_level,
            ingest_level: config.logging.ingest_level,
            detect_silence: config.logging.detect_silence,
            ignore_lines: config.logging.ignore_lines.join(";"),
            processing_help: config.processing.help_text,
            processing_mode: config.processing.mode.to_string(),
            audio_only: config.processing.audio_only,
            audio_track_index: config.processing.audio_track_index,
            copy_audio: config.processing.copy_audio,
            copy_video: config.processing.copy_video,
            width: config.processing.width,
            height: config.processing.height,
            aspect: config.processing.aspect,
            fps: config.processing.fps,
            add_logo: config.processing.add_logo,
            logo: config.processing.logo,
            logo_scale: config.processing.logo_scale,
            logo_opacity: config.processing.logo_opacity,
            logo_position: config.processing.logo_position,
            audio_tracks: config.processing.audio_tracks,
            audio_channels: config.processing.audio_channels,
            volume: config.processing.volume,
            decoder_filter: config.processing.custom_filter,
            ingest_help: config.ingest.help_text,
            ingest_enable: config.ingest.enable,
            ingest_param: config.ingest.input_param,
            ingest_filter: config.ingest.custom_filter,
            playlist_help: config.playlist.help_text,
            playlist_path: config.playlist.path.to_string_lossy().to_string(),
            day_start: config.playlist.day_start,
            length: config.playlist.length,
            infinit: config.playlist.infinit,
            storage_help: config.storage.help_text,
            storage_path: config.storage.path.to_string_lossy().to_string(),
            filler: config.storage.filler.to_string_lossy().to_string(),
            extensions: config.storage.extensions.join(";"),
            shuffle: config.storage.shuffle,
            text_help: config.text.help_text,
            add_text: config.text.add_text,
            fontfile: config.text.fontfile,
            text_from_filename: config.text.text_from_filename,
            style: config.text.style,
            regex: config.text.regex,
            task_help: config.task.help_text,
            task_enable: config.task.enable,
            task_path: config.task.path.to_string_lossy().to_string(),
            output_help: config.output.help_text,
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

#[derive(Clone, Debug, Deserialize, Serialize, sqlx::FromRow)]
pub struct AdvancedConfiguration {
    pub id: i32,
    pub channel_id: i32,
    pub decoder_input_param: Option<String>,
    pub decoder_output_param: Option<String>,
    pub encoder_input_param: Option<String>,
    pub ingest_input_param: Option<String>,
    pub deinterlace: Option<String>,
    pub pad_scale_w: Option<String>,
    pub pad_scale_h: Option<String>,
    pub pad_video: Option<String>,
    pub fps: Option<String>,
    pub scale: Option<String>,
    pub set_dar: Option<String>,
    pub fade_in: Option<String>,
    pub fade_out: Option<String>,
    pub overlay_logo_scale: Option<String>,
    pub overlay_logo_fade_in: Option<String>,
    pub overlay_logo_fade_out: Option<String>,
    pub overlay_logo: Option<String>,
    pub tpad: Option<String>,
    pub drawtext_from_file: Option<String>,
    pub drawtext_from_zmq: Option<String>,
    pub aevalsrc: Option<String>,
    pub afade_in: Option<String>,
    pub afade_out: Option<String>,
    pub apad: Option<String>,
    pub volume: Option<String>,
    pub split: Option<String>,
}
