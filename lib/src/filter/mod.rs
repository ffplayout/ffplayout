use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use simplelog::*;

pub mod a_loudnorm;
pub mod custom_filter;
pub mod v_drawtext;

use crate::utils::{fps_calc, get_delta, is_close, Media, MediaProbe, PlayoutConfig};

#[derive(Clone, Copy, PartialEq)]
enum FilterType {
    Audio,
    Video,
}

use FilterType::*;

use self::custom_filter::custom_filter;

#[derive(Debug, Clone)]
struct Filters {
    audio_chain: Option<String>,
    video_chain: Option<String>,
    audio_map: String,
    video_map: String,
}

impl Filters {
    fn new() -> Self {
        Filters {
            audio_chain: None,
            video_chain: None,
            audio_map: "0:a".to_string(),
            video_map: "0:v".to_string(),
        }
    }

    fn add_filter(&mut self, filter: &str, codec_type: FilterType) {
        match codec_type {
            Audio => match &self.audio_chain {
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
                        self.audio_chain = Some(format!("[{}]{filter}", self.audio_map.clone()));
                    }
                    self.audio_map = "[aout1]".to_string();
                }
            },
            Video => match &self.video_chain {
                Some(vc) => {
                    if filter.starts_with(';') || filter.starts_with('[') {
                        self.video_chain = Some(format!("{vc}{filter}"))
                    } else {
                        self.video_chain = Some(format!("{vc},{filter}"))
                    }
                }
                None => {
                    self.video_chain = Some(format!("[0:v]{filter}"));
                    self.video_map = "[vout1]".to_string();
                }
            },
        }
    }
}

fn deinterlace(field_order: &Option<String>, chain: &mut Filters) {
    if let Some(order) = field_order {
        if order != "progressive" {
            chain.add_filter("yadif=0:-1:0", Video)
        }
    }
}

fn pad(aspect: f64, chain: &mut Filters, v_stream: &ffprobe::Stream, config: &PlayoutConfig) {
    if !is_close(aspect, config.processing.aspect, 0.03) {
        let mut scale = String::new();

        if let (Some(w), Some(h)) = (v_stream.width, v_stream.height) {
            if w > config.processing.width && aspect > config.processing.aspect {
                scale = format!("scale={}:-1,", config.processing.width);
            } else if h > config.processing.height && aspect < config.processing.aspect {
                scale = format!("scale=-1:{},", config.processing.height);
            }
        }
        chain.add_filter(
            &format!(
                "{scale}pad=max(iw\\,ih*({0}/{1})):ow/({0}/{1}):(ow-iw)/2:(oh-ih)/2",
                config.processing.width, config.processing.height
            ),
            Video,
        )
    }
}

fn fps(fps: f64, chain: &mut Filters, config: &PlayoutConfig) {
    if fps != config.processing.fps {
        chain.add_filter(&format!("fps={}", config.processing.fps), Video)
    }
}

fn scale(
    width: Option<i64>,
    height: Option<i64>,
    aspect: f64,
    chain: &mut Filters,
    config: &PlayoutConfig,
) {
    // width: i64, height: i64
    if let (Some(w), Some(h)) = (width, height) {
        if w != config.processing.width || h != config.processing.height {
            chain.add_filter(
                &format!(
                    "scale={}:{}",
                    config.processing.width, config.processing.height
                ),
                Video,
            );
        } else {
            chain.add_filter("null", Video);
        }

        if !is_close(aspect, config.processing.aspect, 0.03) {
            chain.add_filter(&format!("setdar=dar={}", config.processing.aspect), Video)
        }
    } else {
        chain.add_filter(
            &format!(
                "scale={}:{}",
                config.processing.width, config.processing.height
            ),
            Video,
        );
        chain.add_filter(&format!("setdar=dar={}", config.processing.aspect), Video)
    }
}

fn fade(node: &mut Media, chain: &mut Filters, codec_type: FilterType) {
    let mut t = "";

    if codec_type == Audio {
        t = "a"
    }

    if node.seek > 0.0 || node.is_live == Some(true) {
        chain.add_filter(&format!("{t}fade=in:st=0:d=0.5"), codec_type)
    }

    if node.out != node.duration && node.out - node.seek - 1.0 > 0.0 {
        chain.add_filter(
            &format!("{t}fade=out:st={}:d=1.0", (node.out - node.seek - 1.0)),
            codec_type,
        )
    }
}

