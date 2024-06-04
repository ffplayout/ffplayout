use std::{fs::File, io::Read, path::PathBuf};

use serde::{Deserialize, Serialize};
use serde_with::{serde_as, NoneAsEmptyString};
use shlex::split;

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AdvancedConfig {
    pub decoder: DecoderConfig,
    pub encoder: EncoderConfig,
    pub filters: Filters,
    pub ingest: IngestConfig,
}

#[serde_as]
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct DecoderConfig {
    #[serde_as(as = "NoneAsEmptyString")]
    pub input_param: Option<String>,
    #[serde_as(as = "NoneAsEmptyString")]
    pub output_param: Option<String>,
    #[serde(skip_serializing, skip_deserializing)]
    pub input_cmd: Option<Vec<String>>,
    #[serde(skip_serializing, skip_deserializing)]
    pub output_cmd: Option<Vec<String>>,
}

#[serde_as]
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct EncoderConfig {
    #[serde_as(as = "NoneAsEmptyString")]
    pub input_param: Option<String>,
    #[serde(skip_serializing, skip_deserializing)]
    pub input_cmd: Option<Vec<String>>,
}

#[serde_as]
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct IngestConfig {
    #[serde_as(as = "NoneAsEmptyString")]
    pub input_param: Option<String>,
    #[serde(skip_serializing, skip_deserializing)]
    pub input_cmd: Option<Vec<String>>,
}

#[serde_as]
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct Filters {
    #[serde_as(as = "NoneAsEmptyString")]
    pub deinterlace: Option<String>,
    #[serde_as(as = "NoneAsEmptyString")]
    pub pad_scale_w: Option<String>,
    #[serde_as(as = "NoneAsEmptyString")]
    pub pad_scale_h: Option<String>,
    #[serde_as(as = "NoneAsEmptyString")]
    pub pad_video: Option<String>,
    #[serde_as(as = "NoneAsEmptyString")]
    pub fps: Option<String>,
    #[serde_as(as = "NoneAsEmptyString")]
    pub scale: Option<String>,
    #[serde_as(as = "NoneAsEmptyString")]
    pub set_dar: Option<String>,
    #[serde_as(as = "NoneAsEmptyString")]
    pub fade_in: Option<String>,
    #[serde_as(as = "NoneAsEmptyString")]
    pub fade_out: Option<String>,
    #[serde_as(as = "NoneAsEmptyString")]
    pub overlay_logo_scale: Option<String>,
    #[serde_as(as = "NoneAsEmptyString")]
    pub overlay_logo_fade_in: Option<String>,
    #[serde_as(as = "NoneAsEmptyString")]
    pub overlay_logo_fade_out: Option<String>,
    #[serde_as(as = "NoneAsEmptyString")]
    pub overlay_logo: Option<String>,
    #[serde_as(as = "NoneAsEmptyString")]
    pub tpad: Option<String>,
    #[serde_as(as = "NoneAsEmptyString")]
    pub drawtext_from_file: Option<String>,
    #[serde_as(as = "NoneAsEmptyString")]
    pub drawtext_from_zmq: Option<String>,
    #[serde_as(as = "NoneAsEmptyString")]
    pub aevalsrc: Option<String>,
    #[serde_as(as = "NoneAsEmptyString")]
    pub afade_in: Option<String>,
    #[serde_as(as = "NoneAsEmptyString")]
    pub afade_out: Option<String>,
    #[serde_as(as = "NoneAsEmptyString")]
    pub apad: Option<String>,
    #[serde_as(as = "NoneAsEmptyString")]
    pub volume: Option<String>,
    #[serde_as(as = "NoneAsEmptyString")]
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

            match toml_edit::de::from_str(&contents) {
                Ok(tm) => config = tm,
                Err(e) => eprintln!("Serialize advanced config file: {e}"),
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
