use std::{
    fmt,
    path::{Path, PathBuf},
    str::FromStr,
};

use chrono::NaiveTime;
use flexi_logger::Level;
use serde::{Deserialize, Serialize};
use shlex::split;
use sqlx::{Pool, Sqlite};
use tokio::io::AsyncReadExt;

use crate::db::{handles, models};
use crate::utils::{files::norm_abs_path, free_tcp_socket, time_to_sec};
use crate::vec_strings;
use crate::AdvancedConfig;

use super::errors::ServiceError;

pub const DUMMY_LEN: f64 = 60.0;
pub const IMAGE_FORMAT: [&str; 21] = [
    "bmp", "dds", "dpx", "exr", "gif", "hdr", "j2k", "jpg", "jpeg", "pcx", "pfm", "pgm", "phm",
    "png", "psd", "ppm", "sgi", "svg", "tga", "tif", "webp",
];

// Some well known errors can be safely ignore
pub const FFMPEG_IGNORE_ERRORS: [&str; 11] = [
    "ac-tex damaged",
    "codec s302m, is muxed as a private data stream",
    "corrupt decoded frame in stream",
    "corrupt input packet in stream",
    "end mismatch left",
    "Packet corrupt",
    "Referenced QT chapter track not found",
    "skipped MB in I-frame at",
    "Thread message queue blocking",
    "Warning MVs not available",
    "frame size not set",
];

pub const FFMPEG_UNRECOVERABLE_ERRORS: [&str; 5] = [
    "Address already in use",
    "Invalid argument",
    "Numerical result",
    "Error initializing complex filters",
    "Error while decoding stream #0:0: Invalid data found when processing input",
];

#[derive(Debug, Clone, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputMode {
    Desktop,
    HLS,
    Null,
    Stream,
}

impl OutputMode {
    fn new(s: &str) -> Self {
        match s {
            "desktop" => Self::Desktop,
            "null" => Self::Null,
            "stream" => Self::Stream,
            _ => Self::HLS,
        }
    }
}

impl Default for OutputMode {
    fn default() -> Self {
        Self::HLS
    }
}

impl FromStr for OutputMode {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "desktop" => Ok(Self::Desktop),
            "hls" => Ok(Self::HLS),
            "null" => Ok(Self::Null),
            "stream" => Ok(Self::Stream),
            _ => Err("Use 'desktop', 'hls', 'null' or 'stream'".to_string()),
        }
    }
}

