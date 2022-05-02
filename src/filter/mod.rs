use std::path::Path;

use simplelog::*;

pub mod a_loudnorm;
pub mod ingest_filter;
pub mod v_drawtext;
pub mod v_overlay;

use crate::utils::{get_delta, is_close, GlobalConfig, Media};

#[derive(Debug, Clone)]
struct Filters {
    audio_chain: Option<String>,
    video_chain: Option<String>,
    audio_map: Option<String>,
    video_map: Option<String>,
}

impl Filters {
    fn new() -> Self {
        Filters {
            audio_chain: None,
            video_chain: None,
            audio_map: Some("0:a".to_string()),
            video_map: Some("0:v".to_string()),
        }
    }

    fn add_filter(&mut self, filter: &str, codec_type: &str) {
        match codec_type {
            "audio" => match &self.audio_chain {
                Some(ac) => {
                    if filter.starts_with(';') || filter.starts_with('[') {
                        self.audio_chain = Some(format!("{ac}{filter}"))
                    } else {
                        self.audio_chain = Some(format!("{ac},{filter}"))
                    }
                }
                None => {
                    if filter.contains("aevalsrc") || filter.contains("anoisesrc") {
                        self.audio_chain = Some(filter.to_string());
                    } else {
                        self.audio_chain =
                            Some(format!("[{}]{filter}", self.audio_map.clone().unwrap()));
                    }
                    self.audio_map = Some("[aout1]".to_string());
                }
            },
            "video" => match &self.video_chain {
                Some(vc) => {
                    if filter.starts_with(';') || filter.starts_with('[') {
                        self.video_chain = Some(format!("{vc}{filter}"))
                    } else {
                        self.video_chain = Some(format!("{vc},{filter}"))
                    }
                }
                None => {
                    self.video_chain = Some(format!("[0:v]{filter}"));
                    self.video_map = Some("[vout1]".to_string());
                }
            },
            _ => (),
        }
    }
}

fn deinterlace(field_order: &Option<String>, chain: &mut Filters) {
    if let Some(order) = field_order {
        if order != "progressive" {
            chain.add_filter("yadif=0:-1:0", "video")
        }
    }
}

fn pad(aspect: f64, chain: &mut Filters, config: &GlobalConfig) {
    if !is_close(aspect, config.processing.aspect, 0.03) {
        if aspect < config.processing.aspect {
            chain.add_filter(
                &format!(
                    "pad=ih*{}/{}/sar:ih:(ow-iw)/2:(oh-ih)/2",
                    config.processing.width, config.processing.height
                ),
                "video",
            )
        } else if aspect > config.processing.aspect {
            chain.add_filter(
                &format!(
                    "pad=iw:iw*{}/{}/sar:(ow-iw)/2:(oh-ih)/2",
                    config.processing.width, config.processing.height
                ),
                "video",
            )
        }
    }
}

fn fps(fps: f64, chain: &mut Filters, config: &GlobalConfig) {
    if fps != config.processing.fps {
        chain.add_filter(&format!("fps={}", config.processing.fps), "video")
    }
}

fn scale(width: i64, height: i64, aspect: f64, chain: &mut Filters, config: &GlobalConfig) {
    if width != config.processing.width || height != config.processing.height {
        chain.add_filter(
            &format!(
                "scale={}:{}",
                config.processing.width, config.processing.height
            ),
            "video",
        )
    }

    if !is_close(aspect, config.processing.aspect, 0.03) {
        chain.add_filter(&format!("setdar=dar={}", config.processing.aspect), "video")
    }
}

fn fade(node: &mut Media, chain: &mut Filters, codec_type: &str) {
    let mut t = "";

    if codec_type == "audio" {
        t = "a"
    }

    if node.seek > 0.0 {
        chain.add_filter(&format!("{t}fade=in:st=0:d=0.5"), codec_type)
    }

    if node.out != node.duration && node.out - node.seek - 1.0 > 0.0 {
        chain.add_filter(
            &format!("{t}fade=out:st={}:d=1.0", (node.out - node.seek - 1.0)),
            codec_type,
        )
    }
}

