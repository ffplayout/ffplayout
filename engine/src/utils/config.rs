use std::{
    fmt,
    path::{Path, PathBuf},
    str::FromStr,
};

use chrono::NaiveTime;
use chrono_tz::Tz;
use flexi_logger::Level;
use regex::Regex;
use serde::{Deserialize, Serialize};
use shlex::split;
use sqlx::{Pool, Sqlite};
use tokio::{fs, io::AsyncReadExt};
use ts_rs::TS;

use crate::db::{handles, models};
use crate::file::norm_abs_path;
use crate::utils::{gen_tcp_socket, time_to_sec};
use crate::vec_strings;
use crate::AdvancedConfig;
use crate::ARGS;

use super::errors::ServiceError;

pub const DUMMY_LEN: f64 = 60.0;
pub const IMAGE_FORMAT: [&str; 21] = [
    "bmp", "dds", "dpx", "exr", "gif", "hdr", "j2k", "jpg", "jpeg", "pcx", "pfm", "pgm", "phm",
    "png", "psd", "ppm", "sgi", "svg", "tga", "tif", "webp",
];

// Some well known errors can be safely ignore
pub const FFMPEG_IGNORE_ERRORS: [&str; 13] = [
    "ac-tex damaged",
    "codec s302m, is muxed as a private data stream",
    "corrupt decoded frame in stream",
    "corrupt input packet in stream",
    "end mismatch left",
    "Invalid mb type in I-frame at",
    "Packet corrupt",
    "Referenced QT chapter track not found",
    "skipped MB in I-frame at",
    "Thread message queue blocking",
    "timestamp discontinuity",
    "Warning MVs not available",
    "frame size not set",
];

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
    #[ts(skip)]
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
    pub audio_only: bool,
    pub copy_audio: bool,
    pub copy_video: bool,
    pub width: i64,
    pub height: i64,
    pub aspect: f64,
    pub fps: f64,
    pub add_logo: bool,
    pub logo: String,
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub logo_path: String,
    pub logo_scale: String,
    pub logo_opacity: f64,
    pub logo_position: String,
    pub audio_tracks: i32,
    #[serde(default = "default_track_index")]
    pub audio_track_index: i32,
    pub audio_channels: u8,
    pub volume: f64,
    pub custom_filter: String,
    pub override_filter: bool,
    #[serde(default)]
    pub vtt_enable: bool,
    #[serde(default)]
    pub vtt_dummy: Option<String>,
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub cmd: Option<Vec<String>>,
}

impl Processing {
    fn new(config: &models::Configuration) -> Self {
        Self {
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
            logo_path: config.processing_logo.clone(),
            logo_scale: config.processing_logo_scale.clone(),
            logo_opacity: config.processing_logo_opacity,
            logo_position: config.processing_logo_position.clone(),
            audio_tracks: config.processing_audio_tracks,
            audio_channels: config.processing_audio_channels,
            volume: config.processing_volume,
            custom_filter: config.processing_filter.clone(),
            override_filter: config.processing_override_filter,
            vtt_enable: config.processing_vtt_enable,
            vtt_dummy: config.processing_vtt_dummy.clone(),
            cmd: None,
        }
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, TS)]
#[ts(export, export_to = "playout_config.d.ts")]
pub struct Ingest {
    pub enable: bool,
    pub input_param: String,
    pub custom_filter: String,
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub input_cmd: Option<Vec<String>>,
}