fn overlay(node: &mut Media, chain: &mut Filters, config: &PlayoutConfig) {
    if config.processing.add_logo
        && Path::new(&config.processing.logo).is_file()
        && &node.category != "advertisement"
    {
        let mut logo_chain = format!(
            "null[v];movie={}:loop=0,setpts=N/(FRAME_RATE*TB),format=rgba,colorchannelmixer=aa={}[l];[v][l]{}:shortest=1",
            config.processing.logo, config.processing.logo_opacity, config.processing.logo_filter
        );

        if node.last_ad.unwrap_or(false) {
            logo_chain.push_str(",fade=in:st=0:d=1.0:alpha=1")
        }

        if node.next_ad.unwrap_or(false) {
            logo_chain.push_str(
                format!(",fade=out:st={}:d=1.0:alpha=1", node.out - node.seek - 1.0).as_str(),
            )
        }

        chain.add_filter(&logo_chain, Video);
    }
}

fn extend_video(node: &mut Media, chain: &mut Filters) {
    if let Some(video_duration) = node
        .probe
        .as_ref()
        .and_then(|p| p.video_streams.get(0))
        .and_then(|v| v.duration.as_ref())
        .and_then(|v| v.parse::<f64>().ok())
    {
        if node.out - node.seek > video_duration - node.seek + 0.1 && node.duration >= node.out {
            chain.add_filter(
                &format!(
                    "tpad=stop_mode=add:stop_duration={}",
                    (node.out - node.seek) - (video_duration - node.seek)
                ),
                Video,
            )
        }
    }
}

/// add drawtext filter for lower thirds messages
fn add_text(
    node: &mut Media,
    chain: &mut Filters,
    config: &PlayoutConfig,
    filter_chain: &Arc<Mutex<Vec<String>>>,
) {
    if config.text.add_text
        && (config.text.text_from_filename || config.out.mode.to_lowercase() == "hls")
    {
        let filter = v_drawtext::filter_node(config, Some(node), filter_chain);

        chain.add_filter(&filter, Video);
    }
}

fn add_audio(node: &mut Media, chain: &mut Filters) {
    if node
        .probe
        .as_ref()
        .and_then(|p| p.audio_streams.get(0))
        .is_none()
        && !Path::new(&node.audio).is_file()
    {
        warn!("Clip <b><magenta>{}</></b> has no audio!", node.source);
        let audio = format!(
            "aevalsrc=0:channel_layout=stereo:duration={}:sample_rate=48000",
            node.out - node.seek
        );
        chain.add_filter(&audio, Audio);
    }
}

fn extend_audio(node: &mut Media, chain: &mut Filters) {
    let probe = if Path::new(&node.audio).is_file() {
        Some(MediaProbe::new(&node.audio))
    } else {
        node.probe.clone()
    };

    if let Some(audio_duration) = probe
        .as_ref()
        .and_then(|p| p.audio_streams.get(0))
        .and_then(|a| a.duration.clone())
        .and_then(|a| a.parse::<f64>().ok())
    {
        if node.out - node.seek > audio_duration - node.seek + 0.1 && node.duration >= node.out {
            chain.add_filter(&format!("apad=whole_dur={}", node.out - node.seek), Audio)
        }
    }
}

/// Add single pass loudnorm filter to audio line.
fn add_loudnorm(chain: &mut Filters, config: &PlayoutConfig) {
    if config.processing.add_loudnorm {
        let loud_filter = a_loudnorm::filter_node(config);
        chain.add_filter(&loud_filter, Audio);
    }
}

fn audio_volume(chain: &mut Filters, config: &PlayoutConfig) {
    if config.processing.volume != 1.0 {
        chain.add_filter(&format!("volume={}", config.processing.volume), Audio)
    }
}

fn aspect_calc(aspect_string: &Option<String>, config: &PlayoutConfig) -> f64 {
    let mut source_aspect = config.processing.aspect;

    if let Some(aspect) = aspect_string {
        let aspect_vec: Vec<&str> = aspect.split(':').collect();
        let w: f64 = aspect_vec[0].parse().unwrap();
        let h: f64 = aspect_vec[1].parse().unwrap();
        source_aspect = w as f64 / h as f64;
    }

    source_aspect
}

