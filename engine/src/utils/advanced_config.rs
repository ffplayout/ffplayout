use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_with::{serde_as, NoneAsEmptyString};
use shlex::split;
use sqlx::{Pool, Sqlite};
use tokio::io::AsyncReadExt;
use ts_rs::TS;

use crate::db::{handles, models::AdvancedConfiguration};
use crate::utils::ServiceError;

#[derive(Debug, Default, Serialize, Deserialize, Clone, TS)]
#[ts(export, export_to = "advanced_config.d.ts")]
pub struct AdvancedConfig {
    pub id: i32,
    pub name: Option<String>,
    pub decoder: DecoderConfig,
    pub encoder: EncoderConfig,
    pub filter: FilterConfig,
    pub ingest: IngestConfig,
}

#[serde_as]
#[derive(Debug, Default, Serialize, Deserialize, Clone, TS)]
#[ts(export, export_to = "advanced_config.d.ts")]
pub struct DecoderConfig {
    #[ts(type = "string")]
    #[serde_as(as = "NoneAsEmptyString")]
    pub input_param: Option<String>,
    #[ts(type = "string")]
    #[serde_as(as = "NoneAsEmptyString")]
    pub output_param: Option<String>,
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub input_cmd: Option<Vec<String>>,
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub output_cmd: Option<Vec<String>>,
}

#[serde_as]
#[derive(Debug, Default, Serialize, Deserialize, Clone, TS)]
#[ts(export, export_to = "advanced_config.d.ts")]
pub struct EncoderConfig {
    #[ts(type = "string")]
    #[serde_as(as = "NoneAsEmptyString")]
    pub input_param: Option<String>,
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub input_cmd: Option<Vec<String>>,
}

#[serde_as]
#[derive(Debug, Default, Serialize, Deserialize, Clone, TS)]
#[ts(export, export_to = "advanced_config.d.ts")]
pub struct IngestConfig {
    #[ts(type = "string")]
    #[serde_as(as = "NoneAsEmptyString")]
    pub input_param: Option<String>,
    #[ts(skip)]
    #[serde(skip_serializing, skip_deserializing)]
    pub input_cmd: Option<Vec<String>>,
}

#[serde_as]
#[derive(Debug, Default, Serialize, Deserialize, Clone, TS)]
#[ts(export, export_to = "advanced_config.d.ts")]
pub struct FilterConfig {
    #[ts(type = "string")]
    #[serde_as(as = "NoneAsEmptyString")]
    pub deinterlace: Option<String>,
    #[ts(type = "string")]
    #[serde_as(as = "NoneAsEmptyString")]
    pub pad_video: Option<String>,
    #[ts(type = "string")]
    #[serde_as(as = "NoneAsEmptyString")]
    pub fps: Option<String>,
    #[ts(type = "string")]
    #[serde_as(as = "NoneAsEmptyString")]
    pub scale: Option<String>,
    #[ts(type = "string")]
    #[serde_as(as = "NoneAsEmptyString")]
    pub set_dar: Option<String>,
    #[ts(type = "string")]
    #[serde_as(as = "NoneAsEmptyString")]
    pub fade_in: Option<String>,
    #[ts(type = "string")]
    #[serde_as(as = "NoneAsEmptyString")]
    pub fade_out: Option<String>,
    #[ts(type = "string")]
    #[serde_as(as = "NoneAsEmptyString")]
    pub logo: Option<String>,
    #[ts(type = "string")]
    #[serde_as(as = "NoneAsEmptyString")]
    pub overlay_logo_scale: Option<String>,
    #[ts(type = "string")]
    #[serde_as(as = "NoneAsEmptyString")]
    pub overlay_logo_fade_in: Option<String>,
    #[ts(type = "string")]
    #[serde_as(as = "NoneAsEmptyString")]
    pub overlay_logo_fade_out: Option<String>,
    #[ts(type = "string")]
    #[serde_as(as = "NoneAsEmptyString")]
    pub overlay_logo: Option<String>,
    #[ts(type = "string")]
    #[serde_as(as = "NoneAsEmptyString")]
    pub tpad: Option<String>,
    #[ts(type = "string")]
    #[serde_as(as = "NoneAsEmptyString")]
    pub drawtext_from_file: Option<String>,
    #[ts(type = "string")]
    #[serde_as(as = "NoneAsEmptyString")]
    pub drawtext_from_zmq: Option<String>,
    #[ts(type = "string")]
    #[serde_as(as = "NoneAsEmptyString")]
    pub aevalsrc: Option<String>,
    #[ts(type = "string")]
    #[serde_as(as = "NoneAsEmptyString")]
    pub afade_in: Option<String>,
    #[ts(type = "string")]
    #[serde_as(as = "NoneAsEmptyString")]
    pub afade_out: Option<String>,
    #[ts(type = "string")]
    #[serde_as(as = "NoneAsEmptyString")]
    pub apad: Option<String>,
    #[ts(type = "string")]
    #[serde_as(as = "NoneAsEmptyString")]
    pub volume: Option<String>,
    #[ts(type = "string")]
    #[serde_as(as = "NoneAsEmptyString")]
    pub split: Option<String>,
}