fn overlay(node: &mut Media, chain: &mut Filters, config: &GlobalConfig) {
    if config.processing.add_logo
        && Path::new(&config.processing.logo).is_file()
        && &node.category.clone().unwrap_or_default() != "advertisement"
    {
        let mut logo_chain = v_overlay::filter_node(config, false);

        if node.last_ad.unwrap() {
            logo_chain.push_str(",fade=in:st=0:d=1.0:alpha=1")
        }

        if node.next_ad.unwrap() {
            logo_chain.push_str(
                format!(",fade=out:st={}:d=1.0:alpha=1", node.out - node.seek - 1.0).as_str(),
            )
        }

        logo_chain
            .push_str(format!("[l];[v][l]{}:shortest=1", config.processing.logo_filter).as_str());

        chain.add_filter(&logo_chain, "video");
    }
}

fn extend_video(node: &mut Media, chain: &mut Filters) {
    let video_streams = node.probe.clone().unwrap().video_streams.unwrap();
    if !video_streams.is_empty() {
        if let Some(duration) = &video_streams[0].duration {
            let duration_float = duration.clone().parse::<f64>().unwrap();

            if node.out - node.seek > duration_float - node.seek + 0.1 {
                chain.add_filter(
                    &format!(
                        "tpad=stop_mode=add:stop_duration={}",
                        (node.out - node.seek) - (duration_float - node.seek)
                    ),
                    "video",
                )
            }
        }
    }
}

fn add_text(node: &mut Media, chain: &mut Filters, config: &GlobalConfig) {
    // add drawtext filter for lower thirds messages

    if config.text.add_text && config.text.over_pre {
        let filter = v_drawtext::filter_node(node);

        chain.add_filter(&filter, "video");

        if let Some(filters) = &chain.video_chain {
            for (i, f) in filters.split(',').enumerate() {
                if f.contains("drawtext") && !config.text.text_from_filename {
                    debug!("drawtext node is on index: <yellow>{i}</>");
                    break;
                }
            }
        }
    }
}

fn add_audio(node: &mut Media, chain: &mut Filters) {
    let audio_streams = node.probe.clone().unwrap().audio_streams.unwrap();
    if audio_streams.is_empty() {
        warn!("Clip: '{}' has no audio!", node.source);
        let audio = format!(
            "aevalsrc=0:channel_layout=stereo:duration={}:sample_rate=48000",
            node.out - node.seek
        );
        chain.add_filter(&audio, "audio");
    }
}

fn extend_audio(node: &mut Media, chain: &mut Filters) {
    let audio_streams = node.probe.clone().unwrap().audio_streams.unwrap();
    if !audio_streams.is_empty() {
        if let Some(duration) = &audio_streams[0].duration {
            let duration_float = duration.clone().parse::<f64>().unwrap();

            if node.out - node.seek > duration_float - node.seek + 0.1 {
                chain.add_filter(&format!("apad=whole_dur={}", node.out - node.seek), "audio")
            }
        }
    }
}

/// Add single pass loudnorm filter to audio line.
fn add_loudnorm(node: &mut Media, chain: &mut Filters, config: &GlobalConfig) {
    if node.probe.is_some()
        && !node
            .probe
            .clone()
            .unwrap()
            .audio_streams
            .unwrap()
            .is_empty()
        && config.processing.add_loudnorm
    {
        let loud_filter = a_loudnorm::filter_node(config);
        chain.add_filter(&loud_filter, "audio");
    }
}

fn audio_volume(chain: &mut Filters, config: &GlobalConfig) {
    if config.processing.volume != 1.0 {
        chain.add_filter(&format!("volume={}", config.processing.volume), "audio")
    }
}

fn aspect_calc(aspect_string: &Option<String>, config: &GlobalConfig) -> f64 {
    let mut source_aspect = config.processing.aspect;

    if let Some(aspect) = aspect_string {
        let aspect_vec: Vec<&str> = aspect.split(':').collect();
        let w: f64 = aspect_vec[0].parse().unwrap();
        let h: f64 = aspect_vec[1].parse().unwrap();
        source_aspect = w as f64 / h as f64;
    }

    source_aspect
}

