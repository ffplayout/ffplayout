use std::{error::Error, fmt, str::FromStr};

use chrono_tz::Tz;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Pool, Row, Sqlite, sqlite::SqliteRow};

use crate::{
    db::handles,
    utils::config::{OutputMode, PlayoutConfig},
};

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
        })
    }
}

fn default_id() -> i32 {
    1
}

fn default_true() -> bool {
    true
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
    #[serde(default = "default_true")]
    pub two_factor: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

impl FromRow<'_, SqliteRow> for User {
    fn from_row(row: &SqliteRow) -> sqlx::Result<Self> {
        Ok(Self {
            id: row.try_get("id").unwrap_or_default(),
            mail: row.try_get("mail").ok(),
            username: row.try_get("username").unwrap_or_default(),
            password: row.try_get("password").unwrap_or_default(),
            role_id: row.try_get("role_id").ok(),
            channel_ids: Some(
                row.try_get::<String, &str>("channel_ids")
                    .unwrap_or_default()
                    .split(',')
                    .map(|i| i.parse::<i32>().unwrap_or_default())
                    .collect(),
            ),
            two_factor: row.try_get("two_factor").unwrap_or(true),
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
#[serde(default)]
pub struct TextPreset {
    #[sqlx(default)]
    #[serde(skip_deserializing)]
    pub id: i32,
    pub channel_id: i32,
    pub name: String,
    pub text: String,
    pub use_filename: bool,
    pub font_family: String,
    pub font_weight: String,
    pub filename_regex: String,
    pub position_x: String,
    pub position_y: String,
    pub font_size: f32,
    pub line_spacing: f32,
    pub text_color: String,
    pub text_opacity: f64,
    pub background_enabled: bool,
    pub background_color: String,
    pub background_opacity: f64,
    pub background_padding: u32,
    pub opacity: f64,
    pub scroll_direction: String,
    pub scroll_speed: u32,
    pub scroll_repeat: i32,
    pub fade_in_seconds: f64,
    pub fade_out_seconds: f64,
}

impl Default for TextPreset {
    fn default() -> Self {
        Self {
            id: 0,
            channel_id: 1,
            name: String::new(),
            text: String::new(),
            use_filename: false,
            font_family: "DejaVu Sans".to_string(),
            font_weight: "normal".to_string(),
            filename_regex: r"^.+[/\\](.*)(.mp4|.mkv|.webm)$".to_string(),
            position_x: "center".to_string(),
            position_y: "end:72".to_string(),
            font_size: 24.0,
            line_spacing: 4.0,
            text_color: "#ffffff".to_string(),
            text_opacity: 1.0,
            background_enabled: false,
            background_color: "#000000".to_string(),
            background_opacity: 0.8,
            background_padding: 4,
            opacity: 1.0,
            scroll_direction: "none".to_string(),
            scroll_speed: 100,
            scroll_repeat: -1,
            fade_in_seconds: 0.0,
            fade_out_seconds: 0.0,
        }
    }
}

impl TextPreset {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("text preset name must not be empty".to_string());
        }
        if !self.font_size.is_finite() || self.font_size <= 0.0 {
            return Err("text font size must be a positive number".to_string());
        }
        if self.font_family.trim().is_empty() {
            return Err("text font family must not be empty".to_string());
        }
        if !matches!(self.font_weight.as_str(), "normal" | "semibold" | "bold") {
            return Err("invalid text font weight".to_string());
        }
        if !self.line_spacing.is_finite() || self.line_spacing < 0.0 {
            return Err("text line spacing must not be negative".to_string());
        }
        for (name, value) in [
            ("text opacity", self.text_opacity),
            ("background opacity", self.background_opacity),
            ("opacity", self.opacity),
        ] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(format!("{name} must be between 0 and 1"));
            }
        }
        if !self.fade_in_seconds.is_finite() || self.fade_in_seconds < 0.0 {
            return Err("fade-in duration must not be negative".to_string());
        }
        if !self.fade_out_seconds.is_finite() || self.fade_out_seconds < 0.0 {
            return Err("fade-out duration must not be negative".to_string());
        }
        if !matches!(
            self.scroll_direction.as_str(),
            "none" | "left_to_right" | "right_to_left"
        ) {
            return Err("invalid text scroll direction".to_string());
        }
        if self.scroll_direction != "none" && self.scroll_speed == 0 {
            return Err("text scroll speed must be greater than zero".to_string());
        }
        if self.scroll_repeat < -1 {
            return Err("text scroll repeat must be -1 or greater".to_string());
        }
        if self.use_filename {
            Regex::new(&self.filename_regex)
                .map_err(|error| format!("invalid filename regex: {error}"))?;
        }
        Ok(())
    }
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
    pub processing_add_logo: bool,
    pub processing_logo: String,
    pub processing_logo_scale: String,
    pub processing_logo_opacity: f64,
    pub processing_logo_position: String,
    pub processing_volume: f64,
    #[serde(default)]
    pub processing_vtt_enable: bool,
    #[serde(default)]
    pub processing_vtt_dummy: Option<String>,
    #[serde(default = "default_vtt_name")]
    pub processing_vtt_name: String,
    #[serde(default = "default_vtt_language")]
    pub processing_vtt_language: String,
    #[serde(default)]
    pub processing_vtt_default: bool,

    pub ingest_enable: bool,
    pub ingest_url: String,

    pub playlist_day_start: String,
    pub playlist_length: String,
    pub playlist_infinit: bool,

    pub storage_filler: String,
    pub storage_extensions: String,
    pub storage_shuffle: bool,

    pub text_preset_id: Option<i32>,

    pub task_enable: bool,
    pub task_path: String,

    pub output_id: i32,
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
            processing_add_logo: config.processing.add_logo,
            processing_logo: config.processing.logo,
            processing_logo_scale: config.processing.logo_scale,
            processing_logo_opacity: config.processing.logo_opacity,
            processing_logo_position: config.processing.logo_position,
            processing_volume: config.processing.volume,
            processing_vtt_enable: config.processing.vtt_enable,
            processing_vtt_dummy: config.processing.vtt_dummy,
            processing_vtt_name: config.processing.vtt_name,
            processing_vtt_language: config.processing.vtt_language,
            processing_vtt_default: config.processing.vtt_default,
            ingest_enable: config.ingest.enable,
            ingest_url: config.ingest.ingest_url,
            playlist_day_start: config.playlist.day_start,
            playlist_length: config.playlist.length,
            playlist_infinit: config.playlist.infinit,
            storage_filler: config.storage.filler,
            storage_extensions: config.storage.extensions.join(";"),
            storage_shuffle: config.storage.shuffle,
            text_preset_id: config.text.preset_id,
            task_enable: config.task.enable,
            task_path: config.task.path.to_string_lossy().to_string(),
            output_id: config.output.id,
        }
    }
}

