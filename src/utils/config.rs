use std::{
    env,
    fs::File,
    path::{Path, PathBuf},
    process,
};

use serde::{Deserialize, Serialize};
use shlex::split;

use crate::utils::{free_tcp_socket, time_to_sec, Args};
use crate::vec_strings;

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
    pub log_level: String,
    pub ffmpeg_level: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Processing {
    pub help_text: String,
    pub mode: String,
    pub width: i64,
    pub height: i64,
    pub aspect: f64,
    pub fps: f64,
    pub add_logo: bool,
    pub logo: String,
    pub logo_scale: String,
    pub logo_opacity: f32,
    pub logo_filter: String,
    pub add_loudnorm: bool,
    pub loudnorm_ingest: bool,
    pub loud_i: f32,
    pub loud_tp: f32,
    pub loud_lra: f32,
    pub volume: f64,

    #[serde(skip_serializing, skip_deserializing)]
    pub settings: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Ingest {
    pub help_text: String,
    pub enable: bool,
    input_param: String,

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
    pub filler_clip: String,
    pub extensions: Vec<String>,
    pub shuffle: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Text {
    pub help_text: String,
    pub add_text: bool,

    #[serde(skip_serializing, skip_deserializing)]
    pub bind_address: Option<String>,

    #[serde(skip_serializing, skip_deserializing)]
    pub node_pos: Option<usize>,

    pub fontfile: String,
    pub text_from_filename: bool,
    pub style: String,
    pub regex: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Out {
    pub help_text: String,
    pub mode: String,
    pub preview: bool,
    pub preview_param: String,

    #[serde(skip_serializing, skip_deserializing)]
    pub preview_cmd: Option<Vec<String>>,

    pub output_param: String,

    #[serde(skip_serializing, skip_deserializing)]
    pub output_cmd: Option<Vec<String>>,
}

impl PlayoutConfig {
    /// Read config from YAML file, and set some extra config values.
    pub fn new(args: Option<Args>) -> Self {
        let mut config_path = PathBuf::from("/etc/ffplayout/ffplayout.yml");

        if let Some(cfg) = args.clone().and_then(|a| a.config) {
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
                process::exit(0x0100);
            }
        };

        let mut config: PlayoutConfig =
            serde_yaml::from_reader(f).expect("Could not read config file.");
        config.general.generate = None;
        config.general.stat_file = env::temp_dir()
            .join("ffplayout_status.json")
            .display()
            .to_string();
        let bitrate = format!(
            "{}k",
            config.processing.width * config.processing.height / 10
        );
        let buf_size = format!(
            "{}k",
            (config.processing.width * config.processing.height / 10) / 2
        );

        config.playlist.start_sec = Some(time_to_sec(&config.playlist.day_start));

        if config.playlist.length.contains(':') {
            config.playlist.length_sec = Some(time_to_sec(&config.playlist.length));
        } else {
            config.playlist.length_sec = Some(86400.0);
        }

        // We set the decoder settings here, so we only define them ones.
        let mut settings = vec_strings![
            "-pix_fmt",
            "yuv420p",
            "-r",
            &config.processing.fps.to_string(),
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
            &buf_size
        ];

        settings.append(&mut pre_audio_codec(config.processing.add_loudnorm));
        settings.append(&mut vec_strings![
            "-ar", "48000", "-ac", "2", "-f", "mpegts", "-"
        ]);

        config.processing.settings = Some(settings);

        config.ingest.input_cmd = split(config.ingest.input_param.as_str());
        config.out.preview_cmd = split(config.out.preview_param.as_str());
        config.out.output_cmd = split(config.out.output_param.as_str());

        // when text overlay without text_from_filename is on, turn also the RPC server on,
        // to get text messages from it
        if config.text.add_text && !config.text.text_from_filename {
            config.rpc_server.enable = true;
            config.text.bind_address = free_tcp_socket();
            config.text.node_pos = Some(2);
        } else {
            config.text.bind_address = None;
            config.text.node_pos = None;
        }

        // Read command line arguments, and override the config with them.

        if let Some(arg) = args {
            if let Some(gen) = arg.generate {
                config.general.generate = Some(gen);
            }

            if let Some(log_path) = arg.log {
                if Path::new(&log_path).is_dir() {
                    config.logging.log_to_file = true;
                }
                config.logging.log_path = log_path;
            }

            if let Some(playlist) = arg.playlist {
                config.playlist.path = playlist;
            }

            if let Some(mode) = arg.play_mode {
                config.processing.mode = mode;
            }

            if let Some(folder) = arg.folder {
                config.storage.path = folder;
                config.processing.mode = "folder".into();
            }

            if let Some(start) = arg.start {
                config.playlist.day_start = start.clone();
                config.playlist.start_sec = Some(time_to_sec(&start));
            }

            if let Some(length) = arg.length {
                config.playlist.length = length.clone();

                if length.contains(':') {
                    config.playlist.length_sec = Some(time_to_sec(&length));
                } else {
                    config.playlist.length_sec = Some(86400.0);
                }
            }

            if arg.infinit {
                config.playlist.infinit = arg.infinit;
            }

            if let Some(output) = arg.output {
                config.out.mode = output;
            }

            if let Some(volume) = arg.volume {
                config.processing.volume = volume;
            }
        }

        config
    }
}

impl Default for PlayoutConfig {
    fn default() -> Self {
        Self::new(None)
    }
}

/// When add_loudnorm is False we use a different audio encoder,
/// s302m has higher quality, but is experimental
/// and works not well together with the loudnorm filter.
fn pre_audio_codec(add_loudnorm: bool) -> Vec<String> {
    let mut codec = vec_strings!["-c:a", "s302m", "-strict", "-2"];

    if add_loudnorm {
        codec = vec_strings!["-c:a", "mp2", "-b:a", "384k"];
    }

    codec
}