impl fmt::Display for OutputMode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            OutputMode::Desktop => write!(f, "desktop"),
            OutputMode::HLS => write!(f, "hls"),
            OutputMode::Null => write!(f, "null"),
            OutputMode::Stream => write!(f, "stream"),
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, Eq, PartialEq)]
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

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Template {
    pub sources: Vec<Source>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Source {
    pub start: NaiveTime,
    pub duration: NaiveTime,
    pub shuffle: bool,
    pub paths: Vec<PathBuf>,
}

/// Global Config
///
/// This we init ones, when ffplayout is starting and use them globally in the hole program.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct PlayoutConfig {
    #[serde(skip_serializing, skip_deserializing)]
    pub global: Global,
    #[serde(skip_serializing, skip_deserializing)]
    pub advanced: AdvancedConfig,
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

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct Global {
    pub hls_path: PathBuf,
    pub playlist_path: PathBuf,
    pub storage_path: PathBuf,
    pub logging_path: PathBuf,
    pub shared_storage: bool,
}

impl Global {
    pub fn new(config: &models::GlobalSettings) -> Self {
        Self {
            hls_path: PathBuf::from(config.hls_path.clone()),
            playlist_path: PathBuf::from(config.playlist_path.clone()),
            storage_path: PathBuf::from(config.storage_path.clone()),
            logging_path: PathBuf::from(config.logging_path.clone()),
            shared_storage: config.shared_storage,
        }
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct General {
    pub help_text: String,
    #[serde(skip_serializing, skip_deserializing)]
    pub id: i32,
    #[serde(skip_serializing, skip_deserializing)]
    pub channel_id: i32,
    pub stop_threshold: f64,
    #[serde(skip_serializing, skip_deserializing)]
    pub generate: Option<Vec<String>>,
    #[serde(skip_serializing, skip_deserializing)]
    pub ffmpeg_filters: Vec<String>,
    #[serde(skip_serializing, skip_deserializing)]
    pub ffmpeg_libs: Vec<String>,
    #[serde(skip_serializing, skip_deserializing)]
    pub template: Option<Template>,
    #[serde(skip_serializing, skip_deserializing)]
    pub skip_validation: bool,
    #[serde(skip_serializing, skip_deserializing)]
    pub validate: bool,
}

impl General {
    fn new(config: &models::Configuration) -> Self {
        Self {
            help_text: config.general_help.clone(),
            id: config.id,
            channel_id: config.channel_id,
            stop_threshold: config.general_stop_threshold,
            generate: None,
            ffmpeg_filters: vec![],
            ffmpeg_libs: vec![],
            template: None,
            skip_validation: false,
            validate: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Mail {
    pub help_text: String,
    pub subject: String,
    pub smtp_server: String,
    pub starttls: bool,
    pub sender_addr: String,
    pub sender_pass: String,
    pub recipient: String,
    pub mail_level: Level,
    pub interval: i64,
}

impl Mail {
    fn new(config: &models::Configuration) -> Self {
        Self {
            help_text: config.mail_help.clone(),
            subject: config.mail_subject.clone(),
            smtp_server: config.mail_smtp.clone(),
            starttls: config.mail_starttls,
            sender_addr: config.mail_addr.clone(),
            sender_pass: config.mail_pass.clone(),
            recipient: config.mail_recipient.clone(),
            mail_level: string_to_log_level(config.mail_level.clone()),
            interval: config.mail_interval,
        }
    }
}

impl Default for Mail {
    fn default() -> Self {
        Mail {
            help_text: String::default(),
            subject: String::default(),
            smtp_server: String::default(),
            starttls: bool::default(),
            sender_addr: String::default(),
            sender_pass: String::default(),
            recipient: String::default(),
            mail_level: Level::Debug,
            interval: i64::default(),
        }
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct Logging {
    pub help_text: String,
    pub ffmpeg_level: String,
    pub ingest_level: String,
    pub detect_silence: bool,
    pub ignore_lines: Vec<String>,
}

impl Logging {
    fn new(config: &models::Configuration) -> Self {
        Self {
            help_text: config.logging_help.clone(),
            ffmpeg_level: config.logging_ffmpeg_level.clone(),
            ingest_level: config.logging_ingest_level.clone(),
            detect_silence: config.logging_detect_silence,
            ignore_lines: config
                .logging_ignore
                .split(';')
                .map(|s| s.to_string())
                .collect(),
        }
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct Processing {
    pub help_text: String,
    pub mode: ProcessMode,
    pub audio_only: bool,
    pub copy_audio: bool,
    pub copy_video: bool,
    pub width: i64,
    pub height: i64,
    pub aspect: f64,
    pub fps: f64,
    pub add_logo: bool,
    pub logo: String,
    pub logo_scale: String,
    pub logo_opacity: f64,
    pub logo_position: String,
    pub audio_tracks: i32,
    #[serde(default = "default_track_index")]
    pub audio_track_index: i32,
    pub audio_channels: u8,
    pub volume: f64,
    pub custom_filter: String,
    #[serde(skip_serializing, skip_deserializing)]
    pub cmd: Option<Vec<String>>,
}

impl Processing {
    fn new(config: &models::Configuration) -> Self {
        Self {
            help_text: config.processing_help.clone(),
            mode: ProcessMode::new(&config.processing_mode.clone()),
            audio_only: config.processing_audio_only,
            audio_track_index: config.processing_audio_track_index,
            copy_audio: config.processing_copy_audio,
            copy_video: config.processing_copy_video,
            width: config.processing_width,
            height: config.processing_height,
            aspect: config.processing_aspect,
            fps: config.processing_fps,
            add_logo: config.processing_add_logo,
            logo: config.processing_logo.clone(),
            logo_scale: config.processing_logo_scale.clone(),
            logo_opacity: config.processing_logo_opacity,
            logo_position: config.processing_logo_position.clone(),
            audio_tracks: config.processing_audio_tracks,
            audio_channels: config.processing_audio_channels,
            volume: config.processing_volume,
            custom_filter: config.processing_filter.clone(),
            cmd: None,
        }
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct Ingest {
    pub help_text: String,
    pub enable: bool,
    pub input_param: String,
    pub custom_filter: String,
    #[serde(skip_serializing, skip_deserializing)]
    pub input_cmd: Option<Vec<String>>,
}

impl Ingest {
    fn new(config: &models::Configuration) -> Self {
        Self {
            help_text: config.ingest_help.clone(),
            enable: config.ingest_enable,
            input_param: config.ingest_param.clone(),
            custom_filter: config.ingest_filter.clone(),
            input_cmd: None,
        }
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct Playlist {
    pub help_text: String,
    pub day_start: String,
    #[serde(skip_serializing, skip_deserializing)]
    pub start_sec: Option<f64>,
    pub length: String,
    #[serde(skip_serializing, skip_deserializing)]
    pub length_sec: Option<f64>,
    pub infinit: bool,
}

impl Playlist {
    fn new(config: &models::Configuration) -> Self {
        Self {
            help_text: config.playlist_help.clone(),
            day_start: config.playlist_day_start.clone(),
            start_sec: None,
            length: config.playlist_length.clone(),
            length_sec: None,
            infinit: config.playlist_infinit,
        }
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct Storage {
    pub help_text: String,
    #[serde(skip_serializing, skip_deserializing)]
    pub paths: Vec<PathBuf>,
    pub filler: PathBuf,
    pub extensions: Vec<String>,
    pub shuffle: bool,
}

impl Storage {
    fn new(config: &models::Configuration) -> Self {
        Self {
            help_text: config.storage_help.clone(),
            paths: vec![],
            filler: PathBuf::from(config.storage_filler.clone()),
            extensions: config
                .storage_extensions
                .split(';')
                .map(|s| s.to_string())
                .collect(),
            shuffle: config.storage_shuffle,
        }
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct Text {
    pub help_text: String,
    pub add_text: bool,
    #[serde(skip_serializing, skip_deserializing)]
    pub node_pos: Option<usize>,
    #[serde(skip_serializing, skip_deserializing)]
    pub zmq_stream_socket: Option<String>,
    #[serde(skip_serializing, skip_deserializing)]
    pub zmq_server_socket: Option<String>,
    pub fontfile: String,
    pub text_from_filename: bool,
    pub style: String,
    pub regex: String,
}

impl Text {
    fn new(config: &models::Configuration) -> Self {
        Self {
            help_text: config.text_help.clone(),
            add_text: config.text_add,
            node_pos: None,
            zmq_stream_socket: None,
            zmq_server_socket: None,
            fontfile: config.text_font.clone(),
            text_from_filename: config.text_from_filename,
            style: config.text_style.clone(),
            regex: config.text_regex.clone(),
        }
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct Task {
    pub help_text: String,
    pub enable: bool,
    pub path: PathBuf,
}

impl Task {
    fn new(config: &models::Configuration) -> Self {
        Self {
            help_text: config.task_help.clone(),
            enable: config.task_enable,
            path: PathBuf::from(config.task_path.clone()),
        }
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct Output {
    pub help_text: String,
    pub mode: OutputMode,
    pub output_param: String,
    #[serde(skip_serializing, skip_deserializing)]
    pub output_count: usize,
    #[serde(skip_serializing, skip_deserializing)]
    pub output_filter: Option<String>,
    #[serde(skip_serializing, skip_deserializing)]
    pub output_cmd: Option<Vec<String>>,
}

impl Output {
    fn new(config: &models::Configuration) -> Self {
        Self {
            help_text: config.output_help.clone(),
            mode: OutputMode::new(&config.output_mode),
            output_param: config.output_param.clone(),
            output_count: 0,
            output_filter: None,
            output_cmd: None,
        }
    }
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
        "null" => OutputMode::Null,
        "stream" => OutputMode::Stream,
        _ => OutputMode::HLS,
    }
}

fn default_track_index() -> i32 {
    -1
}

// fn default_tracks() -> i32 {
//     1
// }

// fn default_channels() -> u8 {
//     2
// }

impl PlayoutConfig {
    pub async fn new(pool: &Pool<Sqlite>, channel_id: i32) -> Self {
        let global = handles::select_global(pool)
            .await
            .expect("Can't read globals");
        let config = handles::select_configuration(pool, channel_id)
            .await
            .expect("Can't read config");
        let adv_config = handles::select_advanced_configuration(pool, channel_id)
            .await
            .expect("Can't read advanced config");

        let mut global = Global::new(&global);
        let advanced = AdvancedConfig::new(adv_config);
        let general = General::new(&config);
        let mail = Mail::new(&config);
        let logging = Logging::new(&config);
        let mut processing = Processing::new(&config);
        let mut ingest = Ingest::new(&config);
        let mut playlist = Playlist::new(&config);
        let mut storage = Storage::new(&config);
        let mut text = Text::new(&config);
        let task = Task::new(&config);
        let mut output = Output::new(&config);

        if !global.shared_storage {
            global.storage_path = global.storage_path.join(channel_id.to_string());
        }

        if !global.storage_path.is_dir() {
            tokio::fs::create_dir_all(&global.storage_path)
                .await
                .expect("Can't create storage folder");
        }

        if channel_id > 1 || !global.shared_storage {
            global.playlist_path = global.playlist_path.join(channel_id.to_string());
            global.hls_path = global.hls_path.join(channel_id.to_string());
        }

        if !global.playlist_path.is_dir() {
            tokio::fs::create_dir_all(&global.playlist_path)
                .await
                .expect("Can't create playlist folder");
        }

        let (filler_path, _, _) = norm_abs_path(&global.storage_path, &config.storage_filler)
            .expect("Can't get filler path");

        storage.filler = filler_path;

        playlist.start_sec = Some(time_to_sec(&playlist.day_start));

        if playlist.length.contains(':') {
            playlist.length_sec = Some(time_to_sec(&playlist.length));
        } else {
            playlist.length_sec = Some(86400.0);
        }

        if processing.add_logo && !Path::new(&processing.logo).is_file() {
            processing.add_logo = false;
        }

        if processing.audio_tracks < 1 {
            processing.audio_tracks = 1
        }

        let mut process_cmd = vec_strings![];

        if processing.audio_only {
            process_cmd.append(&mut vec_strings!["-vn"]);
        } else if processing.copy_video {
            process_cmd.append(&mut vec_strings!["-c:v", "copy"]);
        } else if let Some(decoder_cmd) = &advanced.decoder.output_cmd {
            process_cmd.append(&mut decoder_cmd.clone());
        } else {
            let bitrate = format!("{}k", processing.width * processing.height / 16);
            let buff_size = format!("{}k", (processing.width * processing.height / 16) / 2);

            process_cmd.append(&mut vec_strings![
                "-pix_fmt",
                "yuv420p",
                "-r",
                &processing.fps,
                "-c:v",
                "mpeg2video",
                "-g",
                "1",
                "-b:v",
                &bitrate,
                "-minrate",
                &bitrate,
                "-maxrate",
                &bitrate,
                "-bufsize",
                &buff_size
            ]);
        }

        if processing.copy_audio {
            process_cmd.append(&mut vec_strings!["-c:a", "copy"]);
        } else if advanced.decoder.output_cmd.is_none() {
            process_cmd.append(&mut pre_audio_codec(
                &processing.custom_filter,
                &ingest.custom_filter,
                processing.audio_channels,
            ));
        }

        process_cmd.append(&mut vec_strings!["-f", "mpegts", "-"]);

        processing.cmd = Some(process_cmd);

        ingest.input_cmd = split(ingest.input_param.as_str());

        output.output_count = 1;
        output.output_filter = None;

        if output.mode == OutputMode::Null {
            output.output_cmd = Some(vec_strings!["-f", "null", "-"]);
        } else if let Some(mut cmd) = split(output.output_param.as_str()) {
            // get output count according to the var_stream_map value, or by counting output parameters
            if let Some(i) = cmd.clone().iter().position(|m| m == "-var_stream_map") {
                output.output_count = cmd[i + 1].split_whitespace().count();
            } else {
                output.output_count = cmd
                    .iter()
                    .enumerate()
                    .filter(|(i, p)| i > &0 && !p.starts_with('-') && !cmd[i - 1].starts_with('-'))
                    .count();
            }

            if let Some(i) = cmd.clone().iter().position(|r| r == "-filter_complex") {
                output.output_filter = Some(cmd[i + 1].clone());
                cmd.remove(i);
                cmd.remove(i);
            }

            for item in cmd.iter_mut() {
                if item.ends_with(".ts") || (item.ends_with(".m3u8") && item != "master.m3u8") {
                    if let Ok((hls_path, _, _)) = norm_abs_path(&global.hls_path, item) {
                        item.clone_from(&hls_path.to_string_lossy().to_string());
                    };
                }
            }

            output.output_cmd = Some(cmd);
        }

        // when text overlay without text_from_filename is on, turn also the RPC server on,
        // to get text messages from it
        if text.add_text && !text.text_from_filename {
            text.zmq_stream_socket = free_tcp_socket(String::new());
            text.zmq_server_socket =
                free_tcp_socket(text.zmq_stream_socket.clone().unwrap_or_default());
            text.node_pos = Some(2);
        } else {
            text.zmq_stream_socket = None;
            text.zmq_server_socket = None;
            text.node_pos = None;
        }

        Self {
            global,
            advanced,
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
        }
    }

    pub async fn dump(pool: &Pool<Sqlite>, id: i32) -> Result<(), ServiceError> {
        let mut config = Self::new(pool, id).await;
        config.storage.filler.clone_from(
            &config
                .storage
                .filler
                .strip_prefix(config.global.storage_path.clone())
                .unwrap_or(&config.storage.filler)
                .to_path_buf(),
        );

        let toml_string = toml_edit::ser::to_string_pretty(&config)?;
        tokio::fs::write(&format!("ffplayout_{id}.toml"), toml_string).await?;

        Ok(())
    }

    pub async fn import(pool: &Pool<Sqlite>, import: Vec<String>) -> Result<(), ServiceError> {
        let id = import[0].parse::<i32>()?;
        let path = Path::new(&import[1]);

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

// impl Default for PlayoutConfig {
//     fn default() -> Self {
//         Self::new(1)
//     }
// }

/// When custom_filter contains loudnorm filter use a different audio encoder,
/// s302m has higher quality, but is experimental
/// and works not well together with the loudnorm filter.
fn pre_audio_codec(proc_filter: &str, ingest_filter: &str, channel_count: u8) -> Vec<String> {
    let mut codec = vec_strings![
        "-c:a",
        "s302m",
        "-strict",
        "-2",
        "-sample_fmt",
        "s16",
        "-ar",
        "48000",
        "-ac",
        channel_count
    ];

    if proc_filter.contains("loudnorm") || ingest_filter.contains("loudnorm") {
        codec = vec_strings![
            "-c:a",
            "mp2",
            "-b:a",
            "384k",
            "-ar",
            "48000",
            "-ac",
            channel_count
        ];
    }

    codec
}
