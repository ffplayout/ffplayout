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
    utils::{errors::ServiceError, time_to_sec},
};

pub const DUMMY_LEN: f64 = 60.0;

#[derive(Debug, Clone, Default, Eq, PartialEq, Deserialize, Serialize, TS)]
#[ts(export, export_to = "playout_config.d.ts")]
#[serde(rename_all = "lowercase")]
pub enum OutputMode {
    Desktop,
    #[default]
    HLS,
    Stream,
}

impl OutputMode {
    fn new(s: &str) -> Self {
        match s {
            "desktop" => Self::Desktop,
            "stream" => Self::Stream,
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
    pub logging: Logging,
    pub processing: Processing,
    pub ingest: Ingest,
    pub playlist: Playlist,
    pub storage: Storage,
    pub text: Text,
    pub task: Task,
    #[serde(alias = "out")]
    pub output: Output,
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
    pub volume: f64,
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
            volume: config.processing_volume,
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
pub struct Ingest {
    pub enable: bool,
    pub ingest_url: String,
}

impl Ingest {
    fn new(config: &models::Configuration) -> Self {
        Self {
            enable: config.ingest_enable,
            ingest_url: config.ingest_url.clone(),
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

#[derive(Debug, Default, Clone, Deserialize, Serialize, TS)]
#[ts(export, export_to = "playout_config.d.ts")]
pub struct Output {
    pub id: i32,
    pub mode: OutputMode,
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
    #[serde(default = "default_audio_codec")]
    pub audio_codec: String,
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

impl Output {
    fn new(config: &models::Configuration, outputs: Vec<models::Output>) -> Self {
        let output = outputs
            .iter()
            .find(|output| output.id == config.output_id)
            .cloned()
            .unwrap_or_default();
        let video_codec = output.video_codec.unwrap_or_else(default_video_codec);
        let video_options = serde_json::from_str(&output.video_options)
            .unwrap_or_else(|_| ff_engine::video_option_defaults(&video_codec));

        Self {
            id: output.id,
            mode: OutputMode::new(&output.name),
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
            audio_codec: output.audio_codec.unwrap_or_else(default_audio_codec),
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

        let additional = self.parsed_hls_variants()?;
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

    pub fn validate(&self) -> Result<(), String> {
        if self.width == 0 || self.height == 0 {
            return Err("output size must be greater than zero".to_string());
        }
        if !self.fps.is_finite() || self.fps < 1.0 || self.fps > f64::from(u32::MAX) {
            return Err("output fps must be a positive number".to_string());
        }

        if matches!(self.mode, OutputMode::HLS | OutputMode::Stream) {
            let target = match self.mode {
                OutputMode::HLS => ff_engine::FfmpegOutputTarget::Hls,
                OutputMode::Stream => self
                    .stream_type
                    .ffmpeg_target()
                    .unwrap_or(ff_engine::FfmpegOutputTarget::Rtmp),
                OutputMode::Desktop => unreachable!("desktop output is not encoded"),
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
            ff_engine::validate_video_options(&self.video_codec, &self.video_options)?;
            if ff_engine::audio_codec_uses_bitrate(&self.audio_codec) && self.audio_bitrate == 0 {
                return Err("audio bitrate must be greater than zero".to_string());
            }
        }

        match self.mode {
            OutputMode::HLS => {
                if self.hls_segment_duration == 0 {
                    return Err("HLS segment duration must be greater than zero".to_string());
                }
                self.hls_streams()?;
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
    pub async fn new(
        pool: &Pool<Sqlite>,
        channel_id: i32,
        output_id: Option<i32>,
    ) -> Result<Self, ServiceError> {
        let global = handles::select_global(pool).await?;
        let channel = handles::select_channel(pool, &channel_id).await?;
        let mut config = handles::select_configuration(pool, channel_id).await?;
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
        let logging = Logging::new(&config);
        let mut processing = Processing::new(&config);
        let ingest = Ingest::new(&config);
        let mut playlist = Playlist::new(&config);
        let text = Text::new(&config, text_preset);
        let task = Task::new(&config);
        let output = Output::new(&config, outputs);
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
            logging,
            processing,
            ingest,
            playlist,
            storage,
            text,
            task,
            output,
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
    let args = ARGS.clone();
    let output_id = if let Some(output_name) = &args.output {
        let outputs = handles::select_outputs(pool, channel_id).await?;
        outputs
            .iter()
            .find(|out| out.name == output_name.to_string())
            .map(|out| out.id)
    } else {
        None
    };

    let mut config = PlayoutConfig::new(pool, channel_id, output_id).await?;

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

    if let Some(output) = args.output {
        config.output.mode = output;
    }

    if let Some(volume) = args.volume {
        config.processing.volume = volume;
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
    use super::{Output, OutputMode, StreamType};

    fn output(mode: OutputMode) -> Output {
        Output {
            id: 1,
            mode,
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
            audio_codec: "aac".to_string(),
            audio_bitrate: 128,
            hls_variants: Vec::new(),
        }
    }

    #[test]
    fn validates_structured_output_settings() {
        assert!(output(OutputMode::HLS).validate().is_ok());
        assert!(output(OutputMode::Stream).validate().is_ok());
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
    use super::{MIN_INGEST_PORT, parse_rtmp_ingest_port};

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
