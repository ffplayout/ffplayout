use std::{
    env, fmt,
    fs::File,
    path::{Path, PathBuf},
    process,
    str::FromStr,
};

use log::LevelFilter;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use shlex::split;

use super::vec_strings;
use crate::utils::{free_tcp_socket, home_dir, time_to_sec, OutputMode::*};

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

#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum OutputMode {
    Desktop,
    HLS,
    Null,
    Stream,
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

#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ProcessMode {
    Folder,
    Playlist,
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

pub fn string_to_log_level<'de, D>(deserializer: D) -> Result<LevelFilter, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;

    match s.to_lowercase().as_str() {
        "debug" => Ok(LevelFilter::Debug),
        "error" => Ok(LevelFilter::Error),
        "info" => Ok(LevelFilter::Info),
        "trace" => Ok(LevelFilter::Trace),
        "warning" => Ok(LevelFilter::Warn),
        "off" => Ok(LevelFilter::Off),
        _ => Err(de::Error::custom("Error level not exists!")),
    }
}

fn log_level_to_string<S>(l: &LevelFilter, s: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match l {
        LevelFilter::Debug => s.serialize_str("DEBUG"),
        LevelFilter::Error => s.serialize_str("ERROR"),
        LevelFilter::Info => s.serialize_str("INFO"),
        LevelFilter::Trace => s.serialize_str("TRACE"),
        LevelFilter::Warn => s.serialize_str("WARNING"),
        LevelFilter::Off => s.serialize_str("OFF"),
    }
}

