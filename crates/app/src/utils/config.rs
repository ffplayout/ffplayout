use std::{
    collections::{BTreeMap, HashSet},
    fmt,
    path::{Path, PathBuf},
    str::FromStr,
};

use chrono::NaiveTime;
use chrono_tz::Tz;
use flexi_logger::Level;
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};
use tokio::{fs, io::AsyncReadExt};
use ts_rs::TS;

use crate::{
    ARGS,
    db::{handles, models},
    file::norm_abs_path,
    utils::{errors::ServiceError, paths::validate_directory_path, time_to_sec},
};

pub const DUMMY_LEN: f64 = 60.0;
pub(crate) const ENGINE_AUDIO_SAMPLE_RATE: u32 = 48_000;

#[derive(Debug, Clone, Default, Eq, PartialEq, Deserialize, Serialize, TS)]
#[ts(export, export_to = "playout_config.d.ts")]
#[serde(rename_all = "lowercase")]
pub enum OutputMode {
    Desktop,
    #[default]
    HLS,
    Stream,
    Moq,
}

impl OutputMode {
    fn new(s: &str) -> Self {
        match s {
            "desktop" => Self::Desktop,
            "stream" => Self::Stream,
            "moq" => Self::Moq,
            _ => Self::HLS,
        }
    }
}

impl FromStr for OutputMode {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "desktop" => Ok(Self::Desktop),
            "hls" => Ok(Self::HLS),
            "stream" => Ok(Self::Stream),
            "moq" => Ok(Self::Moq),
            _ => Err("Use 'desktop', 'hls' or 'stream'".to_string()),
        }
    }
}

impl fmt::Display for OutputMode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            OutputMode::Desktop => write!(f, "desktop"),
            OutputMode::HLS => write!(f, "hls"),
            OutputMode::Stream => write!(f, "stream"),
            OutputMode::Moq => write!(f, "moq"),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, TS)]
#[ts(export, export_to = "playout_config.d.ts")]
#[serde(rename_all = "lowercase")]
pub enum StreamType {
    #[default]
    Rtmp,
    Srt,
    Udp,
    Custom,
}

impl StreamType {
    fn ffmpeg_target(self) -> Option<ff_engine::FfmpegOutputTarget> {
        match self {
            Self::Rtmp => Some(ff_engine::FfmpegOutputTarget::Rtmp),
            Self::Srt => Some(ff_engine::FfmpegOutputTarget::Srt),
            Self::Udp => Some(ff_engine::FfmpegOutputTarget::Udp),
            Self::Custom => None,
        }
    }

    pub fn engine_stream_type(self) -> ff_engine::StreamType {
        match self {
            Self::Rtmp => ff_engine::StreamType::Rtmp,
            Self::Srt => ff_engine::StreamType::Srt,
            Self::Udp => ff_engine::StreamType::Udp,
            Self::Custom => ff_engine::StreamType::Custom,
        }
    }
}

impl FromStr for StreamType {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "rtmp" => Ok(Self::Rtmp),
            "srt" => Ok(Self::Srt),
            "udp" => Ok(Self::Udp),
            "custom" => Ok(Self::Custom),
            _ => Err("Use 'rtmp', 'srt', 'udp' or 'custom'".to_string()),
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, Eq, PartialEq, TS)]
#[ts(export, export_to = "playout_config.d.ts")]
#[serde(rename_all = "lowercase")]
pub enum ProcessMode {
    Folder,
    #[default]
    Playlist,
}

impl ProcessMode {
    fn new(s: &str) -> Self {
        match s {
            "folder" => Self::Folder,
            _ => Self::Playlist,
        }
    }
}

impl fmt::Display for ProcessMode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            ProcessMode::Folder => write!(f, "folder"),
            ProcessMode::Playlist => write!(f, "playlist"),
        }
    }
}

impl FromStr for ProcessMode {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "folder" => Ok(Self::Folder),
            "playlist" => Ok(Self::Playlist),
            _ => Err("Use 'folder' or 'playlist'".to_string()),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
pub struct Template {
    pub sources: Vec<Source>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
pub struct Source {
    #[ts(type = "string")]
    pub start: NaiveTime,
    #[ts(type = "string")]
    pub duration: NaiveTime,
    pub shuffle: bool,
    pub paths: Vec<PathBuf>,
}

/// Channel Config
///
/// This we init ones, when ffplayout is starting and use them globally in the hole program.
#[derive(Debug, Default, Clone, Deserialize, Serialize, TS)]
#[ts(export, export_to = "playout_config.d.ts")]
pub struct PlayoutConfig {
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub channel: Channel,
    pub general: General,
    pub mail: Mail,
    #[serde(default)]
    pub notification: Notification,
    pub logging: Logging,
    pub processing: Processing,
    pub audio: Audio,
    pub ingest: Ingest,
    pub playlist: Playlist,
    pub storage: Storage,
    pub text: Text,
    pub task: Task,
    pub recording: Recording,
    #[serde(alias = "out")]
    pub output: Output,
    #[serde(default)]
    pub program: Option<ProgramFormat>,
    // Additional configured instances, including disabled ones.
    #[serde(default)]
    pub outputs: Vec<Output>,
}

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[ts(export, export_to = "playout_config.d.ts")]
pub struct ProgramFormat {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
}

#[derive(Debug, Default, Clone, Copy, Deserialize, Serialize, TS, PartialEq, Eq)]
#[ts(export, export_to = "playout_config.d.ts")]
#[serde(rename_all = "snake_case")]
pub enum EncodingSource {
    HlsVariant,
    #[default]
    Stream,
    Encode,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, TS)]
#[ts(export, export_to = "playout_config.d.ts")]
pub struct Recording {
    pub enable: bool,
    pub source: EncodingSource,
    pub source_output_id: Option<i32>,
    pub variant: String,
    pub path: String,
    pub segment_duration: u32,
    pub retention_days: u32,
    pub minimum_free_space_gb: u32,
    pub width: u32,
    pub height: u32,
    pub video_codec: String,
    pub video_options: BTreeMap<String, String>,
    pub audio_codec: String,
    #[serde(default)]
    pub audio_options: BTreeMap<String, String>,
    pub audio_bitrate: u32,
}

impl Recording {
    fn new(recording: &models::Recording) -> Self {
        Self {
            enable: recording.enabled,
            source: match recording.source.as_str() {
                "hls_variant" => EncodingSource::HlsVariant,
                "encode" => EncodingSource::Encode,
                _ => EncodingSource::Stream,
            },
            source_output_id: recording.source_output_id,
            variant: recording.hls_variant.clone(),
            path: recording.path.clone(),
            segment_duration: u32::try_from(recording.segment_duration).unwrap_or(300),
            retention_days: u32::try_from(recording.retention_days).unwrap_or_default(),
            minimum_free_space_gb: u32::try_from(recording.minimum_free_space_gb)
                .unwrap_or_default(),
            width: u32::try_from(recording.width).unwrap_or_default(),
            height: u32::try_from(recording.height).unwrap_or_default(),
            video_codec: recording.video_codec.clone(),
            video_options: serde_json::from_str(&recording.video_options)
                .unwrap_or_else(|_| ff_engine::video_option_defaults(&recording.video_codec)),
            audio_codec: recording.audio_codec.clone(),
            audio_options: serde_json::from_str(&recording.audio_options).unwrap_or_default(),
            audio_bitrate: u32::try_from(recording.audio_bitrate).unwrap_or(128),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.enable {
            return Ok(());
        }
        validate_directory_path("Recording", &self.path)?;
        if !(30..=3600).contains(&self.segment_duration) {
            return Err(
                "recording segment duration must be between 30 and 3600 seconds".to_string(),
            );
        }
        if self.source == EncodingSource::HlsVariant && self.variant.trim().is_empty() {
            return Err("recording HLS variant must not be empty".to_string());
        }
        if self.source != EncodingSource::Encode && self.source_output_id.is_none() {
            return Err("recording source output must be selected".to_string());
        }
        if self.source == EncodingSource::Encode {
            if (self.width == 0) != (self.height == 0) {
                return Err(
                    "recording width and height must both be set or both be zero".to_string(),
                );
            }
            let capabilities = ff_engine::ffmpeg_capabilities();
            if !capabilities
                .video_codecs_for(ff_engine::FfmpegOutputTarget::Matroska)
                .iter()
                .any(|codec| codec.name == self.video_codec)
            {
                return Err("unsupported recording video codec".to_string());
            }
            if !capabilities
                .audio_codecs_for(ff_engine::FfmpegOutputTarget::Matroska)
                .iter()
                .any(|codec| codec.name == self.audio_codec && !codec.hardware)
            {
                return Err("unsupported recording audio codec".to_string());
            }
            ff_engine::validate_video_options(&self.video_codec, &self.video_options)?;
            ff_engine::validate_audio_options(
                &self.audio_codec,
                &self.audio_options,
                ENGINE_AUDIO_SAMPLE_RATE,
                u64::from(self.audio_bitrate) * 1_000,
            )?;
            if ff_engine::audio_codec_uses_bitrate(&self.audio_codec) && self.audio_bitrate == 0 {
                return Err("recording audio bitrate must be greater than zero".to_string());
            }
        }
        Ok(())
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, TS)]
pub struct Channel {
    pub logs: PathBuf,
    pub public: PathBuf,
    pub playlists: PathBuf,
    pub storage: PathBuf,
    pub shared: bool,
    #[ts(type = "string")]
    pub timezone: Option<Tz>,
}

impl Channel {
    pub fn new(config: &models::GlobalSettings, channel: models::Channel) -> Self {
        Self {
            logs: PathBuf::from(config.logs.clone()),
            public: PathBuf::from(channel.public.clone()),
            playlists: PathBuf::from(channel.playlists.clone()),
            storage: PathBuf::from(channel.storage.clone()),
            shared: config.shared,
            timezone: channel.timezone,
        }
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, TS)]
#[ts(export, export_to = "playout_config.d.ts")]
pub struct General {
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub id: i32,
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub channel_id: i32,
    pub stop_threshold: f64,
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub generate: Option<Vec<String>>,
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub ffmpeg_filters: Vec<String>,
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub ffmpeg_libs: Vec<String>,
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub ffmpeg_options: Vec<String>,
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub template: Option<Template>,
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub skip_validation: bool,
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub validate: bool,
}

impl General {
    fn new(config: &models::Configuration) -> Self {
        Self {
            id: config.id,
            channel_id: config.channel_id,
            stop_threshold: config.general_stop_threshold,
            generate: None,
            ffmpeg_filters: vec![],
            ffmpeg_libs: vec![],
            ffmpeg_options: vec![],
            template: None,
            skip_validation: false,
            validate: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[ts(export, export_to = "playout_config.d.ts")]
pub struct Mail {
    #[serde(skip_deserializing)]
    pub show: bool,
    pub subject: String,
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub smtp_server: String,
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub smtp_starttls: bool,
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub smtp_user: String,
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub smtp_password: String,
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub smtp_port: u16,
    pub recipient: String,
    #[ts(type = "string")]
    pub mail_level: Level,
    pub interval: i64,
}

impl Mail {
    fn new(global: &models::GlobalSettings, config: &models::Configuration) -> Self {
        Self {
            show: !global.smtp_password.is_empty() && global.smtp_server != "mail.example.org",
            subject: config.mail_subject.clone(),
            smtp_server: global.smtp_server.clone(),
            smtp_starttls: global.smtp_starttls,
            smtp_user: global.smtp_user.clone(),
            smtp_password: global.smtp_password.clone(),
            smtp_port: global.smtp_port,
            recipient: config.mail_recipient.clone(),
            mail_level: string_to_log_level(config.mail_level.clone()),
            interval: config.mail_interval,
        }
    }
}

impl Default for Mail {
    fn default() -> Self {
        Mail {
            show: false,
            subject: String::default(),
            smtp_server: String::default(),
            smtp_starttls: bool::default(),
            smtp_user: String::default(),
            smtp_password: String::default(),
            smtp_port: 465,
            recipient: String::default(),
            mail_level: Level::Debug,
            interval: i64::default(),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, Deserialize, Serialize, TS, PartialEq, Eq)]
#[ts(export, export_to = "playout_config.d.ts")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NotificationLevel {
    Info,
    Warning,
    Error,
    #[default]
    Fatal,
}

impl fmt::Display for NotificationLevel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Info => "INFO",
            Self::Warning => "WARNING",
            Self::Error => "ERROR",
            Self::Fatal => "FATAL",
        })
    }
}

impl NotificationLevel {
    fn new(value: &str) -> Self {
        match value {
            "INFO" => Self::Info,
            "WARNING" => Self::Warning,
            "ERROR" => Self::Error,
            "FATAL" => Self::Fatal,
            invalid => {
                log::warn!(
                    target: "notification",
                    "Unknown notification level {invalid:?}; falling back to FATAL"
                );
                Self::Fatal
            }
        }
    }