impl Ingest {
    fn new(config: &models::Configuration) -> Self {
        Self {
            enable: config.ingest_enable,
            input_param: config.ingest_param.clone(),
            custom_filter: config.ingest_filter.clone(),
            input_cmd: None,
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
    pub mode: OutputMode,
    pub output_param: String,
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub output_count: usize,
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub output_filter: Option<String>,
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub output_cmd: Option<Vec<String>>,
}

impl Output {
    fn new(config: &models::Configuration) -> Self {
        Self {
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

impl PlayoutConfig {
    pub async fn new(pool: &Pool<Sqlite>, channel_id: i32) -> Result<Self, ServiceError> {
        let global = handles::select_global(pool).await?;
        let channel = handles::select_channel(pool, &channel_id).await?;
        let config = handles::select_configuration(pool, channel_id).await?;
        let adv_config = handles::select_advanced_configuration(pool, channel_id).await?;

        let channel = Channel::new(&global, channel);
        let advanced = AdvancedConfig::new(adv_config);
        let general = General::new(&config);
        let mail = Mail::new(&global, &config);
        let logging = Logging::new(&config);
        let mut processing = Processing::new(&config);
        let mut ingest = Ingest::new(&config);
        let mut playlist = Playlist::new(&config);
        let mut text = Text::new(&config);
        let task = Task::new(&config);
        let mut output = Output::new(&config);
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

        if processing.audio_tracks < 1 {
            processing.audio_tracks = 1;
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
                &buff_size,
                "-mpegts_flags",
                "initial_discontinuity"
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

            let is_tee_muxer = cmd.contains(&"tee".to_string());
            let re_ts = Regex::new(r"filename=(\S+?\.ts)").unwrap();
            let re_m3 = Regex::new(r"\](\S+?\.m3u8)").unwrap();

            for item in &mut cmd {
                if item.ends_with(".ts") || (item.ends_with(".m3u8") && item != "master.m3u8") {
                    if is_tee_muxer {
                        // Processes the `item` string to replace `.ts` and `.m3u8` filenames with their absolute paths.
                        // Ensures that the corresponding directories exist.
                        //
                        // - Uses regular expressions to identify `.ts` and `.m3u8` filenames within the `item` string.
                        // - For each identified filename, normalizes its path and checks if the parent directory exists.
                        // - Creates the parent directory if it does not exist.
                        // - Replaces the original filename in the `item` string with the normalized absolute path.

                        for s in item.clone().split('|') {
                            if let Some(ts) = re_ts.captures(s).and_then(|p| p.get(1)) {
                                let (segment_path, _, _) =
                                    norm_abs_path(&channel.public, ts.as_str())?;
                                let parent = segment_path.parent().ok_or("HLS parent path")?;

                                if !parent.is_dir() {
                                    fs::create_dir_all(parent).await?;
                                }

                                item.clone_from(
                                    &item.replace(ts.as_str(), &segment_path.to_string_lossy()),
                                );
                            }

                            if let Some(m3) = re_m3.captures(s).and_then(|p| p.get(1)) {
                                let (m3u8_path, _, _) =
                                    norm_abs_path(&channel.public, m3.as_str())?;
                                let parent = m3u8_path.parent().ok_or("HLS parent path")?;

                                if !parent.is_dir() {
                                    fs::create_dir_all(parent).await?;
                                }

                                item.clone_from(
                                    &item.replace(m3.as_str(), &m3u8_path.to_string_lossy()),
                                );
                            }
                        }
                    } else if let Ok((public, _, _)) = norm_abs_path(&channel.public, item) {
                        let parent = public.parent().ok_or("HLS parent path")?;

                        if !parent.is_dir() {
                            fs::create_dir_all(parent).await?;
                        }
                        item.clone_from(&public.to_string_lossy().to_string());
                    };
                }
            }

            output.output_cmd = Some(cmd);
        }

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
        })
    }

    pub async fn dump(pool: &Pool<Sqlite>, id: i32) -> Result<(), ServiceError> {
        let config = Self::new(pool, id).await?;

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

/// Read command line arguments, and override the config with them.
pub async fn get_config(
    pool: &Pool<Sqlite>,
    channel_id: i32,
) -> Result<PlayoutConfig, ServiceError> {
    let mut config = PlayoutConfig::new(pool, channel_id).await?;
    let args = ARGS.clone();

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

        template.sources.sort_by(|d1, d2| d1.start.cmp(&d2.start));

        config.general.template = Some(template);
    }

    if let Some(paths) = args.paths {
        config.storage.paths = paths;
    }

    if let Some(playlist) = args.playlists {
        config.channel.playlists = PathBuf::from(&playlist);
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

        if config.output.mode == OutputMode::Null {
            config.output.output_count = 1;
            config.output.output_filter = None;
            config.output.output_cmd = Some(vec_strings!["-f", "null", "-"]);
        }
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
