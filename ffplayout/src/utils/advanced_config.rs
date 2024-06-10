use serde::{Deserialize, Serialize};
use shlex::split;

use crate::db::models::AdvancedConfiguration;

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AdvancedConfig {
    pub decoder: DecoderConfig,
    pub encoder: EncoderConfig,
    pub filter: FilterConfig,
    pub ingest: IngestConfig,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct DecoderConfig {
    pub input_param: Option<String>,
    pub output_param: Option<String>,
    pub input_cmd: Option<Vec<String>>,
    pub output_cmd: Option<Vec<String>>,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct EncoderConfig {
    pub input_param: Option<String>,
    pub input_cmd: Option<Vec<String>>,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct IngestConfig {
    pub input_param: Option<String>,
    pub input_cmd: Option<Vec<String>>,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct FilterConfig {
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
    pub fn new(config: AdvancedConfiguration) -> Self {
        Self {
            decoder: DecoderConfig {
                input_param: config.decoder_input_param.clone(),
                output_param: config.decoder_output_param.clone(),
                input_cmd: match config.decoder_input_param {
                    Some(input_param) => split(&input_param),
                    None => None,
                },
                output_cmd: match config.decoder_output_param {
                    Some(output_param) => split(&output_param),
                    None => None,
                },
            },
            encoder: EncoderConfig {
                input_param: config.encoder_input_param.clone(),
                input_cmd: match config.encoder_input_param {
                    Some(input_param) => split(&input_param),
                    None => None,
                },
            },
            filter: FilterConfig {
                deinterlace: config.deinterlace,
                pad_scale_w: config.pad_scale_w,
                pad_scale_h: config.pad_scale_h,
                pad_video: config.pad_video,
                fps: config.fps,
                scale: config.scale,
                set_dar: config.set_dar,
                fade_in: config.fade_in,
                fade_out: config.fade_out,
                overlay_logo_scale: config.overlay_logo_scale,
                overlay_logo_fade_in: config.overlay_logo_fade_in,
                overlay_logo_fade_out: config.overlay_logo_fade_out,
                overlay_logo: config.overlay_logo,
                tpad: config.tpad,
                drawtext_from_file: config.drawtext_from_file,
                drawtext_from_zmq: config.drawtext_from_zmq,
                aevalsrc: config.aevalsrc,
                afade_in: config.afade_in,
                afade_out: config.afade_out,
                apad: config.apad,
                volume: config.volume,
                split: config.split,
            },
            ingest: IngestConfig {
                input_param: config.ingest_input_param.clone(),
                input_cmd: match config.ingest_input_param {
                    Some(input_param) => split(&input_param),
                    None => None,
                },
            },
        }
    }
}
