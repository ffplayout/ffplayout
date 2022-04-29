use std::{
    env,
    fs::File,
    path::{Path, PathBuf},
    process,
};

use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use serde_yaml::{self};
use shlex::split;

use crate::utils::{get_args, time_to_sec};

/// Global Config
///
/// This we init ones, when ffplayout is starting and use them globally in the hole program.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GlobalConfig {
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
    pub stop_threshold: f64,
    pub generate: Option<Vec<String>>,

    #[serde(skip_serializing, skip_deserializing)]
    pub stat_file: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RpcServer {
    pub enable: bool,
    pub address: String,
    pub authorization: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Mail {
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
    pub loud_i: f32,
    pub loud_tp: f32,
    pub loud_lra: f32,
    pub volume: f64,
    pub settings: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Ingest {
    pub enable: bool,
    input_param: String,
    pub input_cmd: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Playlist {
    pub path: String,
    pub day_start: String,
    pub start_sec: Option<f64>,
    pub length: String,
    pub length_sec: Option<f64>,
    pub infinit: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Storage {
    pub path: String,
    pub filler_clip: String,
    pub extensions: Vec<String>,
    pub shuffle: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Text {
    pub add_text: bool,
    pub over_pre: bool,
    pub bind_address: String,
    pub fontfile: String,
    pub text_from_filename: bool,
    pub style: String,
    pub regex: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Out {
    pub mode: String,
    pub preview: bool,
    preview_param: String,
    pub preview_cmd: Option<Vec<String>>,
    output_param: String,
    pub output_cmd: Option<Vec<String>>,
}

impl GlobalConfig {
    /// Read config from YAML file, and set some extra config values.
    fn new() -> Self {
        let args = get_args();
        let mut config_path = match env::current_exe() {
            Ok(path) => path.parent().unwrap().join("ffplayout.yml"),
            Err(_) => PathBuf::from("./ffplayout.yml"),
        };

        if let Some(cfg) = args.config {
            config_path = PathBuf::from(cfg);
        } else if Path::new("/etc/ffplayout/ffplayout.yml").is_file() {
            config_path = PathBuf::from("/etc/ffplayout/ffplayout.yml");
        }

        let f = match File::open(&config_path) {
            Ok(file) => file,
            Err(err) => {
                println!(
                    "{config_path:?} doesn't exists!\nPut \"ffplayout.yml\" in \"/etc/playout/\" or beside the executable!\n\nSystem error: {err}"
                );
                process::exit(0x0100);
            }
        };

        let mut config: GlobalConfig =
            serde_yaml::from_reader(f).expect("Could not read config file.");
        config.general.generate = None;
        config.general.stat_file = env::temp_dir()
            .join("ffplayout_status.json")
            .display()
            .to_string();
        let fps = config.processing.fps.to_string();
        let bitrate = config.processing.width * config.processing.height / 10;
        config.playlist.start_sec = Some(time_to_sec(&config.playlist.day_start));

        if config.playlist.length.contains(':') {
            config.playlist.length_sec = Some(time_to_sec(&config.playlist.length));
        } else {
            config.playlist.length_sec = Some(86400.0);
        }

        // We set the decoder settings here, so we only define them ones.
        let mut settings: Vec<String> = vec![
            "-pix_fmt",
            "yuv420p",
            "-r",
            &fps,
            "-c:v",
            "mpeg2video",
            "-g",
            "1",
            "-b:v",
            format!("{bitrate}k").as_str(),
            "-minrate",
            format!("{bitrate}k").as_str(),
            "-maxrate",
            format!("{bitrate}k").as_str(),
            "-bufsize",
            format!("{}k", bitrate / 2).as_str(),
        ]
        .iter()
        .map(|&s| s.into())
        .collect();

        settings.append(&mut pre_audio_codec(config.processing.add_loudnorm));
        settings.append(
            &mut vec!["-ar", "48000", "-ac", "2", "-f", "mpegts", "-"]
                .iter()
                .map(|&s| s.into())
                .collect(),
        );

        config.processing.settings = Some(settings);

        config.ingest.input_cmd = split(config.ingest.input_param.as_str());
        config.out.preview_cmd = split(config.out.preview_param.as_str());
        config.out.output_cmd = split(config.out.output_param.as_str());

        // Read command line arguments, and override the config with them.

        if let Some(gen) = args.generate {
            config.general.generate = Some(gen);
        }

        if let Some(log_path) = args.log {
            config.logging.log_path = log_path;
        }

        if let Some(playlist) = args.playlist {
            config.playlist.path = playlist;
        }

        if let Some(mode) = args.play_mode {
            config.processing.mode = mode;
        }

        if let Some(folder) = args.folder {
            config.storage.path = folder;
        }

        if let Some(start) = args.start {
            config.playlist.day_start = start.clone();
            config.playlist.start_sec = Some(time_to_sec(&start));
        }

        if let Some(length) = args.length {
            config.playlist.length = length.clone();

            if length.contains(':') {
                config.playlist.length_sec = Some(time_to_sec(&length));
            } else {
                config.playlist.length_sec = Some(86400.0);
            }
        }

        if args.infinit {
            config.playlist.infinit = args.infinit;
        }

        if let Some(output) = args.output {
            config.out.mode = output;
        }

        if let Some(volume) = args.volume {
            config.processing.volume = volume;
        }

        config
    }

    pub fn global() -> &'static GlobalConfig {
        INSTANCE.get().expect("Config is not initialized")
    }
}

static INSTANCE: OnceCell<GlobalConfig> = OnceCell::new();

fn pre_audio_codec(add_loudnorm: bool) -> Vec<String> {
    // when add_loudnorm is False we use a different audio encoder,
    // s302m has higher quality, but is experimental
    // and works not well together with the loudnorm filter

    let mut codec = vec!["-c:a", "s302m", "-strict", "-2"];

    if add_loudnorm {
        codec = vec!["-c:a", "mp2", "-b:a", "384k"];
    }

    codec.iter().map(|&s| s.into()).collect()
}

pub fn init_config() {
    let config = GlobalConfig::new();
    INSTANCE.set(config).unwrap();
}
