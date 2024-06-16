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
                deinterlace: config.filter_deinterlace,
                pad_scale_w: config.filter_pad_scale_w,
                pad_scale_h: config.filter_pad_scale_h,
                pad_video: config.filter_pad_video,
                fps: config.filter_fps,
                scale: config.filter_scale,
                set_dar: config.filter_set_dar,
                fade_in: config.filter_fade_in,
                fade_out: config.filter_fade_out,
                overlay_logo_scale: config.filter_overlay_logo_scale,
                overlay_logo_fade_in: config.filter_overlay_logo_fade_in,
                overlay_logo_fade_out: config.filter_overlay_logo_fade_out,
                overlay_logo: config.filter_overlay_logo,
                tpad: config.filter_tpad,
                drawtext_from_file: config.filter_drawtext_from_file,
                drawtext_from_zmq: config.filter_drawtext_from_zmq,
                aevalsrc: config.filter_aevalsrc,
                afade_in: config.filter_afade_in,
                afade_out: config.filter_afade_out,
                apad: config.filter_apad,
                volume: config.filter_volume,
                split: config.filter_split,
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