fn fps_calc(r_frame_rate: &str) -> f64 {
    let frame_rate_vec: Vec<&str> = r_frame_rate.split('/').collect();
    let rate: f64 = frame_rate_vec[0].parse().unwrap();
    let factor: f64 = frame_rate_vec[1].parse().unwrap();
    let fps: f64 = rate / factor;

    fps
}

fn realtime_filter(node: &mut Media, chain: &mut Filters, config: &GlobalConfig, codec_type: &str) {
    // this realtime filter is important for HLS output to stay in sync

    let mut t = "";

    if codec_type == "audio" {
        t = "a"
    }

    if &config.out.mode.to_lowercase() == "hls" {
        let mut speed_filter = format!("{t}realtime=speed=1");
        let (delta, _) = get_delta(&node.begin.unwrap());
        let duration = node.out - node.seek;

        if delta < 0.0 {
            let speed = duration / (duration + delta);

            if speed > 0.0 && speed < 1.1 && delta < config.general.stop_threshold {
                speed_filter = format!("{t}realtime=speed={speed}");
            }
        }

        chain.add_filter(&speed_filter, codec_type);
    }
}

pub fn filter_chains(node: &mut Media) -> Vec<String> {
    let config = GlobalConfig::global();

    let mut filters = Filters::new();
    let mut audio_map = "1:a".to_string();
    filters.audio_map = Some(audio_map);

    if let Some(probe) = node.probe.clone() {
        if probe.audio_streams.is_some() {
            audio_map = "0:a".to_string();
            filters.audio_map = Some(audio_map);
        }

        let v_stream = &probe.video_streams.unwrap()[0];
        let aspect = aspect_calc(&v_stream.display_aspect_ratio, config);
        let frame_per_sec = fps_calc(&v_stream.r_frame_rate);

        deinterlace(&v_stream.field_order, &mut filters);
        pad(aspect, &mut filters, config);
        fps(frame_per_sec, &mut filters, config);
        scale(
            v_stream.width.unwrap(),
            v_stream.height.unwrap(),
            aspect,
            &mut filters,
            config,
        );
        extend_video(node, &mut filters);

        add_audio(node, &mut filters);
        extend_audio(node, &mut filters);
    }

    add_text(node, &mut filters, config);
    fade(node, &mut filters, "video");
    overlay(node, &mut filters, config);
    realtime_filter(node, &mut filters, config, "video");

    add_loudnorm(node, &mut filters, config);
    fade(node, &mut filters, "audio");
    audio_volume(&mut filters, config);
    realtime_filter(node, &mut filters, config, "audio");

    let mut filter_cmd = vec![];
    let mut filter_str: String = String::new();
    let mut filter_map: Vec<String> = vec![];

    if let Some(v_filters) = filters.video_chain {
        filter_str.push_str(v_filters.as_str());
        filter_str.push_str(filters.video_map.clone().unwrap().as_str());
        filter_map.append(&mut vec!["-map".to_string(), filters.video_map.unwrap()]);
    } else {
        filter_map.append(&mut vec!["-map".to_string(), "0:v".to_string()]);
    }

    if let Some(a_filters) = filters.audio_chain {
        if filter_str.len() > 10 {
            filter_str.push(';')
        }
        filter_str.push_str(a_filters.as_str());
        filter_str.push_str(filters.audio_map.clone().unwrap().as_str());
        filter_map.append(&mut vec!["-map".to_string(), filters.audio_map.unwrap()]);
    } else {
        filter_map.append(&mut vec!["-map".to_string(), filters.audio_map.unwrap()]);
    }

    if filter_str.len() > 10 {
        filter_cmd.push("-filter_complex".to_string());
        filter_cmd.push(filter_str);
    }

    filter_cmd.append(&mut filter_map);

    filter_cmd
}