    pub fn accepts(self, level: Level, fatal: bool) -> bool {
        match self {
            Self::Fatal => fatal,
            Self::Error => level <= Level::Error,
            Self::Warning => level <= Level::Warn,
            Self::Info => level <= Level::Info,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[ts(export, export_to = "playout_config.d.ts")]
pub struct Notification {
    #[serde(skip_deserializing)]
    pub show: bool,
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub server: String,
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub token: String,
    pub topic: String,
    pub level: NotificationLevel,
    pub tags: String,
}

impl Notification {
    fn new(global: &models::GlobalSettings, config: &models::Configuration) -> Self {
        Self {
            show: !global.notification_server.is_empty(),
            server: global.notification_server.clone(),
            token: global.notification_token.clone(),
            topic: config.notification_topic.clone(),
            level: NotificationLevel::new(&config.notification_level),
            tags: config.notification_tags.clone(),
        }
    }
}

impl Default for Notification {
    fn default() -> Self {
        Self {
            show: false,
            server: String::new(),
            token: String::new(),
            topic: String::new(),
            level: NotificationLevel::Fatal,
            tags: String::new(),
        }
    }
}

#[cfg(test)]
mod notification_level_tests {
    use flexi_logger::Level;

    use super::NotificationLevel;

    #[test]
    fn fatal_notifications_only_accept_fatal_records() {
        assert!(NotificationLevel::Fatal.accepts(Level::Error, true));
        assert!(!NotificationLevel::Fatal.accepts(Level::Error, false));
    }

    #[test]
    fn error_notifications_include_fatal_and_regular_errors() {
        assert!(NotificationLevel::Error.accepts(Level::Error, true));
        assert!(NotificationLevel::Error.accepts(Level::Error, false));
        assert!(!NotificationLevel::Error.accepts(Level::Warn, false));
    }

    #[test]
    fn info_notifications_exclude_debug_and_trace_records() {
        assert!(NotificationLevel::Info.accepts(Level::Info, false));
        assert!(!NotificationLevel::Info.accepts(Level::Debug, false));
        assert!(!NotificationLevel::Info.accepts(Level::Trace, false));
    }

    #[test]
    fn unknown_notification_level_falls_back_to_fatal() {
        assert_eq!(NotificationLevel::new("INVALID"), NotificationLevel::Fatal);
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, TS)]
#[ts(export, export_to = "playout_config.d.ts")]
pub struct Logging {
    pub ffmpeg_level: String,
    pub ingest_level: String,
    pub detect_silence: bool,
    pub ignore_lines: Vec<String>,
}

impl Logging {
    fn new(config: &models::Configuration) -> Self {
        Self {
            ffmpeg_level: config.logging_ffmpeg_level.clone(),
            ingest_level: config.logging_ingest_level.clone(),
            detect_silence: config.logging_detect_silence,
            ignore_lines: config
                .logging_ignore
                .split(';')
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(String::from)
                .collect(),
        }
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, TS)]
#[ts(export, export_to = "playout_config.d.ts")]
pub struct Processing {
    pub mode: ProcessMode,
    pub add_logo: bool,
    pub logo: String,
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub logo_path: String,
    pub logo_scale: String,
    pub logo_opacity: f64,
    pub logo_position: String,
    #[serde(default)]
    pub vtt_enable: bool,
    #[serde(default)]
    pub vtt_dummy: Option<String>,
    #[serde(default = "default_vtt_name")]
    pub vtt_name: String,
    #[serde(default = "default_vtt_language")]
    pub vtt_language: String,
    #[serde(default)]
    pub vtt_default: bool,
}

fn default_vtt_name() -> String {
    "Subtitles".to_string()
}

fn default_vtt_language() -> String {
    "und".to_string()
}

impl Processing {
    fn new(config: &models::Configuration) -> Self {
        Self {
            mode: ProcessMode::new(&config.processing_mode.clone()),
            add_logo: config.processing_add_logo,
            logo: config.processing_logo.clone(),
            logo_path: config.processing_logo.clone(),
            logo_scale: config.processing_logo_scale.clone(),
            logo_opacity: config.processing_logo_opacity,
            logo_position: config.processing_logo_position.clone(),
            vtt_enable: config.processing_vtt_enable,
            vtt_dummy: config.processing_vtt_dummy.clone(),
            vtt_name: config.processing_vtt_name.clone(),
            vtt_language: config.processing_vtt_language.clone(),
            vtt_default: config.processing_vtt_default,
        }
    }

    pub fn hls_subtitle(&self) -> Result<Option<ff_engine::HlsSubtitle>, String> {
        if !self.vtt_enable {
            return Ok(None);
        }

        let subtitle = ff_engine::HlsSubtitle {
            name: self.vtt_name.trim().to_string(),
            language: self.vtt_language.trim().to_string(),
            default: self.vtt_default,
        };
        subtitle.validate()?;
        Ok(Some(subtitle))
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, TS)]
#[ts(export, export_to = "playout_config.d.ts")]
pub struct Audio {
    pub volume: f64,
    #[ts(type = "\"all\" | \"live\" | \"off\"")]
    pub loudness_scope: String,
    pub compressor_ratio: f64,
    pub compressor_attack_ms: f64,
    pub compressor_hold_ms: f64,
    pub compressor_release_ms: f64,
    pub compressor_strong_release_ms: f64,
    pub compressor_knee_db: f64,
    pub pause_hold_ms: f64,
    pub pause_return_delay_ms: f64,
    pub loudness_output_max_correction_db: f64,
    pub loudness_output_gain_up_db_per_second: f64,
    pub loudness_output_gain_down_db_per_second: f64,

    pub compressor_threshold_dbfs: f64,
    pub pause_threshold_dbfs: f64,
    // Derived from loudness_scope.
    pub loudness_enable: bool,
    pub loudness_target_lufs: f64,
    pub loudness_dead_band_lu: f64,
    pub loudness_max_gain_db: f64,
    pub loudness_max_attenuation_db: f64,
    pub loudness_gain_up_db_per_second: f64,
    pub loudness_gain_down_db_per_second: f64,
    pub loudness_silence_gate_lufs: f64,
    pub loudness_true_peak_ceiling_dbtp: f64,
}

impl Audio {
    fn new(config: &models::Configuration) -> Self {
        Self {
            volume: config.processing_volume,
            loudness_scope: config.processing_loudness_scope.clone(),
            compressor_ratio: config.processing_compressor_ratio,
            compressor_attack_ms: config.processing_compressor_attack_ms,
            compressor_hold_ms: config.processing_compressor_hold_ms,
            compressor_release_ms: config.processing_compressor_release_ms,
            compressor_strong_release_ms: config.processing_compressor_strong_release_ms,
            compressor_knee_db: config.processing_compressor_knee_db,
            pause_hold_ms: config.processing_pause_hold_ms,
            pause_return_delay_ms: config.processing_pause_return_delay_ms,
            loudness_output_max_correction_db: config.processing_loudness_output_max_correction_db,
            loudness_output_gain_up_db_per_second: config
                .processing_loudness_output_gain_up_db_per_second,
            loudness_output_gain_down_db_per_second: config
                .processing_loudness_output_gain_down_db_per_second,

            compressor_threshold_dbfs: config.processing_compressor_threshold_dbfs,
            pause_threshold_dbfs: config.processing_pause_threshold_dbfs,
            loudness_enable: config.processing_loudness_scope != "off",
            loudness_target_lufs: config.processing_loudness_target_lufs,
            loudness_dead_band_lu: config.processing_loudness_dead_band_lu,
            loudness_max_gain_db: config.processing_loudness_max_gain_db,
            loudness_max_attenuation_db: config.processing_loudness_max_attenuation_db,
            loudness_gain_up_db_per_second: config.processing_loudness_gain_up_db_per_second,
            loudness_gain_down_db_per_second: config.processing_loudness_gain_down_db_per_second,
            loudness_silence_gate_lufs: config.processing_loudness_silence_gate_lufs,
            loudness_true_peak_ceiling_dbtp: config.processing_loudness_true_peak_ceiling_dbtp,
        }
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, TS)]
#[ts(export, export_to = "playout_config.d.ts")]
pub struct Ingest {
    pub listeners: Vec<LiveInput>,
}

fn default_auto_takeover() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, TS)]
#[ts(export, export_to = "playout_config.d.ts")]
pub struct LiveInput {
    #[serde(default = "default_auto_takeover")]
    pub auto_takeover: bool,
    #[serde(default = "default_live_idle_timeout_ms")]
    pub idle_timeout_ms: u32,
    pub id: i32,
    pub priority: i32,
    pub enabled: bool,
    pub name: String,
    pub backend: String,
    pub identifier: String,
    pub options: BTreeMap<String, String>,
    pub demuxer_options: BTreeMap<String, String>,
}

fn default_live_idle_timeout_ms() -> u32 {
    ff_engine::DEFAULT_LIVE_IDLE_TIMEOUT_MS
}

impl Default for LiveInput {
    fn default() -> Self {
        Self {
            auto_takeover: true,
            idle_timeout_ms: default_live_idle_timeout_ms(),
            id: 0,
            priority: 0,
            enabled: false,
            name: String::new(),
            backend: String::new(),
            identifier: String::new(),
            options: BTreeMap::new(),
            demuxer_options: BTreeMap::new(),
        }
    }
}

impl LiveInput {
    pub fn listen_port(&self) -> Result<u16, String> {
        match self.backend.as_str() {
            "rtmp" => parse_rtmp_ingest_port(&self.identifier),
            "srt" => {
                let url = reqwest::Url::parse(&self.identifier)
                    .map_err(|_| "invalid SRT listener URL".to_string())?;

                if url.scheme() != "srt"
                    || url.host_str().is_none()
                    || !url.username().is_empty()
                    || url.password().is_some()
                    || url.query().is_some()
                    || url.fragment().is_some()
                    || !matches!(url.path(), "" | "/")
                {
                    return Err("SRT listener must use srt://host:port".to_string());
                }

                let port = url.port().ok_or("SRT listener must include a port")?;

                if port < MIN_INGEST_PORT {
                    return Err(format!("listener port must be at least {MIN_INGEST_PORT}"));
                }

                Ok(port)
            }
            _ => Err("unsupported live input backend".to_string()),
        }
    }
}

pub const MIN_INGEST_PORT: u16 = 1024;
pub const DEFAULT_INGEST_PORT: u16 = 1936;

/// Extract the explicit listen port from an RTMP ingest URL.
///
/// The RTMP listener must use an unprivileged port so every channel can be
/// started by the regular service user.
pub fn parse_rtmp_ingest_port(url: &str) -> Result<u16, String> {
    let authority_and_path = url
        .strip_prefix("rtmp://")
        .ok_or_else(|| "ingest URL must use the rtmp:// scheme".to_string())?;
    let authority = authority_and_path
        .split_once('/')
        .map(|(authority, _)| authority)
        .ok_or_else(|| "ingest URL must include a stream path".to_string())?;

    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        let (host, port) = rest
            .split_once("]:")
            .ok_or_else(|| "ingest URL must include a port".to_string())?;
        (host, port)
    } else {
        authority
            .rsplit_once(':')
            .ok_or_else(|| "ingest URL must include a port".to_string())?
    };

