use serde::{Deserialize, Serialize};
use serde_yaml::{self};
use std::path::Path;
// use regex::Regex;

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
    pub r#loop: bool,
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

pub fn read_yaml() -> Config {
    let mut config_path: String = "ffplayout.yml".to_string();

    if Path::new("/etc/ffplayout/ffplayout.yml").exists() {
        config_path = "/etc/ffplayout/ffplayout.yml".to_string();
    }

    let f = std::fs::File::open(config_path).expect("Could not open file.");
    let config: Config = serde_yaml::from_reader(f).expect("Could not read config file.");

    config
}
