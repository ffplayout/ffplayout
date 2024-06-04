use std::{fs::File, io::Read, path::PathBuf};

use serde::{Deserialize, Serialize};
use shlex::split;

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AdvancedConfig {
    pub decoder: DecoderConfig,
    pub encoder: EncoderConfig,
    pub ingest: IngestConfig,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct DecoderConfig {
    pub input_param: Option<String>,
    pub output_param: Option<String>,
    pub filters: Filters,
    #[serde(skip_serializing, skip_deserializing)]
    pub input_cmd: Option<Vec<String>>,
    #[serde(skip_serializing, skip_deserializing)]
    pub output_cmd: Option<Vec<String>>,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct EncoderConfig {
    pub input_param: Option<String>,
    #[serde(skip_serializing, skip_deserializing)]
    pub input_cmd: Option<Vec<String>>,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct IngestConfig {
    pub input_param: Option<String>,
    #[serde(skip_serializing, skip_deserializing)]
    pub input_cmd: Option<Vec<String>>,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct Filters {
    pub deinterlace: Option<String>,
    pub pad_scale_w: Option<String>,
    pub pad_scale_h: Option<String>,
    pub pad_video: Option<String>,
    pub fps: Option<String>,
    pub scale: Option<String>,
    pub set_dar: Option<String>,
    pub fade_in: Option<String>,
    pub fade_out: Option<String>,
    pub overlay_logo_scale: Option<String>,
    pub overlay_logo_fade_in: Option<String>,
    pub overlay_logo_fade_out: Option<String>,
    pub overlay_logo: Option<String>,
    pub tpad: Option<String>,
    pub drawtext_from_file: Option<String>,
    pub drawtext_from_zmq: Option<String>,
    pub aevalsrc: Option<String>,
    pub afade_in: Option<String>,
    pub afade_out: Option<String>,
    pub apad: Option<String>,
    pub volume: Option<String>,
    pub split: Option<String>,
}

impl AdvancedConfig {
    pub fn new(cfg_path: PathBuf) -> Self {
        let mut config: AdvancedConfig = Default::default();

        if let Ok(mut file) = File::open(cfg_path) {
            let mut contents = String::new();

            if let Err(e) = file.read_to_string(&mut contents) {
                eprintln!("Read advanced config file: {e}")
            };

            if let Ok(tm) = toml_edit::de::from_str(&contents) {
                config = tm
            };

            if let Some(input_parm) = &config.decoder.input_param {
                config.decoder.input_cmd = split(input_parm);
            }

            if let Some(output_param) = &config.decoder.output_param {
                config.decoder.output_cmd = split(output_param);
            }

            if let Some(input_param) = &config.encoder.input_param {
                config.encoder.input_cmd = split(input_param);
            }

            if let Some(input_param) = &config.ingest.input_param {
                config.ingest.input_cmd = split(input_param);
            }
        };

        config
    }
}