    if host.is_empty() {
        return Err("ingest URL must include a host".to_string());
    }

    let port = port
        .parse::<u16>()
        .map_err(|_| "ingest URL port must be between 1024 and 65535".to_string())?;
    if port < MIN_INGEST_PORT {
        return Err(format!(
            "ingest URL port must be between {MIN_INGEST_PORT} and 65535"
        ));
    }

    Ok(port)
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, TS)]
#[ts(export, export_to = "playout_config.d.ts")]
pub struct Playlist {
    pub day_start: String,
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub start_sec: Option<f64>,
    pub length: String,
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub length_sec: Option<f64>,
    pub infinit: bool,
}

impl Playlist {
    fn new(config: &models::Configuration) -> Self {
        Self {
            day_start: config.playlist_day_start.clone(),
            start_sec: None,
            length: config.playlist_length.clone(),
            length_sec: None,
            infinit: config.playlist_infinit,
        }
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, TS)]
#[ts(export, export_to = "playout_config.d.ts")]
pub struct Storage {
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub path: PathBuf,
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub paths: Vec<PathBuf>,
    pub filler: String,
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub filler_path: PathBuf,
    pub extensions: Vec<String>,
    pub shuffle: bool,
    #[serde(skip_deserializing)]
    pub shared_storage: bool,
}

impl Storage {
    fn new(config: &models::Configuration, path: PathBuf, shared_storage: bool) -> Self {
        Self {
            path,
            paths: vec![],
            filler: config.storage_filler.clone(),
            filler_path: PathBuf::from(config.storage_filler.clone()),
            extensions: config
                .storage_extensions
                .split(';')
                .map(String::from)
                .collect(),
            shuffle: config.storage_shuffle,
            shared_storage,
        }
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, TS)]
#[ts(export, export_to = "playout_config.d.ts")]
pub struct Text {
    pub preset_id: Option<i32>,
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub preset: Option<models::TextPreset>,
}

impl Text {
    fn new(config: &models::Configuration, preset: Option<models::TextPreset>) -> Self {
        Self {
            preset_id: config.text_preset_id,
            preset,
        }
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, TS)]
#[ts(export, export_to = "playout_config.d.ts")]
pub struct Task {
    pub enable: bool,
    pub path: PathBuf,
}

