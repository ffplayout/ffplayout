use serde::{Deserialize, Serialize};
use serde_yaml::{self};
use std::{fs::File, path::Path, process};
// use regex::Regex;

use crate::utils::get_args;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub general: General,
    pub mail: Mail,
    pub logging: Logging,
    pub processing: Processing,
    pub ingest: Ingest,
    pub playlist: Playlist,
    pub storage: Storage,
    pub text: Text,
    pub out: Out,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct General {
    pub stop_threshold: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Mail {
    pub subject: String,
    pub smtp_server: String,
    pub smtp_port: u32,
    pub sender_addr: String,
    pub sender_pass: String,
    pub recipient: String,
    pub mail_level: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Logging {
    pub log_to_file: bool,
    pub backup_count: u32,
    pub log_path: String,
    pub log_level: String,
    pub ffmpeg_level: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Processing {
    pub mode: String,
    pub width: u32,
    pub height: u32,
    pub aspect: f32,
    pub fps: u32,
    pub add_logo: bool,
    pub logo: String,
    pub logo_scale: String,
    pub logo_opacity: f32,
    pub logo_filter: String,
    pub add_loudnorm: bool,
    pub loud_i: f32,
    pub loud_tp: f32,
    pub loud_lra: f32,
    pub output_count: u32,
    pub volume: String,
    pub settings: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Ingest {
    pub enable: bool,
    pub stream_input: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Playlist {
    pub path: String,
    pub day_start: String,
    pub length: String,
    pub infinit: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Storage {
    pub path: String,
    pub filler_clip: String,
    pub extensions: Vec<String>,
    pub shuffle: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Text {
    pub add_text: bool,
    pub over_pre: bool,
    pub bind_address: String,
    pub text_from_filename: bool,
    pub style: String,
    pub regex: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Out {
    pub mode: String,
    pub preview: bool,
    pub preview_param: Vec<String>,
    pub stream_param: Vec<String>,
}

pub fn get_config() -> Config {
    let args = get_args();
    let mut config_path: String = "ffplayout.yml".to_string();

    if args.config.is_some() {
        config_path = args.config.unwrap();
    } else if Path::new("/etc/ffplayout/ffplayout.yml").is_file() {
        config_path = "/etc/ffplayout/ffplayout.yml".to_string();
    }

    if !Path::new(&config_path).is_file() {
        println!(
            "{} '{config_path}'\n{}",
            "ffplayout config doesn't exists:",
            "Put 'ffplayout.yml' in '/etc/playout/' or beside the executable!"
        );
        process::exit(0x0100);
    }

    let f = File::open(config_path).expect("Could not open config file.");
    let mut config: Config = serde_yaml::from_reader(f).expect("Could not read config file.");

    let fps = config.processing.fps.to_string();
    let bitrate = config.processing.width * config.processing.height / 10;

    let settings: Vec<String> = vec![
        "-pix_fmt",
        "yuv420p",
        "-r",
        &fps,
        "-c:v",
        "mpeg2video",
        "-g",
        "1",
        "-b:v",
        format!("{}k", bitrate).as_str(),
        "-minrate",
        format!("{}k", bitrate).as_str(),
        "-maxrate",
        format!("{}k", bitrate).as_str(),
        "-bufsize",
        format!("{}k", bitrate / 2).as_str(),
        "-c:a",
        "s302m",
        "-strict",
        "-2",
        "-ar",
        "48000",
        "-ac",
        "2",
        "-f",
        "mpegts",
        "-",
    ].iter().map(|&s|s.into()).collect();

    config.processing.settings = Some(settings);

    if args.playlist.is_some() {
        config.playlist.path = args.playlist.unwrap();
    }

    if args.play_mode.is_some() {
        config.processing.mode = args.play_mode.unwrap();
    }

    if args.folder.is_some() {
        config.storage.path = args.folder.unwrap();
    }

    if args.start.is_some() {
        config.playlist.day_start = args.start.unwrap();
    }

    if args.length.is_some() {
        config.playlist.length = args.length.unwrap();
    }

    if args.infinit {
        config.playlist.infinit = args.infinit;
    }

    if args.output.is_some() {
        config.out.mode = args.output.unwrap();
    }

    if args.volume.is_some() {
        config.processing.volume = args.volume.unwrap();
    }

    config
}