/// Global Config
///
/// This we init ones, when ffplayout is starting and use them globally in the hole program.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PlayoutConfig {
    pub general: General,
    pub rpc_server: RpcServer,
    pub mail: Mail,
    pub logging: Logging,
    pub processing: Processing,
    pub ingest: Ingest,
    pub playlist: Playlist,
    pub storage: Storage,
    pub text: Text,
    pub out: Out,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct General {
    pub help_text: String,
    pub stop_threshold: f64,

    #[serde(skip_serializing, skip_deserializing)]
    pub generate: Option<Vec<String>>,

    #[serde(skip_serializing, skip_deserializing)]
    pub stat_file: String,

    #[serde(skip_serializing, skip_deserializing)]
    pub ffmpeg_filters: Vec<String>,

    #[serde(skip_serializing, skip_deserializing)]
    pub ffmpeg_libs: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RpcServer {
    pub help_text: String,
    pub enable: bool,
    pub address: String,
    pub authorization: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Mail {
    pub help_text: String,
    pub subject: String,
    pub smtp_server: String,
    pub starttls: bool,
    pub sender_addr: String,
    pub sender_pass: String,
    pub recipient: String,
    pub mail_level: String,
    pub interval: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Logging {
    pub help_text: String,
    pub log_to_file: bool,
    pub backup_count: usize,
    pub local_time: bool,
    pub timestamp: bool,
    pub log_path: String,
    #[serde(
        serialize_with = "log_level_to_string",
        deserialize_with = "string_to_log_level"
    )]
    pub log_level: LevelFilter,
    pub ffmpeg_level: String,
    pub ingest_level: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Processing {
    pub help_text: String,
    pub mode: ProcessMode,
    #[serde(default)]
    pub audio_only: bool,
    pub width: i64,
    pub height: i64,
    pub aspect: f64,
    pub fps: f64,
    pub add_logo: bool,
    pub logo: String,
    pub logo_scale: String,
    pub logo_opacity: f32,
    pub logo_filter: String,
    #[serde(default = "default_tracks")]
    pub audio_tracks: i32,
    #[serde(default = "default_channels")]
    pub audio_channels: u8,
    pub volume: f64,
    #[serde(default)]
    pub custom_filter: String,

    #[serde(skip_serializing, skip_deserializing)]
    pub cmd: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Ingest {
    pub help_text: String,
    pub enable: bool,
    input_param: String,
    #[serde(default)]
    pub custom_filter: String,

    #[serde(skip_serializing, skip_deserializing)]
    pub input_cmd: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Playlist {
    pub help_text: String,
    pub path: String,
    pub day_start: String,

    #[serde(skip_serializing, skip_deserializing)]
    pub start_sec: Option<f64>,

    pub length: String,

    #[serde(skip_serializing, skip_deserializing)]
    pub length_sec: Option<f64>,

    pub infinit: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Storage {
    pub help_text: String,
    pub path: String,
    #[serde(skip_serializing, skip_deserializing)]
    pub paths: Vec<String>,
    pub filler_clip: String,
    pub extensions: Vec<String>,
    pub shuffle: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Out {
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

fn default_tracks() -> i32 {
    1
}

fn default_channels() -> u8 {
    2
}

impl PlayoutConfig {
    /// Read config from YAML file, and set some extra config values.
    pub fn new(cfg_path: Option<String>) -> Self {
        let mut config_path = PathBuf::from("/etc/ffplayout/ffplayout.yml");

        if let Some(cfg) = cfg_path {
            config_path = PathBuf::from(cfg);
        }

        if !config_path.is_file() {
            if Path::new("./assets/ffplayout.yml").is_file() {
                config_path = PathBuf::from("./assets/ffplayout.yml")
            } else if let Some(p) = env::current_exe().ok().as_ref().and_then(|op| op.parent()) {
                config_path = p.join("ffplayout.yml")
            };
        }

        let f = match File::open(&config_path) {
            Ok(file) => file,
            Err(_) => {
                println!(
                    "{config_path:?} doesn't exists!\nPut \"ffplayout.yml\" in \"/etc/playout/\" or beside the executable!"
                );
                process::exit(1);
            }
        };

        let mut config: PlayoutConfig =
            serde_yaml::from_reader(f).expect("Could not read config file.");
        config.general.generate = None;
        config.general.stat_file = home_dir()
            .unwrap_or_else(env::temp_dir)
            .join(".ffp_status")
            .display()
            .to_string();

        if config.logging.ingest_level.is_none() {
            config.logging.ingest_level = Some(config.logging.ffmpeg_level.clone())
        }

        config.playlist.start_sec = Some(time_to_sec(&config.playlist.day_start));

        if config.playlist.length.contains(':') {
            config.playlist.length_sec = Some(time_to_sec(&config.playlist.length));
        } else {
            config.playlist.length_sec = Some(86400.0);
        }

        if config.processing.add_logo && !Path::new(&config.processing.logo).is_file() {
            config.processing.add_logo = false;
        }

        config.processing.logo_scale = config
            .processing
            .logo_scale
            .trim_end_matches('~')
            .to_string();

        if config.processing.audio_tracks < 1 {
            config.processing.audio_tracks = 1
        }

        let bitrate = format!(
            "{}k",
            config.processing.width * config.processing.height / 16
        );

        let buff_size = format!(
            "{}k",
            (config.processing.width * config.processing.height / 16) / 2
        );

        let mut process_cmd = vec_strings![];

        if config.processing.audio_only {
            process_cmd.append(&mut vec_strings!["-vn"]);
        } else {
            process_cmd.append(&mut vec_strings![
                "-pix_fmt",
                "yuv420p",
                "-r",
                &config.processing.fps,
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

        process_cmd.append(&mut pre_audio_codec(
            &config.processing.custom_filter,
            &config.ingest.custom_filter,
        ));
        process_cmd.append(&mut vec_strings![
            "-ar",
            "48000",
            "-ac",
            config.processing.audio_channels,
            "-f",
            "mpegts",
            "-"
        ]);

        config.processing.cmd = Some(process_cmd);

        config.ingest.input_cmd = split(config.ingest.input_param.as_str());

        config.out.output_count = 1;
        config.out.output_filter = None;

        if config.out.mode == Null {
            config.out.output_cmd = Some(vec_strings!["-f", "null", "-"]);
        } else if let Some(mut cmd) = split(config.out.output_param.as_str()) {
            // get output count according to the var_stream_map value, or by counting output parameters
            if let Some(i) = cmd.clone().iter().position(|m| m == "-var_stream_map") {
                config.out.output_count = cmd[i + 1].split_whitespace().count();
            } else {
                config.out.output_count = cmd
                    .iter()
                    .enumerate()
                    .filter(|(i, p)| i > &0 && !p.starts_with('-') && !cmd[i - 1].starts_with('-'))
                    .count();
            }

            if let Some(i) = cmd.clone().iter().position(|r| r == "-filter_complex") {
                config.out.output_filter = Some(cmd[i + 1].clone());
                cmd.remove(i);
                cmd.remove(i);
            }

            config.out.output_cmd = Some(cmd);
        }

        // when text overlay without text_from_filename is on, turn also the RPC server on,
        // to get text messages from it
        if config.text.add_text && !config.text.text_from_filename {
            config.rpc_server.enable = true;
            config.text.zmq_stream_socket = free_tcp_socket(String::new());
            config.text.zmq_server_socket =
                free_tcp_socket(config.text.zmq_stream_socket.clone().unwrap_or_default());
            config.text.node_pos = Some(2);
        } else {
            config.text.zmq_stream_socket = None;
            config.text.zmq_server_socket = None;
            config.text.node_pos = None;
        }

        config
    }
}

impl Default for PlayoutConfig {
    fn default() -> Self {
        Self::new(None)
    }
}

/// When custom_filter contains loudnorm filter use a different audio encoder,
/// s302m has higher quality, but is experimental
/// and works not well together with the loudnorm filter.
fn pre_audio_codec(proc_filter: &str, ingest_filter: &str) -> Vec<String> {
    let mut codec = vec_strings!["-c:a", "s302m", "-strict", "-2", "-sample_fmt", "s16"];

    if proc_filter.contains("loudnorm") || ingest_filter.contains("loudnorm") {
        codec = vec_strings!["-c:a", "mp2", "-b:a", "384k"];
    }

    codec
}