impl Task {
    fn new(config: &models::Configuration) -> Self {
        Self {
            enable: config.task_enable,
            path: PathBuf::from(config.task_path.clone()),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[ts(export, export_to = "playout_config.d.ts")]
pub struct Output {
    pub id: i32,
    #[serde(default)]
    pub label: String,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    #[serde(default)]
    pub inherit_resolution: bool,
    #[serde(default = "enabled_by_default")]
    pub share_encoding: bool,
    #[serde(default)]
    pub encoding_source_id: Option<i32>,
    #[serde(default)]
    pub encoding_variant: String,
    #[serde(default)]
    pub public_moq: bool,
    pub mode: OutputMode,
    #[serde(skip)]
    #[ts(skip)]
    pub discard: bool,
    #[serde(default)]
    pub stream_url: String,
    #[serde(default)]
    pub stream_type: StreamType,
    #[serde(default)]
    pub stream_format: String,
    #[serde(default = "default_hls_playlist_name")]
    pub hls_playlist_name: String,
    #[serde(default = "default_hls_segment_duration")]
    pub hls_segment_duration: u32,
    #[serde(default = "default_hls_list_size")]
    pub hls_list_size: u32,
    #[serde(default)]
    pub desktop_fullscreen: bool,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    #[serde(default = "default_video_codec")]
    pub video_codec: String,
    #[serde(default)]
    pub video_options: BTreeMap<String, String>,
    /// FFmpeg AVIO/protocol options used while opening a network stream.
    #[serde(default)]
    pub protocol_options: BTreeMap<String, String>,
    /// FFmpeg muxer options for this output, such as HLS `hls_flags`.
    #[serde(default)]
    pub muxer_options: BTreeMap<String, String>,
    /// Global container metadata. Availability depends on the output muxer.
    #[serde(default)]
    pub metadata_options: BTreeMap<String, String>,
    #[serde(default = "default_audio_codec")]
    pub audio_codec: String,
    /// FFmpeg AVOptions for the selected audio encoder.
    #[serde(default)]
    pub audio_options: BTreeMap<String, String>,
    #[serde(default = "default_audio_bitrate")]
    pub audio_bitrate: u32,
    /// Adaptive HLS renditions, one per entry, each formatted as
    /// `NAME:WIDTHxHEIGHT:VIDEO_BITRATE[:AUDIO_BITRATE]` (e.g.
    /// `high:1920x1080:5000k:192k`). Only relevant when `mode == HLS`;
    /// entries are added to the base rendition configured directly on this
    /// output.
    #[serde(default)]
    pub hls_variants: Vec<String>,
}

const fn enabled_by_default() -> bool {
    true
}

fn default_hls_playlist_name() -> String {
    "stream".to_string()
}

const fn default_hls_segment_duration() -> u32 {
    6
}

const fn default_hls_list_size() -> u32 {
    600
}

fn default_video_codec() -> String {
    "libx264".to_string()
}

fn default_audio_codec() -> String {
    "aac".to_string()
}

const fn default_audio_bitrate() -> u32 {
    128
}

impl Default for Output {
    fn default() -> Self {
        Self::from_record(models::Output::new(0, OutputMode::default()))
    }
}

impl Output {
    fn encoder_video_options(&self) -> BTreeMap<String, String> {
        let mut options = self.video_options.clone();
        if self.mode == OutputMode::Moq && self.video_codec == "libx264" {
            options.insert("tune".into(), "zerolatency".into());
        }

        options
    }

    /// Resolve the CLI-only sink without exposing it to config serialization or the DB.
    pub(crate) fn for_playout(&self) -> Self {
        let mut output = self.clone();
        if output.discard {
            output.mode = OutputMode::Stream;
            output.stream_type = StreamType::Custom;
            output.stream_format = "null".into();
            output.stream_url = if cfg!(windows) { "NUL" } else { "/dev/null" }.into();
            output.protocol_options.clear();
            output.muxer_options.clear();
        }

        output
    }

    fn new(config: &models::Configuration, outputs: Vec<models::Output>) -> Self {
        let output = outputs
            .iter()
            .find(|output| output.id == config.output_id)
            .cloned()
            .unwrap_or_default();
        Self::from_record(output)
    }

    pub(crate) fn from_record(output: models::Output) -> Self {
        let mode = OutputMode::new(&output.name);
        let video_codec = output.video_codec.unwrap_or_else(default_video_codec);
        let video_options = serde_json::from_str(&output.video_options)
            .unwrap_or_else(|_| ff_engine::video_option_defaults(&video_codec));
        // The former MoQ TCP bridge stored protocol options. Native publishing
        // has no FFmpeg network transport, so retire those settings on load.
        let protocol_options = if mode == OutputMode::Moq {
            BTreeMap::new()
        } else {
            serde_json::from_str(&output.protocol_options).unwrap_or_default()
        };
        let muxer_options = serde_json::from_str(&output.muxer_options).unwrap_or_default();
        let metadata_options = serde_json::from_str(&output.metadata_options).unwrap_or_default();
        let audio_options = serde_json::from_str(&output.audio_options).unwrap_or_default();

        Self {
            id: output.id,
            label: output.label,
            enabled: output.active || !output.configured,
            inherit_resolution: output.inherit_resolution,
            share_encoding: output.share_encoding,
            encoding_source_id: if output.share_encoding {
                output.encoding_source_id
            } else {
                None
            },
            encoding_variant: output.encoding_variant,
            public_moq: output.public_moq,
            mode,
            discard: false,
            stream_url: output.stream_url,
            stream_type: output
                .stream_type
                .as_deref()
                .unwrap_or("rtmp")
                .parse()
                .unwrap_or_default(),
            stream_format: output.stream_format.unwrap_or_default(),
            hls_playlist_name: output
                .hls_playlist_name
                .unwrap_or_else(default_hls_playlist_name),
            hls_segment_duration: output
                .hls_segment_duration
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or_else(default_hls_segment_duration),
            hls_list_size: output
                .hls_list_size
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or_else(default_hls_list_size),
            desktop_fullscreen: output.desktop_fullscreen,
            width: u32::try_from(output.width).unwrap_or(1280),
            height: u32::try_from(output.height).unwrap_or(720),
            fps: output.fps,
            video_codec,
            video_options,
            protocol_options,
            muxer_options,
            metadata_options,
            audio_codec: output.audio_codec.unwrap_or_else(default_audio_codec),
            audio_options,
            audio_bitrate: output
                .audio_bitrate
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or_else(default_audio_bitrate),
            hls_variants: output
                .hls_variants
                .split(';')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_string)
                .collect(),
        }
    }

    /// Parses `hls_variants` into engine-ready [`ff_engine::HlsVariant`]s,
    /// returning a descriptive error for the offending entry on failure.
    pub fn parsed_hls_variants(&self) -> Result<Vec<ff_engine::HlsVariant>, String> {
        self.hls_variants
            .iter()
            .map(|spec| {
                spec.parse::<ff_engine::HlsVariant>()
                    .map_err(|e| format!("invalid HLS variant \"{spec}\": {e}"))
            })
            .collect()
    }

    /// Returns all HLS renditions, with the configured base output first and
    /// additional variants appended.
    pub fn hls_streams(&self) -> Result<Vec<ff_engine::HlsVariant>, String> {
        let base = ff_engine::HlsVariant {
            name: self.hls_playlist_name.trim().to_string(),
            width: self.width,
            height: self.height,
            video_bitrate: self.video_maxrate(),
            audio_bitrate: u64::from(self.audio_bitrate) * 1_000,
        };
        validate_hls_name(&base.name)?;

        let additional = if self.encoding_source_id.is_some() {
            Vec::new()
        } else {
            self.parsed_hls_variants()?
        };
        let mut names = HashSet::new();

        names.insert(base.name.as_str());
        for stream in &additional {
            validate_hls_name(&stream.name)?;
            if !names.insert(stream.name.as_str()) {
                return Err(format!("duplicate HLS stream name {:?}", stream.name));
            }
        }

        let mut streams = Vec::with_capacity(additional.len() + 1);
        streams.push(base);
        streams.extend(additional);
        Ok(streams)
    }

    fn validate_hls_audio_options(&self) -> Result<(), String> {
        for variant in self.hls_streams()?.iter().skip(1) {
            ff_engine::validate_audio_options(
                &self.audio_codec,
                &self.audio_options,
                ENGINE_AUDIO_SAMPLE_RATE,
                variant.audio_bitrate,
            )
            .map_err(|error| format!("HLS variant {:?}: {error}", variant.name))?;
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.mode == OutputMode::Moq {
            let capabilities = ff_engine::ffmpeg_capabilities();
            let video = capabilities
                .encoders
                .iter()
                .find(|codec| codec.name == self.video_codec);
            let audio = capabilities
                .encoders
                .iter()
                .find(|codec| codec.name == self.audio_codec);
            if !video.is_some_and(|codec| super::moq_codecs::supports_video(&codec.codec_id))
                || !audio.is_some_and(|codec| super::moq_codecs::supports_audio(&codec.codec_id))
            {
                return Err(format!(
                    "MoQ does not support video encoder {:?} with audio encoder {:?}",
                    self.video_codec, self.audio_codec
                ));
            }
        }
        if self.width == 0 || self.height == 0 {
            return Err("output size must be greater than zero".to_string());
        }
        if !self.fps.is_finite() || self.fps < 1.0 || self.fps > f64::from(u32::MAX) {
            return Err("output fps must be a positive number".to_string());
        }
        if self
            .muxer_options
            .iter()
            .any(|(key, value)| key.trim().is_empty() || value.trim().is_empty())
        {
            return Err("muxer option names and values must not be empty".to_string());
        }
        // Also tolerate stale API payloads from before native MoQ publishing.
        if !matches!(self.mode, OutputMode::Stream | OutputMode::Moq)
            && !self.protocol_options.is_empty()
        {
            return Err(
                "protocol options can only be used with network stream outputs".to_string(),
            );
        }
        ff_engine::validate_output_metadata(&self.metadata_options)?;
        if self.mode == OutputMode::Desktop && !self.metadata_options.is_empty() {
            return Err("container metadata is unavailable for desktop output".to_string());
        }

        if matches!(self.mode, OutputMode::HLS | OutputMode::Stream) {
            let target = match self.mode {
                OutputMode::HLS => ff_engine::FfmpegOutputTarget::Hls,
                OutputMode::Stream => self
                    .stream_type
                    .ffmpeg_target()
                    .unwrap_or(ff_engine::FfmpegOutputTarget::Rtmp),
                OutputMode::Desktop | OutputMode::Moq => {
                    unreachable!("nonstandard output is handled separately")
                }
            };
            let capabilities = ff_engine::ffmpeg_capabilities();
            let custom_format = self.stream_format.trim();
            if self.mode == OutputMode::Stream && self.stream_type == StreamType::Custom {
                if custom_format.is_empty() {
                    return Err("custom stream format must not be empty".to_string());
                }
                if !capabilities.has_muxer_named(custom_format) {
                    return Err(format!(
                        "FFmpeg output format {custom_format:?} is not available"
                    ));
                }
            }
            let muxer_name = match self.mode {
                OutputMode::HLS => "hls",
                OutputMode::Stream => match self.stream_type {
                    StreamType::Rtmp => "flv",
                    StreamType::Srt | StreamType::Udp => "mpegts",
                    StreamType::Custom => custom_format,
                },
                OutputMode::Desktop | OutputMode::Moq => {
                    unreachable!("nonstandard output is handled separately")
                }
            };
            ff_engine::validate_muxer_options(muxer_name, &self.muxer_options)?;
            if self.mode == OutputMode::Stream {
                ff_engine::validate_output_protocol_options(
                    self.stream_type.engine_stream_type(),
                    &self.stream_url,
                    &self.protocol_options,
                )?;
            }
            let video_codecs =
                if self.mode == OutputMode::Stream && self.stream_type == StreamType::Custom {
                    capabilities.usable_codecs(ff_engine::FfmpegMediaType::Video)
                } else {
                    capabilities.video_codecs_for(target)
                };
            if !video_codecs
                .iter()
                .any(|codec| codec.name == self.video_codec)
            {
                return Err(format!(
                    "unsupported video codec {:?} for {} output",
                    self.video_codec, self.mode
                ));
            }
            let audio_codecs =
                if self.mode == OutputMode::Stream && self.stream_type == StreamType::Custom {
                    capabilities.usable_codecs(ff_engine::FfmpegMediaType::Audio)
                } else {
                    capabilities.audio_codecs_for(target)
                };
            if !audio_codecs
                .iter()
                .filter(|codec| !codec.hardware)
                .any(|codec| codec.name == self.audio_codec)
            {
                return Err(format!(
                    "unsupported audio codec {:?} for {} output",
                    self.audio_codec, self.mode
                ));
            }
        }

        if matches!(
            self.mode,
            OutputMode::HLS | OutputMode::Stream | OutputMode::Moq
        ) {
            ff_engine::validate_video_options(&self.video_codec, &self.video_options)?;
            ff_engine::validate_audio_options(
                &self.audio_codec,
                &self.audio_options,
                ENGINE_AUDIO_SAMPLE_RATE,
                u64::from(self.audio_bitrate) * 1_000,
            )?;
            if ff_engine::audio_codec_uses_bitrate(&self.audio_codec) && self.audio_bitrate == 0 {
                return Err("audio bitrate must be greater than zero".to_string());
            }
        }

        match self.mode {
            OutputMode::HLS => {
                if self.hls_segment_duration == 0 {
                    return Err("HLS segment duration must be greater than zero".to_string());
                }
                self.validate_hls_audio_options()?;
            }
            OutputMode::Stream if self.stream_url.trim().is_empty() => {
                return Err("stream output URL must not be empty".to_string());
            }
            _ => {}
        }

        Ok(())
    }

    fn video_maxrate(&self) -> u64 {
        self.video_options
            .get("maxrate")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(2_400)
            .saturating_mul(1_000)
    }
}

fn validate_hls_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("HLS stream name must not be empty".to_string());
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return Err(
            "HLS stream name may only contain ASCII letters, numbers, '_' and '-'".to_string(),
        );
    }
    if name == "master" {
        return Err("HLS stream name \"master\" is reserved".to_string());
    }
    Ok(())
}

pub fn string_to_log_level(l: String) -> Level {
    match l.to_lowercase().as_str() {
        "error" => Level::Error,
        "info" => Level::Info,
        "trace" => Level::Trace,
        "warning" => Level::Warn,
        _ => Level::Debug,
    }
}

impl PlayoutConfig {
    pub fn active_outputs(&self) -> impl Iterator<Item = &Output> {
        std::iter::once(&self.output)
            .chain(self.outputs.iter())
            .filter(|output| output.enabled)
    }