/// This realtime filter is important for HLS output to stay in sync.
fn realtime_filter(node: &mut Media, chain: &mut Filters, config: &PlayoutConfig) {
    if config.general.generate.is_none() && &config.out.mode.to_lowercase() == "hls" {
        let mut speed_filter = "realtime=speed=1".to_string();

        if let Some(begin) = &node.begin {
            let (delta, _) = get_delta(config, begin);

            if delta < 0.0 && node.seek == 0.0 {
                let duration = node.out - node.seek;
                let speed = duration / (duration + delta);

                if speed > 0.0 && speed < 1.1 && delta < config.general.stop_threshold {
                    speed_filter = format!("realtime=speed={speed}");
                }
            }
        }

        chain.add_filter(&speed_filter, Video);
    }
}

fn custom(filter: &str, chain: &mut Filters) {
    let (video_filter, audio_filter) = custom_filter(filter);

    if !video_filter.is_empty() {
        chain.add_filter(&video_filter, Video);
    }

    if !audio_filter.is_empty() {
        chain.add_filter(&audio_filter, Audio);
    }
}

pub fn filter_chains(
    config: &PlayoutConfig,
    node: &mut Media,
    filter_chain: &Arc<Mutex<Vec<String>>>,
) -> Vec<String> {
    let mut filters = Filters::new();

    if let Some(probe) = node.probe.as_ref() {
        if probe.audio_streams.get(0).is_none() || Path::new(&node.audio).is_file() {
            filters.audio_map = "1:a".to_string();
        }

        if let Some(v_stream) = &probe.video_streams.get(0) {
            let aspect = aspect_calc(&v_stream.display_aspect_ratio, config);
            let frame_per_sec = fps_calc(&v_stream.r_frame_rate, 1.0);

            deinterlace(&v_stream.field_order, &mut filters);
            pad(aspect, &mut filters, v_stream, config);
            fps(frame_per_sec, &mut filters, config);
            scale(
                v_stream.width,
                v_stream.height,
                aspect,
                &mut filters,
                config,
            );
        }

        extend_video(node, &mut filters);

        add_audio(node, &mut filters);
        extend_audio(node, &mut filters);
    } else {
        fps(0.0, &mut filters, config);
        scale(None, None, 1.0, &mut filters, config);
    }

    add_text(node, &mut filters, config, filter_chain);
    fade(node, &mut filters, Video);
    overlay(node, &mut filters, config);
    realtime_filter(node, &mut filters, config);

    // add at least anull filter, for correct filter construction,
    // is important for split filter in HLS mode
    filters.add_filter("anull", Audio);

    add_loudnorm(&mut filters, config);
    fade(node, &mut filters, Audio);
    audio_volume(&mut filters, config);

    custom(&config.processing.custom_filter, &mut filters);
    custom(&node.custom_filter, &mut filters);

    let mut filter_cmd = vec![];
    let mut filter_str: String = String::new();
    let mut filter_map: Vec<String> = vec![];

    if let Some(v_filters) = filters.video_chain {
        filter_str.push_str(v_filters.as_str());
        filter_str.push_str(filters.video_map.clone().as_str());
        filter_map.append(&mut vec!["-map".to_string(), filters.video_map]);
    } else {
        filter_map.append(&mut vec!["-map".to_string(), "0:v".to_string()]);
    }

    if let Some(a_filters) = filters.audio_chain {
        if filter_str.len() > 10 {
            filter_str.push(';')
        }
        filter_str.push_str(a_filters.as_str());
        filter_str.push_str(filters.audio_map.clone().as_str());
        filter_map.append(&mut vec!["-map".to_string(), filters.audio_map]);
    } else {
        filter_map.append(&mut vec!["-map".to_string(), filters.audio_map]);
    }

    if filter_str.len() > 10 {
        filter_cmd.push("-filter_complex".to_string());
        filter_cmd.push(filter_str);
    }

    filter_cmd.append(&mut filter_map);

    filter_cmd
}
