use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::utils::{is_close, Config, Program};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Filters {
    pub audio_chain: Option<String>,
    pub video_chain: Option<String>,
    pub audio_map: Option<String>,
    pub video_map: Option<String>,
}

impl Filters {
    pub fn new() -> Self {
        Filters {
            audio_chain: None,
            video_chain: None,
            audio_map: Some("0:a".to_string()),
            video_map: Some("0:v".to_string()),
        }
    }

    fn add_filter(&mut self, filter: String, codec_type: String) {
        match codec_type.as_str() {
            "audio" => match &self.audio_chain {
                Some(ac) => {
                    if filter.starts_with(";") || filter.starts_with("[") {
                        self.audio_chain = Some(format!("{}{}", ac, filter))
                    } else {
                        self.audio_chain = Some(format!("{},{}", ac, filter))
                    }
                }
                None => {
                    self.audio_chain = Some(filter);
                    self.audio_map = Some("[aout1]".to_string());
                }
            },
            "video" => match &self.video_chain {
                Some(vc) => {
                    if filter.starts_with(";") || filter.starts_with("[") {
                        self.video_chain = Some(format!("{}{}", vc, filter))
                    } else {
                        self.video_chain = Some(format!("{},{}", vc, filter))
                    }
                }
                None => {
                    self.video_chain = Some(filter);
                    self.video_map = Some("[vout1]".to_string());
                }
            },
            _ => (),
        }
    }
}

fn deinterlace(field_order: Option<String>, chain: &mut Filters) {
    if field_order.is_some() && field_order.unwrap() != "progressive".to_string() {
        chain.add_filter("yadif=0:-1:0".into(), "video".into())
    }
}

fn pad(aspect: f64, chain: &mut Filters, config: &Config) {
    if is_close(aspect, config.processing.aspect, 0.03) {
        if aspect < config.processing.aspect {
            chain.add_filter(
                format!(
                    "pad=ih*{}/{}/sar:ih:(ow-iw)/2:(oh-ih)/2",
                    config.processing.width, config.processing.height
                )
                .into(),
                "video".into(),
            )
        } else if aspect > config.processing.aspect {
            chain.add_filter(
                format!(
                    "pad=iw:iw*{}/{}/sar:(ow-iw)/2:(oh-ih)/2",
                    config.processing.width, config.processing.height
                )
                .into(),
                "video".into(),
            )
        }
    }
}

fn fps(fps: f64, chain: &mut Filters, config: &Config) {
    if fps != config.processing.fps {
        chain.add_filter(
            format!("fps={}", config.processing.fps).into(),
            "video".into(),
        )
    }
}

fn scale(width: i64, height: i64, aspect: f64, chain: &mut Filters, config: &Config) {
    if width != config.processing.width || height != config.processing.height {
        chain.add_filter(
            format!(
                "scale={}:{}",
                config.processing.width, config.processing.height
            )
            .into(),
            "video".into(),
        )
    }

    if is_close(aspect, config.processing.aspect, 0.03) {
        chain.add_filter(
            format!("setdar=dar={}", config.processing.aspect).into(),
            "video".into(),
        )
    }
}

fn fade(node: &mut Program, chain: &mut Filters, codec_type: String) {
    let mut t = "".to_string();

    if codec_type == "audio".to_string() {
        t = "a".to_string()
    }

    if node.seek > 0.0 {
        chain.add_filter(format!("{t}fade=in:st=0:d=0.5"), codec_type.clone())
    }

    if node.out != node.duration && node.out - node.seek - 1.0 > 0.0 {
        chain.add_filter(
            format!("{t}fade=out:st={}:d=1.0", (node.out - node.seek - 1.0)).into(),
            codec_type,
        )
    }
}

fn overlay(node: &mut Program, chain: &mut Filters, config: &Config, last_ad: bool, next_ad: bool) {
    if config.processing.add_logo
        && Path::new(&config.processing.logo).is_file()
        && node.category != "advertisement".to_string()
    {
        let opacity = format!(
            "format=rgba,colorchannelmixer=aa={}",
            config.processing.logo_opacity
        );
        let logo_loop = "loop=loop=-1:size=1:start=0";
        let mut logo_chain = format!("[v];movie={},{logo_loop},{opacity}", config.processing.logo);

        if last_ad {
            logo_chain.push_str(",fade=in:st=0:d=1.0:alpha=1")
        }

        if next_ad {
            logo_chain.push_str(
                format!(",fade=out:st={}:d=1.0:alpha=1", node.out - node.seek - 1.0).as_str(),
            )
        }

        logo_chain
            .push_str(format!("[l];[v][l]{}:shortest=1", config.processing.logo_filter).as_str());

        chain.add_filter(logo_chain, "video".into());
    }
}

pub fn filter_chains(node: &mut Program, config: &Config, last: bool, next: bool) -> Vec<String> {
    let mut filters = Filters::new();
    let probe = node.probe.clone();

    match probe {
        Some(p) => {
            // let a_stream = &p.audio_streams.unwrap()[0];
            let v_stream = &p.video_streams.unwrap()[0];
            let aspect_string = v_stream.display_aspect_ratio.clone().unwrap();
            let aspect_vec: Vec<&str> = aspect_string.split(':').collect();
            let w: f64 = aspect_vec[0].parse().unwrap();
            let h: f64 = aspect_vec[1].parse().unwrap();
            let source_aspect: f64 = w as f64 / h as f64;
            let frame_rate_vec: Vec<&str> = v_stream.r_frame_rate.split('/').collect();
            let rate: f64 = frame_rate_vec[0].parse().unwrap();
            let factor: f64 = frame_rate_vec[1].parse().unwrap();
            let frames_per_second: f64 = rate / factor;

            deinterlace(v_stream.field_order.clone(), &mut filters);
            pad(source_aspect, &mut filters, &config);
            fps(frames_per_second, &mut filters, &config);
            scale(
                v_stream.width.unwrap(),
                v_stream.height.unwrap(),
                source_aspect,
                &mut filters,
                &config,
            );
            fade(node, &mut filters, "audio".into());
            fade(node, &mut filters, "video".into());
            overlay(node, &mut filters, &config, last, next);
        }
        None => {
            println!("Clip has no media probe object. No filter applied!")
        }
    }

    let mut filter_cmd = vec!["-filter_complex".to_string()];
    let mut filter_str: String = "".to_string();
    let mut filter_map: Vec<String> = vec![];

    if filters.audio_chain.is_some() {
        filter_str.push_str(
            format!(
                "[0:a]{}{};",
                filters.audio_chain.unwrap(),
                filters.audio_map.clone().unwrap()
            )
            .as_str(),
        );
        filter_map.append(&mut vec!["-map".to_string(), filters.audio_map.unwrap()]);
    } else {
        filter_map.append(&mut vec!["-map".to_string(), "0:a".to_string()]);
    }

    if filters.video_chain.is_some() {
        filter_str.push_str(
            format!(
                "[0:v]{}{}",
                filters.video_chain.unwrap(),
                filters.video_map.clone().unwrap()
            )
            .as_str(),
        );
        filter_map.append(&mut vec!["-map".to_string(), filters.video_map.unwrap()]);
    } else {
        filter_map.append(&mut vec!["-map".to_string(), "0:v".to_string()]);
    }

    filter_cmd.push(filter_str);
    filter_cmd.append(&mut filter_map);

    filter_cmd
}