    pub fn output_source(&self, id: Option<i32>) -> Option<&Output> {
        std::iter::once(&self.output)
            .chain(self.outputs.iter())
            .find(|output| Some(output.id) == id)
    }

    /// Resolve a selected output/variant to the owning encoder, rejecting dependency cycles.
    pub fn encoding_source<'a>(
        &'a self,
        output: &'a Output,
    ) -> Result<(&'a Output, usize), String> {
        let mut current = output;
        let mut rendition = 0;
        let mut visited = HashSet::new();
        visited.insert(output.id);
        while let Some(id) = current.encoding_source_id {
            if id <= 0 || !visited.insert(id) {
                return Err(
                    "encoding sources must not contain a cycle or reference an unsaved output"
                        .into(),
                );
            }
            let source = std::iter::once(&self.output)
                .chain(self.outputs.iter())
                .find(|source| source.id == id)
                .ok_or("encoding source must be a configured output")?;
            if source.mode == OutputMode::Desktop || current.mode == OutputMode::Desktop {
                return Err("desktop output cannot provide or reuse encoded packets".into());
            }
            if source.mode == OutputMode::HLS {
                let names = if source.encoding_source_id.is_some() {
                    vec![source.hls_playlist_name.clone()]
                } else {
                    source
                        .hls_streams()?
                        .into_iter()
                        .map(|variant| variant.name)
                        .collect()
                };
                rendition = names
                    .iter()
                    .position(|name| *name == current.encoding_variant)
                    .ok_or("encoding source HLS variant is unavailable")?;
            } else if !current.encoding_variant.is_empty() {
                return Err("only HLS encoding sources have variants".into());
            }
            current = source;
        }

        Ok((current, rendition))
    }

    /// Apply inherited encoder settings while retaining the destination's transport and muxer.
    pub fn effective_output(&self, output: &Output) -> Result<Output, String> {
        if output.encoding_source_id.is_none() {
            let mut effective = output.clone();
            effective.video_options = output.encoder_video_options();

            return Ok(effective);
        }
        let (source, index) = self.encoding_source(output)?;
        let mut effective = output.clone();
        effective.width = source.width;
        effective.height = source.height;
        effective.inherit_resolution = source.inherit_resolution;
        effective.video_codec = source.video_codec.clone();
        effective.video_options = source.encoder_video_options();
        effective.audio_codec = source.audio_codec.clone();
        effective.audio_options = source.audio_options.clone();
        effective.audio_bitrate = source.audio_bitrate;
        effective.hls_variants.clear();
        if source.mode == OutputMode::HLS {
            let variants = source.hls_streams()?;
            let variant = variants
                .get(index)
                .ok_or("encoding rendition is unavailable")?;
            effective.width = variant.width;
            effective.height = variant.height;
            effective.inherit_resolution = false;
            effective
                .video_options
                .insert("maxrate".into(), (variant.video_bitrate / 1000).to_string());
            effective.audio_bitrate = u32::try_from(variant.audio_bitrate / 1000)
                .map_err(|_| "encoding source audio bitrate is out of range")?;
        }

        Ok(effective)
    }

    pub fn validate_outputs(&self) -> Result<(), String> {
        if self.outputs.len() >= 8 {
            return Err("at most eight outputs are supported".into());
        }
        if let Some(program) = &self.program
            && (program.width == 0
                || program.height == 0
                || !program.fps.is_finite()
                || program.fps < 1.0
                || program.fps > f64::from(u32::MAX))
        {
            return Err("invalid program format".into());
        }
        let mut ids = HashSet::new();
        let mut hls_outputs = 0;
        let mut desktops = 0;
        let mut moq = 0;

        for output in std::iter::once(&self.output).chain(self.outputs.iter()) {
            if output.label.chars().count() > 128 {
                return Err("output name is too long".into());
            }
            if output.id < 0 || (output.id != 0 && !ids.insert(output.id)) {
                return Err("duplicate or invalid output ID".into());
            }
            if output.fps
                != self
                    .program
                    .as_ref()
                    .map_or(self.output.fps, |program| program.fps)
            {
                return Err("parallel outputs must use the same frame rate".into());
            }
            if !output.enabled {
                continue;
            }
            let effective = self.effective_output(output)?;
            effective.validate()?;
            if output.mode == OutputMode::HLS {
                hls_outputs += 1;
                if hls_outputs > 1 {
                    return Err("only one HLS output is supported per channel".into());
                }
            }
            desktops += usize::from(output.mode == OutputMode::Desktop);
            moq += usize::from(output.mode == OutputMode::Moq);
        }
        if desktops > 1 || moq > 1 {
            return Err("at most one desktop and one MoQ output per channel are supported".into());
        }

        Ok(())
    }

    pub async fn new(
        pool: &Pool<Sqlite>,
        channel_id: i32,
        output_id: Option<i32>,
    ) -> Result<Self, ServiceError> {
        let global = handles::select_global(pool).await?;
        let channel = handles::select_channel(pool, &channel_id).await?;
        let mut config = handles::select_configuration(pool, channel_id).await?;
        let recording_config = handles::select_recording(pool, channel_id).await?;
        let outputs = handles::select_outputs(pool, channel_id).await?;

        if let Some(id) = output_id {
            config.output_id = id;
        }

        let channel = Channel::new(&global, channel);
        let text_preset = match config.text_preset_id {
            Some(id) => Some(handles::select_preset(pool, channel_id, id).await?),
            None => None,
        };
        let general = General::new(&config);
        let mail = Mail::new(&global, &config);
        let notification = Notification::new(&global, &config);
        let logging = Logging::new(&config);
        let mut processing = Processing::new(&config);
        let audio = Audio::new(&config);
        let listeners = handles::select_live_inputs(pool, config.id).await?;
        let ingest = Ingest { listeners };
        let mut playlist = Playlist::new(&config);
        let text = Text::new(&config, text_preset);
        let task = Task::new(&config);
        let recording = Recording::new(&recording_config);
        let additional_outputs = if output_id.is_some() {
            Vec::new()
        } else {
            outputs
                .iter()
                .filter(|output| {
                    (output.active || output.configured) && output.id != config.output_id
                })
                .cloned()
                .map(Output::from_record)
                .collect()
        };
        let output = Output::new(&config, outputs);
        let program = handles::output::select_program_format(pool, config.id).await?;
        let mut storage = Storage::new(&config, channel.storage.clone(), channel.shared);

        if !channel.playlists.is_dir() {
            fs::create_dir_all(&channel.playlists).await?;
        }

        if !channel.logs.is_dir() {
            fs::create_dir_all(&channel.logs).await?;
        }

        let (filler_path, _, filler) = norm_abs_path(&channel.storage, &config.storage_filler)?;

        storage.filler = filler;
        storage.filler_path = filler_path;

        playlist.start_sec = Some(time_to_sec(&playlist.day_start, &channel.timezone));

        if playlist.length.contains(':') {
            playlist.length_sec = Some(time_to_sec(&playlist.length, &channel.timezone));
        } else {
            playlist.length_sec = Some(86400.0);
        }

        let (logo_path, _, logo) = norm_abs_path(&channel.storage, &processing.logo)?;

        if processing.add_logo && !logo_path.is_file() {
            processing.add_logo = false;
        }

        processing.logo = logo;
        processing.logo_path = logo_path.to_string_lossy().to_string();

        Ok(Self {
            channel,
            general,
            mail,
            notification,
            logging,
            processing,
            audio,
            ingest,
            playlist,
            storage,
            text,
            task,
            recording,
            output,
            outputs: additional_outputs,
            program,
        })
    }

    pub async fn dump(pool: &Pool<Sqlite>, id: i32) -> Result<(), ServiceError> {
        let config = Self::new(pool, id, None).await?;

        let toml_string = toml_edit::ser::to_string_pretty(&config)?;
        tokio::fs::write(&format!("ffplayout_{id}.toml"), toml_string).await?;

        Ok(())
    }

    pub async fn import(pool: &Pool<Sqlite>, id: i32, path: &Path) -> Result<(), ServiceError> {
        if path.is_file() {
            let mut file = tokio::fs::File::open(path).await?;
            let mut contents = String::new();
            file.read_to_string(&mut contents).await?;

            let config: PlayoutConfig = toml_edit::de::from_str(&contents).unwrap();

            handles::update_configuration(pool, id, config).await?;
        } else {
            return Err(ServiceError::BadRequest("Path not exists!".to_string()));
        }

        Ok(())
    }
}

/// Read command line arguments, and override the config with them.
pub async fn get_config(
    pool: &Pool<Sqlite>,
    channel_id: i32,
) -> Result<PlayoutConfig, ServiceError> {
    get_config_with_args(pool, channel_id, ARGS.clone()).await
}

