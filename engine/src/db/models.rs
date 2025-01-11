use std::{error::Error, fmt, str::FromStr};

use once_cell::sync::OnceCell;
use regex::Regex;
use serde::{
    de::{self, Visitor},
    Deserialize, Serialize,
};
// use serde_with::{formats::CommaSeparator, serde_as, StringWithSeparator};
use sqlx::{sqlite::SqliteRow, FromRow, Pool, Row, Sqlite};

use crate::utils::config::PlayoutConfig;
use crate::{db::handles, utils::s3_utils::S3Ext};

use aws_config as s3_conf;
use aws_sdk_s3::{self as s3, config::Region, Client};

#[derive(Clone, Default, Debug, Deserialize, Serialize, sqlx::FromRow)]
pub struct GlobalSettings {
    pub id: i32,
    pub secret: Option<String>,
    pub logs: String,
    pub playlists: String,
    pub public: String,
    pub storage: String,
    pub shared: bool,
    pub mail_smtp: String,
    pub mail_user: String,
    pub mail_password: String,
    pub mail_starttls: bool,
}

impl GlobalSettings {
    pub async fn new(conn: &Pool<Sqlite>) -> Self {
        let global_settings = handles::select_global(conn);
        match global_settings.await {
            Ok(g) => g,
            Err(_) => GlobalSettings {
                id: 0,
                secret: None,
                logs: String::new(),
                playlists: String::new(),
                public: String::new(),
                storage: String::new(),
                shared: false,
                mail_smtp: String::new(),
                mail_user: String::new(),
                mail_password: String::new(),
                mail_starttls: false,
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

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Channel {
    // to-do : consider s3 in this model
    #[serde(default = "default_id", skip_deserializing)]
    pub id: i32,
    pub name: String,
    pub preview_url: String,
    pub extra_extensions: String,
    pub active: bool,
    pub public: String,
    pub playlists: String,
    #[serde(default, skip_serializing)]
    pub storage: Storage,
    pub last_date: Option<String>,
    pub time_shift: f64,
    // not in use currently
    #[serde(default, skip_serializing)]
    pub timezone: Option<String>,

    #[serde(default)]
    pub utc_offset: i32,
}

// New async function for deserialization, as `from_row` can't be async
impl Channel {
    pub async fn from_row_async(row: &SqliteRow) -> sqlx::Result<Self> {
        let storage_path: String = row.try_get("storage").unwrap_or_default(); // Assuming `storage` is a string field
        let storage = Storage::new(&storage_path).await?; // Use async `Storage::new`

        Ok(Self {
            id: row.try_get("id").unwrap_or_default(),
            name: row.try_get("name").unwrap_or_default(),
            preview_url: row.try_get("preview_url").unwrap_or_default(),
            extra_extensions: row.try_get("extra_extensions").unwrap_or_default(),
            active: row.try_get("active").unwrap_or_default(),
            public: row.try_get("public").unwrap_or_default(),
            playlists: row.try_get("playlists").unwrap_or_default(),
            storage,
            last_date: row.try_get("last_date").unwrap_or_default(),
            time_shift: row.try_get("time_shift").unwrap_or_default(),
            timezone: row.try_get("timezone").unwrap_or_default(),
            utc_offset: row.try_get("utc_offset").unwrap_or_default(),
        })
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Storage {
    raw_path: String,
    #[serde(default, skip_serializing, skip_deserializing)]
    pub baked_path: String,
    #[serde(default, skip_serializing, skip_deserializing)]
    is_s3: bool,
    #[serde(default, skip_serializing, skip_deserializing)]
    bucket_name: Option<String>,
    #[serde(default, skip_serializing, skip_deserializing)]
    s3_client: Option<Client>,
}

impl Storage {
    pub async fn new(path: &str) -> sqlx::Result<Self> {
        // Check if path is S3 and parse details if applicable
        let mut baked_path = path.to_string();
        let is_s3 = path.parse_is_s3();
        let mut bucket_name = None;
        let mut s3_client = None;

        if is_s3 {
            baked_path = String::new();
            let (credentials, bucket, endpoint_url) =
                crate::utils::s3_utils::s3_parse_string(path)?;

            bucket_name = Some(bucket);

            // Create AWS shared credentials provider
            let shared_provider = s3::config::SharedCredentialsProvider::new(credentials);
            let config = s3_conf::from_env()
                .region(Region::new("us-east-1")) // Dummy default region, replace if needed
                .credentials_provider(shared_provider)
                .load()
                .await;

            // Configure the S3 client with forced path style
            let s3_config = s3::config::Builder::from(&config)
                .endpoint_url(endpoint_url)
                .force_path_style(true)
                .build();
            s3_client = Some(s3::Client::from_conf(s3_config));
        }

        Ok(Self {
            raw_path: path.to_string(),
            baked_path,
            is_s3,
            bucket_name,
            s3_client,
        })
    }

    pub fn is_s3(&self) -> bool {
        self.is_s3
    }

    pub fn get_s3_client(&self) -> Option<Client> {
        self.s3_client.clone()
    }

    pub fn get_s3_bucket(&self) -> Option<String> {
        self.bucket_name.clone()
    }
}

impl fmt::Display for Storage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.baked_path)
    }
}

// impl Deref for Storage {
//     type Target = String;

//     fn deref(&self) -> &Self::Target {
//         &self.baked_path
//     }
// }

fn default_id() -> i32 {
    1
}

// #[serde_as]
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct User {
    #[serde(skip_deserializing)]
    pub id: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mail: Option<String>,
    pub username: String,
    #[serde(skip_serializing, default = "empty_string")]
    pub password: String,
    pub role_id: Option<i32>,
    // #[serde_as(as = "StringWithSeparator::<CommaSeparator, i32>")]
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

fn empty_string() -> String {
    "".to_string()
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
        value: sqlx::sqlite::SqliteValueRef<'r>,
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

#[derive(Clone, Debug, Deserialize, Serialize, sqlx::FromRow)]
pub struct AdvancedConfiguration {
    pub id: i32,
    pub channel_id: i32,
    pub decoder_input_param: Option<String>,
    pub decoder_output_param: Option<String>,
    pub encoder_input_param: Option<String>,
    pub ingest_input_param: Option<String>,
    pub filter_deinterlace: Option<String>,
    pub filter_pad_scale_w: Option<String>,
    pub filter_pad_scale_h: Option<String>,
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
}
