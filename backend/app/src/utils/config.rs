use std::{
    collections::HashSet,
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
    utils::{errors::ServiceError, gen_tcp_socket, time_to_sec},
};

pub const DUMMY_LEN: f64 = 60.0;

pub const FFMPEG_UNRECOVERABLE_ERRORS: [&str; 9] = [
    "Address already in use",
    "Device creation failed",
    "Invalid argument",
    "Numerical result",
    "No such filter",
    "Error initializing complex filters",
    "Error while decoding stream #0:0: Invalid data found when processing input",
    "Unrecognized option",
    "Option not found",
];

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
            ignore_lines: config.logging_ignore.split(';').map(String::from).collect(),
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
    pub add_text: bool,
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub node_pos: Option<usize>,
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub zmq_stream_socket: Option<String>,
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub zmq_server_socket: Option<String>,
    #[serde(alias = "fontfile")]
    pub font: String,
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub font_path: String,
    pub text_from_filename: bool,
    pub style: String,
    pub regex: String,
}

impl Text {
    fn new(config: &models::Configuration) -> Self {
        Self {
            add_text: config.text_add,
            node_pos: None,
            zmq_stream_socket: None,
            zmq_server_socket: None,
            font: config.text_font.clone(),
            font_path: config.text_font.clone(),
            text_from_filename: config.text_from_filename,
            style: config.text_style.clone(),
            regex: config.text_regex.clone(),
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
    #[serde(default = "default_hls_playlist_name")]
    pub hls_playlist_name: String,
    #[serde(default = "default_hls_segment_duration")]
    pub hls_segment_duration: u32,
    #[serde(default = "default_hls_list_size")]
    pub hls_list_size: u32,
    pub width: u32,
    pub height: u32,
    pub aspect: f64,
    pub fps: f64,
    #[serde(default = "default_video_preset")]
    pub video_preset: String,
    #[serde(default = "default_rate_control")]
    pub rate_control: String,
    #[serde(default = "default_video_quality")]
    pub video_quality: u8,
    #[serde(default = "default_video_maxrate")]
    pub video_maxrate: u32,
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

fn default_video_preset() -> String {
    "faster".to_string()
}

fn default_rate_control() -> String {
    "crf".to_string()
}

const fn default_video_quality() -> u8 {
    23
}

const fn default_video_maxrate() -> u32 {
    2400
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

        Self {
            id: output.id,
            mode: OutputMode::new(&output.name),
            stream_url: output.stream_url,
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
            width: u32::try_from(output.width).unwrap_or(1280),
            height: u32::try_from(output.height).unwrap_or(720),
            aspect: output.aspect,
            fps: output.fps,
            video_preset: output.video_preset.unwrap_or_else(default_video_preset),
            rate_control: output.rate_control.unwrap_or_else(default_rate_control),
            video_quality: output
                .video_quality
                .and_then(|value| u8::try_from(value).ok())
                .unwrap_or_else(default_video_quality),
            video_maxrate: output
                .video_maxrate
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or_else(default_video_maxrate),
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

    /// Returns no explicit variants for a standalone base output. Once
    /// additional variants are configured, the base output is prepended so
    /// all renditions are included in the master playlist.
    pub fn hls_streams(&self) -> Result<Vec<ff_engine::HlsVariant>, String> {
        let base = ff_engine::HlsVariant {
            name: self.hls_playlist_name.trim().to_string(),
            width: self.width,
            height: self.height,
            video_bitrate: u64::from(self.video_maxrate) * 1_000,
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

        // TODO: we need one stream at least
        if additional.is_empty() {
            return Ok(Vec::new());
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
        if !self.aspect.is_finite() || self.aspect <= 0.0 {
            return Err("output aspect must be a positive number".to_string());
        }
        if !self.fps.is_finite() || self.fps < 1.0 || self.fps > f64::from(u32::MAX) {
            return Err("output fps must be a positive number".to_string());
        }

        if matches!(self.mode, OutputMode::HLS | OutputMode::Stream) {
            const PRESETS: &[&str] = &[
                "ultrafast",
                "superfast",
                "veryfast",
                "faster",
                "fast",
                "medium",
                "slow",
                "slower",
                "veryslow",
                "placebo",
            ];
            if !PRESETS.contains(&self.video_preset.as_str()) {
                return Err(format!("unsupported video preset {:?}", self.video_preset));
            }
            if !matches!(self.rate_control.as_str(), "crf" | "cbr") {
                return Err("rate control must be \"crf\" or \"cbr\"".to_string());
            }
            if self.rate_control == "crf" && self.video_quality > 51 {
                return Err("CRF quality must be between 0 and 51".to_string());
            }
            if self.video_maxrate == 0 {
                return Err("video maxrate must be greater than zero".to_string());
            }
            if self.audio_bitrate == 0 {
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

pub fn string_to_processing_mode(l: String) -> ProcessMode {
    match l.to_lowercase().as_str() {
        "playlist" => ProcessMode::Playlist,
        "folder" => ProcessMode::Folder,
        _ => ProcessMode::Playlist,
    }
}

pub fn string_to_output_mode(l: String) -> OutputMode {
    match l.to_lowercase().as_str() {
        "desktop" => OutputMode::Desktop,
        "hls" => OutputMode::HLS,
        "stream" => OutputMode::Stream,
        _ => OutputMode::HLS,
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
        let general = General::new(&config);
        let mail = Mail::new(&global, &config);
        let logging = Logging::new(&config);
        let mut processing = Processing::new(&config);
        let ingest = Ingest::new(&config);
        let mut playlist = Playlist::new(&config);
        let mut text = Text::new(&config);
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

        // when text overlay without text_from_filename is on, turn also the RPC server on,
        // to get text messages from it
        if text.add_text && !text.text_from_filename {
            text.zmq_stream_socket = gen_tcp_socket("").await;
            text.zmq_server_socket =
                gen_tcp_socket(&text.zmq_stream_socket.clone().unwrap_or_default()).await;
            text.node_pos = Some(2);
        } else {
            text.zmq_stream_socket = None;
            text.zmq_server_socket = None;
            text.node_pos = None;
        }

        let (font_path, _, font) = norm_abs_path(&channel.storage, &text.font)?;
        text.font = font;
        text.font_path = font_path.to_string_lossy().to_string();

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
    use super::{Output, OutputMode};

    fn output(mode: OutputMode) -> Output {
        Output {
            id: 1,
            mode,
            stream_url: "rtmp://localhost/live/test".to_string(),
            hls_playlist_name: "stream".to_string(),
            hls_segment_duration: 6,
            hls_list_size: 600,
            width: 1280,
            height: 720,
            aspect: 1.778,
            fps: 25.0,
            video_preset: "faster".to_string(),
            rate_control: "crf".to_string(),
            video_quality: 23,
            video_maxrate: 2400,
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
    fn standalone_hls_output_uses_no_explicit_variants() {
        let output = output(OutputMode::HLS);
        assert!(output.hls_streams().unwrap().is_empty());
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
    fn rejects_invalid_crf_quality() {
        let mut output = output(OutputMode::Stream);
        output.video_quality = 52;
        assert_eq!(
            output.validate().unwrap_err(),
            "CRF quality must be between 0 and 51"
        );
    }

    #[test]
    fn cbr_does_not_validate_unused_quality() {
        let mut output = output(OutputMode::Stream);
        output.rate_control = "cbr".to_string();
        output.video_quality = 52;
        assert!(output.validate().is_ok());
    }
}