async fn get_config_with_args(
    pool: &Pool<Sqlite>,
    channel_id: i32,
    args: super::args_parse::Args,
) -> Result<PlayoutConfig, ServiceError> {
    // Keep the full source graph until a CLI selection's effective encoding
    // has been resolved. The override changes this run, never persisted rows.
    let mut config = PlayoutConfig::new(pool, channel_id, None).await?;

    if let Some(selection) = args.output {
        let selected = if selection == super::args_parse::CliOutput::Null {
            config.output.clone()
        } else {
            let outputs = handles::select_outputs(pool, channel_id).await?;
            let mode = selection.mode().to_string();
            let matching = || outputs.iter().filter(|output| output.name == mode);
            let record = matching()
                .find(|output| output.active)
                .or_else(|| matching().find(|output| output.configured))
                .or_else(|| matching().next());

            record
                .cloned()
                .map(Output::from_record)
                .unwrap_or_else(|| config.output.clone())
        };
        let mut output = config
            .effective_output(&selected)
            .map_err(ServiceError::Conflict)?;
        output.enabled = true;

        if let Some(program) = &config.program {
            output.fps = program.fps;

            if output.inherit_resolution {
                output.width = program.width;
                output.height = program.height;
            }
        }

        output.encoding_source_id = None;
        output.encoding_variant.clear();
        output.share_encoding = false;
        output.mode = selection.mode();
        output.discard = selection == super::args_parse::CliOutput::Null;
        config.output = output;
        config.outputs.clear();
    }

    config.general.generate = args.generate;
    config.general.validate = args.validate;
    config.general.skip_validation = args.skip_validation;

    if let Some(template_file) = args.template {
        let mut f = fs::File::options()
            .read(true)
            .write(false)
            .open(template_file)
            .await?;
        let mut buffer = Vec::new();
        f.read_to_end(&mut buffer).await?;

        let mut template: Template = serde_json::from_slice(&buffer)?;

        template.sources.sort_by_key(|d1| d1.start);

        config.general.template = Some(template);
    }

    if let Some(paths) = args.paths {
        config.storage.paths = paths;
    }

    if let Some(storage) = args.storage {
        config.channel.storage = PathBuf::from(&storage);
        config.storage.path = PathBuf::from(&storage);
    }

    if let Some(log) = args.logs {
        config.channel.logs = PathBuf::from(&log);
    }

    if let Some(playlist) = args.playlists {
        config.channel.playlists = PathBuf::from(&playlist);
    }

    if let Some(public) = args.public {
        config.channel.public = PathBuf::from(&public);
    }

    if let Some(folder) = args.folder {
        config.channel.storage = folder;
        config.processing.mode = ProcessMode::Folder;
    }

    if let Some(start) = args.start {
        config.playlist.day_start.clone_from(&start);
        config.playlist.start_sec = Some(time_to_sec(&start, &config.channel.timezone));
    }

    if let Some(volume) = args.volume {
        config.audio.volume = volume;
    }

    if let Some(smtp_server) = args.smtp_server {
        config.mail.smtp_server = smtp_server;
    }

    if let Some(smtp_user) = args.smtp_user {
        config.mail.smtp_user = smtp_user;
    }

    if let Some(smtp_password) = args.smtp_password {
        config.mail.smtp_password = smtp_password;
    }

    if args.smtp_starttls.is_some_and(|v| &v == "true") {
        config.mail.smtp_starttls = true;
    }

    if let Some(smtp_port) = args.smtp_port {
        config.mail.smtp_port = smtp_port;
    }

    Ok(config)
}

#[cfg(test)]
mod output_tests {
    use std::collections::BTreeMap;

    use super::{Output, OutputMode, PlayoutConfig, StreamType};

    fn output(mode: OutputMode) -> Output {
        Output {
            id: 1,
            label: String::new(),
            enabled: true,
            inherit_resolution: false,
            share_encoding: true,
            encoding_source_id: None,
            encoding_variant: String::new(),
            public_moq: false,
            mode,
            discard: false,
            stream_url: "rtmp://localhost/live/test".to_string(),
            stream_type: StreamType::Rtmp,
            stream_format: String::new(),
            hls_playlist_name: "stream".to_string(),
            hls_segment_duration: 6,
            hls_list_size: 600,
            desktop_fullscreen: false,
            width: 1280,
            height: 720,
            fps: 25.0,
            video_codec: "libx264".to_string(),
            video_options: ff_engine::video_option_defaults("libx264"),
            protocol_options: BTreeMap::new(),
            muxer_options: BTreeMap::new(),
            metadata_options: BTreeMap::new(),
            audio_codec: "aac".to_string(),
            audio_options: BTreeMap::new(),
            audio_bitrate: 128,
            hls_variants: Vec::new(),
        }
    }

    async fn cli_pool() -> sqlx::SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::db::handles::db_migrate(&pool).await.unwrap();
        let dir =
            std::env::temp_dir().join(format!("ffplayout-cli-config-{}", uuid::Uuid::new_v4()));
        sqlx::query("UPDATE config_global SET public=$1, logs=$1, playlists=$1, storage=$1; UPDATE channels SET public=$1, playlists=$1, storage=$1; UPDATE config_storage SET filler='';")
            .bind(dir.to_string_lossy()).execute(&pool).await.unwrap();

