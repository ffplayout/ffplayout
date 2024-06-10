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

use crate::db::{handles, models::Configuration};
use crate::utils::{free_tcp_socket, time_to_sec};
use crate::vec_strings;
use crate::AdvancedConfig;

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

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ProcessMode {
    Folder,
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

impl Default for ProcessMode {
    fn default() -> Self {
        ProcessMode::Playlist
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
    pub output: Output,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct General {
    pub help_text: String,
    pub channel_id: i32,
    pub stop_threshold: f64,
    pub generate: Option<Vec<String>>,
    pub ffmpeg_filters: Vec<String>,
    pub ffmpeg_libs: Vec<String>,
    pub template: Option<Template>,
    pub skip_validation: bool,
    pub validate: bool,
}

impl General {
    fn new(channel_id: i32, config: &Configuration) -> Self {
        Self {
            help_text: config.general_help.clone(),
            channel_id,
            stop_threshold: config.stop_threshold,
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
    pub interval: u64,
}

impl Mail {
    fn new(config: &Configuration) -> Self {
        Self {
            help_text: config.mail_help.clone(),
            subject: config.subject.clone(),
            smtp_server: config.smtp_server.clone(),
            starttls: config.starttls,
            sender_addr: config.sender_addr.clone(),
            sender_pass: config.sender_pass.clone(),
            recipient: config.recipient.clone(),
            mail_level: string_to_log_level(config.mail_level.clone()),
            interval: config.interval as u64,
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
            interval: u64::default(),
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
    fn new(config: &Configuration) -> Self {
        Self {
            help_text: config.logging_help.clone(),
            ffmpeg_level: config.ffmpeg_level.clone(),
            ingest_level: config.ingest_level.clone(),
            detect_silence: config.detect_silence,
            ignore_lines: config
                .ignore_lines
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
    #[serde(default = "default_track_index")]
    pub audio_track_index: i32,
    pub copy_audio: bool,
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
    pub audio_tracks: i32,
    pub audio_channels: u8,
    pub volume: f64,
    pub custom_filter: String,
    pub cmd: Option<Vec<String>>,
}

impl Processing {
    fn new(config: &Configuration) -> Self {
        Self {
            help_text: config.processing_help.clone(),
            mode: ProcessMode::new(&config.processing_mode.clone()),
            audio_only: config.audio_only,
            audio_track_index: config.audio_track_index,
            copy_audio: config.copy_audio,
            copy_video: config.copy_video,
            width: config.width,
            height: config.height,
            aspect: config.aspect,
            fps: config.fps,
            add_logo: config.add_logo,
            logo: config.logo.clone(),
            logo_scale: config.logo_scale.clone(),
            logo_opacity: config.logo_opacity,
            logo_position: config.logo_position.clone(),
            audio_tracks: config.audio_tracks,
            audio_channels: config.audio_channels,
            volume: config.volume,
            custom_filter: config.decoder_filter.clone(),
            cmd: None,
        }
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct Ingest {
    pub help_text: String,
    pub enable: bool,
    input_param: String,
    pub custom_filter: String,
    pub input_cmd: Option<Vec<String>>,
}

impl Ingest {
    fn new(config: &Configuration) -> Self {
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
    pub path: PathBuf,
    pub day_start: String,
    pub start_sec: Option<f64>,
    pub length: String,
    pub length_sec: Option<f64>,
    pub infinit: bool,
}

impl Playlist {
    fn new(config: &Configuration) -> Self {
        Self {
            help_text: config.playlist_help.clone(),
            path: PathBuf::from(config.playlist_path.clone()),
            day_start: config.day_start.clone(),
            start_sec: None,
            length: config.length.clone(),
            length_sec: None,
            infinit: config.infinit,
        }
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct Storage {
    pub help_text: String,
    pub path: PathBuf,
    pub paths: Vec<PathBuf>,
    pub filler: PathBuf,
    pub extensions: Vec<String>,
    pub shuffle: bool,
}

impl Storage {
    fn new(config: &Configuration) -> Self {
        Self {
            help_text: config.storage_help.clone(),
            path: PathBuf::from(config.storage_path.clone()),
            paths: vec![],
            filler: PathBuf::from(config.filler.clone()),
            extensions: config
                .extensions
                .split(';')
                .map(|s| s.to_string())
                .collect(),
            shuffle: config.shuffle,
        }
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct Text {
    pub help_text: String,
    pub add_text: bool,
    pub node_pos: Option<usize>,
    pub zmq_stream_socket: Option<String>,
    pub zmq_server_socket: Option<String>,
    pub fontfile: String,
    pub text_from_filename: bool,
    pub style: String,
    pub regex: String,
}

impl Text {
    fn new(config: &Configuration) -> Self {
        Self {
            help_text: config.text_help.clone(),
            add_text: config.add_text.clone(),
            node_pos: None,
            zmq_stream_socket: None,
            zmq_server_socket: None,
            fontfile: config.fontfile.clone(),
            text_from_filename: config.text_from_filename,
            style: config.style.clone(),
            regex: config.regex.clone(),
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
    fn new(config: &Configuration) -> Self {
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
    pub output_count: usize,
    pub output_filter: Option<String>,
    pub output_cmd: Option<Vec<String>>,
}

impl Output {
    fn new(config: &Configuration) -> Self {
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
    pub async fn new(pool: &Pool<Sqlite>, channel: i32) -> Self {
        let config = handles::select_configuration(pool, channel)
            .await
            .expect("Can't read config");
        let adv_config = handles::select_advanced_configuration(pool, channel)
            .await
            .expect("Can't read advanced config");

        let advanced = AdvancedConfig::new(adv_config);
        let general = General::new(channel, &config);
        let mail = Mail::new(&config);
        let logging = Logging::new(&config);
        let mut processing = Processing::new(&config);
        let mut ingest = Ingest::new(&config);
        let mut playlist = Playlist::new(&config);
        let storage = Storage::new(&config);
        let mut text = Text::new(&config);
        let task = Task::new(&config);
        let mut output = Output::new(&config);

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