#[derive(Clone, Default, Debug, Deserialize, Serialize, sqlx::FromRow)]
pub struct Output {
    pub id: i32,
    pub channel_id: i32,
    pub name: String,
    pub hls_variants: String,
    pub stream_url: String,
    pub hls_playlist_name: Option<String>,
    pub hls_segment_duration: Option<i64>,
    pub hls_list_size: Option<i64>,
    #[sqlx(default)]
    #[serde(default)]
    pub desktop_fullscreen: bool,
    pub width: i64,
    pub height: i64,
    pub fps: f64,
    pub video_preset: Option<String>,
    pub rate_control: Option<String>,
    pub video_quality: Option<i64>,
    pub video_maxrate: Option<i64>,
    pub audio_bitrate: Option<i64>,
}

impl Output {
    pub fn new(channel_id: i32, mode: OutputMode) -> Self {
        let stream_url = if mode == OutputMode::Stream {
            "rtmp://127.0.0.1/live/stream".to_string()
        } else {
            Default::default()
        };
        let hls_playlist_name = (mode == OutputMode::HLS).then(|| "stream".to_string());
        let hls_segment_duration = (mode == OutputMode::HLS).then_some(6);
        let hls_list_size = (mode == OutputMode::HLS).then_some(600);
        let encoded = matches!(mode, OutputMode::HLS | OutputMode::Stream);

        Self {
            id: 0,
            channel_id,
            name: mode.to_string(),
            hls_variants: String::new(),
            stream_url,
            hls_playlist_name,
            hls_segment_duration,
            hls_list_size,
            desktop_fullscreen: false,
            width: 1280,
            height: 720,
            fps: 25.0,
            video_preset: encoded.then(|| "faster".to_string()),
            rate_control: encoded.then(|| "crf".to_string()),
            video_quality: encoded.then_some(23),
            video_maxrate: encoded.then_some(2400),
            audio_bitrate: encoded.then_some(128),
        }
    }
}

fn default_vtt_name() -> String {
    "Subtitles".to_string()
}

fn default_vtt_language() -> String {
    "und".to_string()
}