impl AdvancedConfig {
    pub fn new(config: AdvancedConfiguration) -> Self {
        Self {
            id: config.id,
            name: config.name,
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
                pad_video: config.filter_pad_video,
                fps: config.filter_fps,
                scale: config.filter_scale,
                set_dar: config.filter_set_dar,
                fade_in: config.filter_fade_in,
                fade_out: config.filter_fade_out,
                logo: config.filter_logo,
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

    pub async fn dump(pool: &Pool<Sqlite>, id: i32) -> Result<(), ServiceError> {
        for conf in handles::select_related_advanced_configuration(pool, id).await? {
            let config = Self::new(conf);
            let f_keys = [
                "deinterlace",
                "pad_scale_w",
                "pad_scale_h",
                "pad_video",
                "fps",
                "scale",
                "set_dar",
                "fade_in",
                "fade_out",
                "overlay_logo_scale",
                "overlay_logo_fade_in",
                "overlay_logo_fade_out",
                "overlay_logo",
                "tpad",
                "drawtext_from_file",
                "drawtext_from_zmq",
                "aevalsrc",
                "afade_in",
                "afade_out",
                "apad",
                "volume",
                "split",
            ];

            let toml_string = toml_edit::ser::to_string_pretty(&config)?;
            let mut doc = toml_string.parse::<toml_edit::DocumentMut>()?;

            if let Some(decoder) = doc.get_mut("decoder").and_then(|o| o.as_table_mut()) {
                decoder
                    .decor_mut()
                    .set_prefix("# Changing these settings is for advanced users only!\n# There will be no support or guarantee that it will be stable after changing them.\n\n");
            }

            if let Some(output_param) = doc
                .get_mut("decoder")
                .and_then(|d| d.get_mut("output_param"))
                .and_then(|o| o.as_value_mut())
            {
                output_param
                    .decor_mut()
                    .set_suffix(" # get also applied to ingest instance.");
            }

            if let Some(filter) = doc.get_mut("filter") {
                for key in &f_keys {
                    if let Some(item) = filter.get_mut(*key).and_then(|o| o.as_value_mut()) {
                        match *key {
                            "deinterlace" => item.decor_mut().set_suffix(" # yadif=0:-1:0"),
                            "pad_video" => item
                                .decor_mut()
                                .set_suffix(" # pad='ih*{}/{}:ih:(ow-iw)/2:(oh-ih)/2'"),
                            "fps" => item.decor_mut().set_suffix(" # fps={}"),
                            "scale" => item.decor_mut().set_suffix(" # scale={}:{}"),
                            "set_dar" => item.decor_mut().set_suffix(" # setdar=dar={}"),
                            "fade_in" => item.decor_mut().set_suffix(" # fade=in:st=0:d=0.5"),
                            "fade_out" => item.decor_mut().set_suffix(" # fade=out:st={}:d=1.0"),
                            "overlay_logo_scale" => item.decor_mut().set_suffix(" # scale={}"),
                            "overlay_logo_fade_in" => {
                                item.decor_mut().set_suffix(" # fade=in:st=0:d=1.0:alpha=1");
                            }
                            "overlay_logo_fade_out" => item
                                .decor_mut()
                                .set_suffix(" # fade=out:st={}:d=1.0:alpha=1"),
                            "overlay_logo" => {
                                item.decor_mut().set_suffix(" # overlay={}:shortest=1");
                            }
                            "tpad" => item
                                .decor_mut()
                                .set_suffix(" # tpad=stop_mode=add:stop_duration={}"),
                            "drawtext_from_file" => {
                                item.decor_mut().set_suffix(" # drawtext=text='{}':{}{}");
                            }
                            "drawtext_from_zmq" => item
                                .decor_mut()
                                .set_suffix(" # zmq=b=tcp\\\\://'{}',drawtext@dyntext={}"),
                            "aevalsrc" => item.decor_mut().set_suffix(
                                " # aevalsrc=0:channel_layout=stereo:duration={}:sample_rate=48000",
                            ),
                            "afade_in" => item.decor_mut().set_suffix(" # afade=in:st=0:d=0.5"),
                            "afade_out" => item.decor_mut().set_suffix(" # afade=out:st={}:d=1.0"),
                            "apad" => item.decor_mut().set_suffix(" # apad=whole_dur={}"),
                            "volume" => item.decor_mut().set_suffix(" # volume={}"),
                            "split" => item.decor_mut().set_suffix(" # split={}{}"),
                            _ => (),
                        }
                    }
                }
            };

            tokio::fs::write(
                &format!("advanced_{id}_{}.toml", config.id),
                doc.to_string(),
            )
            .await?;

            println!("Dump advanced config to: advanced_{id}_{}.toml", config.id);
        }

        Ok(())
    }

    pub async fn import(pool: &Pool<Sqlite>, id: i32, path: &Path) -> Result<(), ServiceError> {
        if path.is_file() {
            let mut file = tokio::fs::File::open(path).await?;
            let mut contents = String::new();
            file.read_to_string(&mut contents).await?;

            let config: Self = toml_edit::de::from_str(&contents).unwrap();

            handles::update_advanced_configuration(pool, id, config).await?;
        } else {
            return Err(ServiceError::BadRequest("Path not exists!".to_string()));
        }

        Ok(())
    }

    pub fn is_empty_filter(&self) -> bool {
        self.filter.aevalsrc.is_none()
            && self.filter.afade_in.is_none()
            && self.filter.afade_out.is_none()
            && self.filter.apad.is_none()
            && self.filter.deinterlace.is_none()
            && self.filter.drawtext_from_file.is_none()
            && self.filter.drawtext_from_zmq.is_none()
            && self.filter.fade_in.is_none()
            && self.filter.fade_out.is_none()
            && self.filter.fps.is_none()
            && self.filter.logo.is_none()
            && self.filter.overlay_logo.is_none()
            && self.filter.overlay_logo_fade_in.is_none()
            && self.filter.overlay_logo_fade_out.is_none()
            && self.filter.overlay_logo_scale.is_none()
            && self.filter.pad_video.is_none()
            && self.filter.scale.is_none()
            && self.filter.set_dar.is_none()
            && self.filter.split.is_none()
            && self.filter.tpad.is_none()
            && self.filter.volume.is_none()
    }
}