        pool
    }

    async fn cli_config(
        pool: &sqlx::SqlitePool,
        selection: super::super::args_parse::CliOutput,
    ) -> PlayoutConfig {
        super::get_config_with_args(
            pool,
            1,
            super::super::args_parse::Args {
                output: Some(selection),
                ..Default::default()
            },
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn cli_activates_a_disabled_configured_output_without_writing_it() {
        use super::super::args_parse::CliOutput;
        let pool = cli_pool().await;
        sqlx::query("UPDATE config_output SET active=(name='hls'), configured=1; UPDATE config_output SET width=640,height=360,inherit_resolution=1 WHERE name='stream'; UPDATE config SET program_format='{\"width\":1920,\"height\":1080,\"fps\":30.0}';")
            .execute(&pool).await.unwrap();
        let before =
            serde_json::to_value(crate::db::handles::select_outputs(&pool, 1).await.unwrap())
                .unwrap();
        let runtime = cli_config(&pool, CliOutput::Stream).await;
        runtime.validate_outputs().unwrap();
        assert!(runtime.output.enabled);
        assert!(runtime.outputs.is_empty());
        assert_eq!(
            (
                runtime.output.width,
                runtime.output.height,
                runtime.output.fps
            ),
            (1920, 1080, 30.0)
        );
        assert_eq!(
            before,
            serde_json::to_value(crate::db::handles::select_outputs(&pool, 1).await.unwrap())
                .unwrap()
        );
    }

    #[tokio::test]
    async fn cli_resolves_shared_hls_variant_chains_before_removing_other_outputs() {
        use super::super::args_parse::CliOutput;
        let pool = cli_pool().await;
        let records = crate::db::handles::select_outputs(&pool, 1).await.unwrap();
        let hls = records
            .iter()
            .find(|output| output.name == "hls")
            .unwrap()
            .id;
        let stream = records
            .iter()
            .find(|output| output.name == "stream")
            .unwrap()
            .id;
        sqlx::query("UPDATE config_output SET active=1,configured=1,share_encoding=0, video_codec='libx264',audio_codec='aac' WHERE id IN ($1,$2)")
            .bind(hls).bind(stream).execute(&pool).await.unwrap();
        sqlx::query("UPDATE config_output SET hls_playlist_name='base',hls_variants='small:320x180:450k:64k' WHERE id=$1")
            .bind(hls).execute(&pool).await.unwrap();
        // A derived HLS output forwards the original small rendition under its own name.
        let mut derived = records
            .iter()
            .find(|output| output.name == "hls")
            .unwrap()
            .clone();
        derived.hls_playlist_name = Some("derived".into());
        let derived_id = crate::db::handles::insert_output(&pool, 1, &derived, true)
            .await
            .unwrap();
        sqlx::query("UPDATE config_output SET configured=1,share_encoding=1,encoding_source_id=$1,encoding_variant='small' WHERE id=$2")
            .bind(hls).bind(derived_id).execute(&pool).await.unwrap();
        sqlx::query("UPDATE config_output SET priority=0,share_encoding=1,encoding_source_id=$1,encoding_variant='derived' WHERE id=$2")
            .bind(derived_id).bind(stream).execute(&pool).await.unwrap();
        sqlx::query("UPDATE config_output SET priority=1 WHERE id!=$1")
            .bind(stream)
            .execute(&pool)
            .await
            .unwrap();
        let before =
            serde_json::to_value(crate::db::handles::select_outputs(&pool, 1).await.unwrap())
                .unwrap();
        for selection in [CliOutput::Stream, CliOutput::Null] {
            let runtime = cli_config(&pool, selection).await;
            assert_eq!(runtime.output.id, stream);
            assert!(runtime.outputs.is_empty());
            assert_eq!((runtime.output.width, runtime.output.height), (320, 180));
            assert_eq!(runtime.output.audio_bitrate, 64);
            assert_eq!(runtime.output.video_options["maxrate"], "450");
            assert!(!runtime.output.share_encoding);
            assert_eq!(runtime.output.encoding_source_id, None);
            assert!(runtime.output.encoding_variant.is_empty());
            assert!(runtime.output.hls_variants.is_empty());
            if selection == CliOutput::Null {
                runtime.output.for_playout().validate().unwrap();
            } else {
                runtime.validate_outputs().unwrap();
            }
        }
        assert_eq!(
            before,
            serde_json::to_value(crate::db::handles::select_outputs(&pool, 1).await.unwrap())
                .unwrap()
        );
    }

    #[tokio::test]
    async fn cli_prefers_active_then_configured_outputs_over_legacy_presets() {
        use super::super::args_parse::CliOutput;
        let pool = cli_pool().await;
        let preset = crate::db::handles::select_outputs(&pool, 1)
            .await
            .unwrap()
            .into_iter()
            .find(|output| output.name == "stream")
            .unwrap();
        let configured = crate::db::handles::insert_output(&pool, 1, &preset, false)
            .await
            .unwrap();
        let active = crate::db::handles::insert_output(&pool, 1, &preset, true)
            .await
            .unwrap();
        sqlx::query("UPDATE config_output SET configured=1 WHERE id IN ($1,$2)")
            .bind(configured)
            .bind(active)
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(cli_config(&pool, CliOutput::Stream).await.output.id, active);
        sqlx::query("UPDATE config_output SET active=0 WHERE id=$1")
            .bind(active)
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            cli_config(&pool, CliOutput::Stream).await.output.id,
            configured
        );
        sqlx::query("UPDATE config_output SET configured=0 WHERE id IN ($1,$2)")
            .bind(configured)
            .bind(active)
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            cli_config(&pool, CliOutput::Stream).await.output.id,
            preset.id
        );
    }

    #[tokio::test]
    async fn null_override_does_not_persist_or_replace_the_stored_output() {
        use super::super::args_parse::{Args, CliOutput};
        use crate::db::handles;
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        handles::db_migrate(&pool).await.unwrap();
        let dir =
            std::env::temp_dir().join(format!("ffplayout-null-config-{}", std::process::id()));
        let storage = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests_assets/storage")
            .canonicalize()
            .unwrap();
        sqlx::query("UPDATE config_global SET public=$1, logs=$1, playlists=$1, storage=$2; UPDATE channels SET public=$1, playlists=$1, storage=$2; UPDATE config_output SET active=(name IN ('hls', 'stream'));")
            .bind(dir.to_string_lossy()).bind(storage.to_string_lossy()).execute(&pool).await.unwrap();
        let before = super::get_config_with_args(&pool, 1, Args::default())
            .await
            .unwrap();
        let runtime = super::get_config_with_args(
            &pool,
            1,
            Args {
                output: Some(CliOutput::Null),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(!before.outputs.is_empty());
        assert!(runtime.outputs.is_empty());
        assert!(runtime.output.discard);
        assert_eq!(runtime.output.mode, OutputMode::Stream);
        assert_eq!(runtime.output.stream_type, StreamType::Rtmp);
        assert_eq!(runtime.output.for_playout().stream_format, "null");
        runtime.output.for_playout().validate().unwrap();
        assert!(
            serde_json::to_value(&runtime.output)
                .unwrap()
                .get("discard")
                .is_none()
        );
        let after = super::get_config_with_args(&pool, 1, Args::default())
            .await
            .unwrap();
        assert_eq!(before.outputs.len(), after.outputs.len());
        assert!(!after.output.discard);
        assert_eq!(after.output.mode, OutputMode::HLS);
        assert_eq!(
            serde_json::to_value(&before.output).unwrap(),
            serde_json::to_value(&after.output).unwrap()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn moq_reuses_a_named_hls_variant_and_rejects_unavailable_sources() {
        let mut hls = output(OutputMode::HLS);
        hls.id = 42;
        hls.hls_variants = vec!["small:640x360:800k:96k".into()];
        let mut moq = output(OutputMode::Moq);
        moq.id = 43;
        moq.encoding_source_id = Some(42);
        moq.encoding_variant = "small".into();
        let mut config = PlayoutConfig {
            output: hls,
            outputs: vec![moq],
            ..Default::default()
        };
        assert!(config.validate_outputs().is_ok());
        assert_eq!(config.encoding_source(&config.outputs[0]).unwrap().1, 1);
        config.outputs[0].encoding_variant = "missing".into();
        assert!(config.validate_outputs().is_err());
        config.outputs[0].encoding_variant = "small".into();
        config.output.audio_codec = "libopus".into();
        assert!(config.validate_outputs().is_err());
        config.output.audio_codec = "aac".into();
        config.output.enabled = false;
        assert!(config.encoding_source(&config.outputs[0]).is_ok());
    }

    #[test]
    fn encoding_sources_preserve_disabled_owners_and_reject_cycles_and_desktop() {
        let mut root = output(OutputMode::HLS);
        root.id = 11;
        root.hls_variants = vec!["small:640x360:800k:96k".into()];
        let mut stream = output(OutputMode::Stream);
        stream.id = 12;
        stream.encoding_source_id = Some(11);
        stream.encoding_variant = "small".into();
        let mut hls = output(OutputMode::Stream);
        hls.id = 13;
        hls.encoding_source_id = Some(12);
        let mut config = PlayoutConfig {
            output: root,
            outputs: vec![stream, hls],
            ..Default::default()
        };
        assert!(config.validate_outputs().is_ok());
        let (owner, index) = config.encoding_source(&config.outputs[1]).unwrap();
        assert_eq!((owner.id, index), (11, 1));
        let effective = config.effective_output(&config.outputs[1]).unwrap();
        assert_eq!(
            (effective.width, effective.height, effective.audio_bitrate),
            (640, 360, 96)
        );
        config.output.encoding_source_id = Some(13);
        config.output.encoding_variant = "small".into();
        assert!(config.validate_outputs().is_err());
        config.output.encoding_source_id = None;
        config.outputs[0].enabled = false;
        assert!(config.validate_outputs().is_ok());
        config.outputs[0].enabled = true;
        config.output.mode = OutputMode::Desktop;
        assert!(config.validate_outputs().is_err());
        config.output.mode = OutputMode::HLS;
        config.outputs[0].encoding_source_id = Some(12);
        assert!(config.validate_outputs().is_err());
    }

    #[test]
    fn standalone_moq_requires_compatible_codecs() {
        let mut config = PlayoutConfig {
            output: output(OutputMode::Moq),
            ..Default::default()
        };
        assert!(config.validate_outputs().is_ok());
        config.output.audio_codec = "libopus".into();
        assert!(config.validate_outputs().is_err());
    }

    #[test]
    fn parallel_outputs_validate_ids_frame_rates_and_destination_collisions() {
        let mut config = PlayoutConfig {
            output: output(OutputMode::Stream),
            ..Default::default()
        };
        config.output.id = 1;
        let mut additional = output(OutputMode::Stream);
        additional.id = 2;
        config.outputs.push(additional);
        assert!(config.validate_outputs().is_ok());
        config.outputs[0].id = 1;
        assert!(config.validate_outputs().is_err());
        config.outputs[0].id = 2;
        config.outputs[0].fps = 30.0;
        assert!(config.validate_outputs().is_err());
        config.outputs[0].fps = config.output.fps;
        config.output.mode = OutputMode::Desktop;
        config.outputs[0].mode = OutputMode::Desktop;
        assert!(config.validate_outputs().is_err());
    }

    #[test]
    fn only_one_enabled_hls_output_is_allowed() {
        let mut config = PlayoutConfig {
            output: output(OutputMode::HLS),
            ..Default::default()
        };
        let mut additional = output(OutputMode::HLS);
        additional.id = 2;
        additional.hls_playlist_name = "secondary".into();
        config.outputs.push(additional);
        assert!(config.validate_outputs().is_err());

        config.outputs[0].enabled = false;
        assert!(config.validate_outputs().is_ok());
    }

    #[test]
    fn validates_structured_output_settings() {
        assert!(output(OutputMode::HLS).validate().is_ok());
        assert!(output(OutputMode::Stream).validate().is_ok());
    }

    #[test]
    fn output_metadata_roundtrips_and_is_rejected_for_desktop() {
        let mut output = output(OutputMode::Stream);
        output
            .metadata_options
            .insert("copyright".into(), "© Example".into());
        assert!(output.validate().is_ok());
        let decoded: Output =
            serde_json::from_value(serde_json::to_value(&output).unwrap()).unwrap();
        assert_eq!(decoded.metadata_options, output.metadata_options);

        output.mode = OutputMode::Desktop;
        assert!(output.validate().unwrap_err().contains("desktop"));
    }

    #[test]
    fn validates_output_protocol_options_before_saving() {
        let mut output = output(OutputMode::Stream);
        output.stream_type = StreamType::Srt;
        output.stream_url = "srt://localhost:9000".to_string();
        output
            .protocol_options
            .insert("latency".to_string(), "2000000".to_string());
        assert!(output.validate().is_ok());

        output
            .protocol_options
            .insert("rw_timeout".to_string(), "0".to_string());
        assert!(
            output
                .validate()
                .unwrap_err()
                .contains("managed by ffplayout")
        );
        output.protocol_options.remove("rw_timeout");
        output.stream_url = "srt://localhost:9000?latency=20000".to_string();
        assert!(
            output
                .validate()
                .unwrap_err()
                .contains("conflicts with the output URL")
        );
        output.stream_url = "srt://localhost:9000?latency=2000000".to_string();
        assert!(output.validate().is_ok());
    }

    #[test]
    fn protocol_options_roundtrip_through_the_configuration_api_shape() {
        let mut output = output(OutputMode::Stream);
        output.stream_type = StreamType::Srt;
        output.stream_url = "srt://localhost:9000".to_string();
        output.protocol_options = BTreeMap::from([
            ("latency".to_string(), "2000000".to_string()),
            (
                "passphrase".to_string(),
                "a sufficiently long secret".to_string(),
            ),
        ]);

        let json = serde_json::to_value(&output).unwrap();
        let decoded: Output = serde_json::from_value(json).unwrap();

        assert_eq!(decoded.protocol_options, output.protocol_options);
    }

    #[test]
    fn rejects_protocol_options_for_non_stream_outputs() {
        for mode in [OutputMode::HLS, OutputMode::Desktop] {
            let mut output = output(mode);
            output
                .protocol_options
                .insert("pkt_size".to_string(), "1316".to_string());
            assert!(
                output
                    .validate()
                    .unwrap_err()
                    .contains("only be used with network stream outputs")
            );
        }
    }

    #[test]
    fn legacy_moq_protocol_options_are_retired_without_blocking_validation() {
        let mut record = crate::db::models::Output::new(1, OutputMode::Moq);
        record.protocol_options = r#"{"tcp_nodelay":"1"}"#.into();
        let mut output = Output::from_record(record);
        assert!(output.protocol_options.is_empty());
        assert!(output.validate().is_ok());

        // A client that still holds the old configuration can save it as well.
        output
            .protocol_options
            .insert("tcp_nodelay".into(), "1".into());
        assert!(output.validate().is_ok());
        output.width = 0;
        assert!(output.validate().is_err());
    }

    #[test]
    fn moq_owns_low_latency_tuning_and_shared_outputs_keep_the_owner_settings() {
        let mut moq = output(OutputMode::Moq);
        moq.id = 1;
        moq.video_options.insert("tune".into(), "none".into());
        let mut hls = output(OutputMode::HLS);
        hls.id = 2;
        hls.video_options.insert("tune".into(), "none".into());
        let mut config = PlayoutConfig {
            output: moq.clone(),
            outputs: vec![hls.clone()],
            ..Default::default()
        };
        assert_eq!(
            config.effective_output(&moq).unwrap().video_options["tune"],
            "zerolatency"
        );
        moq.encoding_source_id = Some(2);
        moq.encoding_variant = hls.hls_playlist_name.clone();
        assert_eq!(
            config.effective_output(&moq).unwrap().video_options["tune"],
            "none"
        );
        hls.encoding_source_id = Some(1);
        config.outputs = vec![hls.clone()];
        assert_eq!(
            config.effective_output(&hls).unwrap().video_options["tune"],
            "zerolatency"
        );
    }

    #[test]
    fn audio_validation_checks_output_and_hls_variant_bitrates() {
        let mut output = output(OutputMode::Stream);
        output.stream_type = StreamType::Srt;
        output.stream_url = "srt://localhost:9000".to_string();
        output.audio_codec = "mp2".to_string();
        output
            .audio_options
            .insert("cutoff".to_string(), "0".to_string());
        output.validate().unwrap();
        output.audio_bitrate = 129;
        assert!(
            output
                .validate()
                .unwrap_err()
                .contains("audio encoder options")
        );
        output.audio_bitrate = 128;
        // Exercise rendition validation directly: FFmpeg builds differ in
        // whether the HLS muxer advertises MP2 support.
        output.mode = OutputMode::HLS;
        output.hls_variants = vec!["low:640x360:1000k:129k".to_string()];
        assert!(
            output
                .validate_hls_audio_options()
                .unwrap_err()
                .contains("HLS variant")
        );
        output.hls_variants = vec!["low:640x360:1000k:96k".to_string()];
        assert!(output.validate_hls_audio_options().is_ok());
    }

    #[test]
    fn validates_audio_encoder_options_before_saving() {
        let mut output = output(OutputMode::HLS);
        output
            .audio_options
            .insert("aac_coder".to_string(), "fast".to_string());
        assert!(output.validate().is_ok());

        output
            .audio_options
            .insert("aac_codre".to_string(), "fast".to_string());
        assert!(
            output
                .validate()
                .unwrap_err()
                .contains("unsupported audio option")
        );
    }

    #[test]
    fn custom_stream_requires_an_output_format() {
        let mut output = output(OutputMode::Stream);
        output.stream_type = StreamType::Custom;

        assert_eq!(
            output.validate(),
            Err("custom stream format must not be empty".to_string())
        );
    }

    #[test]
    fn custom_stream_accepts_an_available_muxer() {
        let mut output = output(OutputMode::Stream);
        output.stream_type = StreamType::Custom;
        output.stream_format = "matroska".to_string();

        assert!(output.validate().is_ok());
    }

    #[test]
    fn custom_pcm_output_does_not_require_an_audio_bitrate() {
        let mut output = output(OutputMode::Stream);
        output.stream_type = StreamType::Custom;
        output.stream_format = "matroska".to_string();
        output.audio_codec = "pcm_s16le".to_string();
        output.audio_bitrate = 0;

        assert!(output.validate().is_ok());
    }

    #[test]
    fn rejects_zero_hls_segment_duration() {
        let mut output = output(OutputMode::HLS);
        output.hls_segment_duration = 0;
        assert_eq!(
            output.validate().unwrap_err(),
            "HLS segment duration must be greater than zero"
        );
    }

    #[test]
    fn rejects_hls_options_that_conflict_with_segment_management() {
        let mut config = output(OutputMode::HLS);
        config
            .muxer_options
            .insert("hls_time".to_string(), "30".to_string());

        assert!(config.validate().is_err());
    }

    #[test]
    fn rejects_unknown_muxer_options_before_saving() {
        let mut config = output(OutputMode::HLS);
        config
            .muxer_options
            .insert("hls_flgs".to_string(), "program_date_time".to_string());

        assert!(config.validate().is_err());
    }

    #[test]
    fn accepts_compatible_hls_flags() {
        let mut config = output(OutputMode::HLS);
        config.muxer_options.insert(
            "hls_flags".to_string(),
            "program_date_time+independent_segments".to_string(),
        );

        assert!(config.validate().is_ok());
    }

    #[test]
    fn rejects_invalid_hls_variant() {
        let mut output = output(OutputMode::HLS);
        output.hls_variants = vec!["invalid".to_string()];
        assert!(
            output
                .validate()
                .unwrap_err()
                .contains("invalid HLS variant")
        );
    }

    #[test]
    fn hls_variants_are_added_after_base_output() {
        let mut output = output(OutputMode::HLS);
        output.hls_variants = vec!["low:640x360:800k:96k".to_string()];
        let streams = output.hls_streams().unwrap();

        assert_eq!(streams.len(), 2);
        assert_eq!(streams[0].name, "stream");
        assert_eq!(streams[0].width, 1280);
        assert_eq!(streams[1].name, "low");
    }

    #[test]
    fn standalone_hls_output_uses_base_stream() {
        let output = output(OutputMode::HLS);
        let streams = output.hls_streams().unwrap();

        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].name, "stream");
        assert_eq!(streams[0].width, 1280);
    }

    #[test]
    fn rejects_variant_with_same_name_as_base_output() {
        let mut output = output(OutputMode::HLS);
        output.hls_variants = vec!["stream:640x360:800k".to_string()];
        assert!(
            output
                .validate()
                .unwrap_err()
                .contains("duplicate HLS stream name")
        );
    }

    #[test]
    fn rejects_invalid_quality_for_visible_setting() {
        let mut output = output(OutputMode::Stream);
        output
            .video_options
            .insert("quality".to_string(), "52".to_string());
        assert!(output.validate().unwrap_err().contains("quality"));
    }

    #[test]
    fn cbr_does_not_validate_unused_quality() {
        let mut output = output(OutputMode::Stream);
        output
            .video_options
            .insert("rate_control".to_string(), "cbr".to_string());
        output
            .video_options
            .insert("quality".to_string(), "52".to_string());
        assert!(output.validate().is_ok());
    }
}

#[cfg(test)]
mod ingest_tests {
    use super::{Ingest, LiveInput, MIN_INGEST_PORT, parse_rtmp_ingest_port};

    #[test]
    fn ingest_api_uses_only_the_listener_list() {
        let json = serde_json::to_value(Ingest::default()).unwrap();

        assert_eq!(json, serde_json::json!({ "listeners": [] }));
        assert!(
            serde_json::from_value::<Ingest>(serde_json::json!({
                "enable": true,
                "ingest_url": "rtmp://127.0.0.1:1936/live/stream"
            }))
            .is_err()
        );
    }

    fn srt_input(url: &str) -> LiveInput {
        LiveInput {
            backend: "srt".to_string(),
            identifier: url.to_string(),
            ..LiveInput::default()
        }
    }

    #[test]
    fn validates_srt_listener_addresses_without_url_options() {
        assert_eq!(srt_input("srt://127.0.0.1:9000").listen_port(), Ok(9000));
        assert_eq!(srt_input("srt://[::1]:9001").listen_port(), Ok(9001));

        for url in [
            "srt://127.0.0.1:80",
            "srt://127.0.0.1",
            "srt://127.0.0.1:9000?mode=caller",
            "srt://127.0.0.1:9000/live",
            "srt://user@127.0.0.1:9000",
            "http://127.0.0.1:9000",
        ] {
            assert!(srt_input(url).listen_port().is_err(), "accepted {url}");
        }
    }

    #[test]
    fn parses_unprivileged_rtmp_ingest_ports() {
        assert_eq!(
            parse_rtmp_ingest_port("rtmp://127.0.0.1:1936/live/stream"),
            Ok(1936)
        );
        assert_eq!(
            parse_rtmp_ingest_port("rtmp://[::1]:1940/live/stream"),
            Ok(1940)
        );
    }

    #[test]
    fn rejects_privileged_or_invalid_rtmp_ingest_urls() {
        assert!(parse_rtmp_ingest_port("rtmp://127.0.0.1:1023/live/stream").is_err());
        assert!(parse_rtmp_ingest_port("rtmp://127.0.0.1/live/stream").is_err());
        assert!(parse_rtmp_ingest_port("http://127.0.0.1:1936/live/stream").is_err());
        assert!(parse_rtmp_ingest_port("rtmp://:1936/live/stream").is_err());
        const { assert!(MIN_INGEST_PORT > 0) };
    }
}

#[cfg(test)]
mod recording_tests {
    use std::collections::BTreeMap;

    use super::{EncodingSource, Recording};

    #[test]
    fn recording_requires_a_path_and_valid_segment_duration() {
        let mut recording = Recording {
            enable: true,
            path: String::new(),
            ..Default::default()
        };
        assert!(recording.validate().is_err());

        recording.path = "/var/lib/ffplayout/recordings/1".to_string();
        recording.segment_duration = 29;
        assert!(recording.validate().is_err());

        recording.segment_duration = 300;
        recording.source_output_id = Some(1);
        assert!(recording.validate().is_ok());
    }

    #[test]
    fn recording_rejects_a_relative_or_system_path() {
        let mut recording = Recording {
            enable: true,
            path: "recordings".to_string(),
            segment_duration: 300,
            source_output_id: Some(1),
            ..Default::default()
        };
        assert!(recording.validate().is_err());

        recording.path = "/etc".to_string();
        assert!(recording.validate().is_err());
    }

    #[test]
    fn hls_variant_recording_requires_a_variant_name() {
        let recording = Recording {
            enable: true,
            source: EncodingSource::HlsVariant,
            source_output_id: Some(1),
            path: "/var/lib/ffplayout/recordings/1".to_string(),
            segment_duration: 300,
            ..Recording::default()
        };
        assert!(recording.validate().is_err());
    }

    #[test]
    fn recording_audio_validation_uses_recording_bitrate() {
        let mut recording = Recording {
            enable: true,
            source: EncodingSource::Encode,
            path: "/var/lib/ffplayout/recordings/1".to_string(),
            segment_duration: 300,
            video_codec: "libx264".to_string(),
            video_options: ff_engine::video_option_defaults("libx264"),
            audio_codec: "mp2".to_string(),
            audio_bitrate: 128,
            audio_options: BTreeMap::from([("cutoff".to_string(), "0".to_string())]),
            ..Recording::default()
        };
        assert!(recording.validate().is_ok());
        recording.audio_bitrate = 129;
        assert!(
            recording
                .validate()
                .unwrap_err()
                .contains("audio encoder options")
        );
    }

    #[test]
    fn recording_rejects_invalid_audio_encoder_options() {
        let recording = Recording {
            enable: true,
            source: EncodingSource::Encode,
            source_output_id: None,
            variant: String::new(),
            path: "/var/lib/ffplayout/recordings/1".to_string(),
            segment_duration: 300,
            retention_days: 0,
            minimum_free_space_gb: 0,
            width: 0,
            height: 0,
            video_codec: "libx264".to_string(),
            video_options: ff_engine::video_option_defaults("libx264"),
            audio_codec: "aac".to_string(),
            audio_options: BTreeMap::from([("aac_codre".to_string(), "fast".to_string())]),
            audio_bitrate: 128,
        };

        assert!(
            recording
                .validate()
                .unwrap_err()
                .contains("unsupported audio option")
        );
    }
}
