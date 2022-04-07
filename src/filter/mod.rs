use std::path::Path;

use simplelog::*;

pub mod v_drawtext;

use crate::utils::{get_delta, is_close, GlobalConfig, Media, PlayoutStatus};

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
                    if filter.contains("aevalsrc") || filter.contains("anoisesrc") {
                        self.audio_chain = Some(filter);
                    } else {
                        self.audio_chain =
                            Some(format!("[{}]{filter}", self.audio_map.clone().unwrap()));
                    }
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
                    self.video_chain = Some(format!("[0:v]{filter}"));
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

fn pad(aspect: f64, chain: &mut Filters, config: &GlobalConfig) {
    if !is_close(aspect, config.processing.aspect, 0.03) {
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

fn fps(fps: f64, chain: &mut Filters, config: &GlobalConfig) {
    if fps != config.processing.fps {
        chain.add_filter(
            format!("fps={}", config.processing.fps).into(),
            "video".into(),
        )
    }
}

fn scale(width: i64, height: i64, aspect: f64, chain: &mut Filters, config: &GlobalConfig) {
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

    if !is_close(aspect, config.processing.aspect, 0.03) {
        chain.add_filter(
            format!("setdar=dar={}", config.processing.aspect).into(),
            "video".into(),
        )
    }
}

fn fade(node: &mut Media, chain: &mut Filters, codec_type: String) {
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

fn overlay(node: &mut Media, chain: &mut Filters, config: &GlobalConfig) {
    if config.processing.add_logo
        && Path::new(&config.processing.logo).is_file()
        && node.category != "advertisement".to_string()
    {
        let opacity = format!(
            "format=rgba,colorchannelmixer=aa={}",
            config.processing.logo_opacity
        );
        let logo_loop = "loop=loop=-1:size=1:start=0";
        let mut logo_chain = format!(
            "null[v];movie={},{logo_loop},{opacity}",
            config.processing.logo
        );

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

        chain.add_filter(logo_chain, "video".into());
    }
}

fn extend_video(node: &mut Media, chain: &mut Filters) {
    let video_streams = node.probe.clone().unwrap().video_streams.unwrap();
    if video_streams.len() > 0 {
        let video_duration = &video_streams[0].duration;

        if video_duration.is_some() {
            let duration_float = video_duration.clone().unwrap().parse::<f64>().unwrap();

            if node.out - node.seek > duration_float - node.seek + 0.1 {
                chain.add_filter(
                    format!(
                        "tpad=stop_mode=add:stop_duration={}",
                        (node.out - node.seek) - (duration_float - node.seek)
                    ),
                    "video".into(),
                )
            }
        }
    }
}

fn add_text(node: &mut Media, chain: &mut Filters, config: &GlobalConfig) {
    // add drawtext filter for lower thirds messages

    if config.text.add_text && config.text.over_pre {
        let filter = v_drawtext::filter_node(node);

        chain.add_filter(filter, "video".into());

        if let Some(filters) = &chain.video_chain {
            for (i, f) in filters.split(",").enumerate() {
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
    if audio_streams.len() == 0 {
        warn!("Clip: '{}' has no audio!", node.source);
        let audio = format!(
            "aevalsrc=0:channel_layout=stereo:duration={}:sample_rate=48000",
            node.out - node.seek
        );
        chain.add_filter(audio, "audio".into());
    }
}

fn extend_audio(node: &mut Media, chain: &mut Filters) {
    let audio_streams = node.probe.clone().unwrap().audio_streams.unwrap();
    if audio_streams.len() > 0 {
        let audio_duration = &audio_streams[0].duration;

        if audio_duration.is_some() {
            let duration_float = audio_duration.clone().unwrap().parse::<f64>().unwrap();

            if node.out - node.seek > duration_float - node.seek + 0.1 {
                chain.add_filter(
                    format!("apad=whole_dur={}", node.out - node.seek),
                    "audio".into(),
                )
            }
        }
    }
}

fn add_loudnorm(node: &mut Media, chain: &mut Filters, config: &GlobalConfig) {
    // add single pass loudnorm filter to audio line

    if node.probe.is_some()
        && node.probe.clone().unwrap().audio_streams.unwrap().len() > 0
        && config.processing.add_loudnorm
    {
        let loud_filter = format!(
            "loudnorm=I={}:TP={}:LRA={}",
            config.processing.loud_i, config.processing.loud_tp, config.processing.loud_lra
        );

        chain.add_filter(loud_filter, "audio".into());
    }
}

fn audio_volume(chain: &mut Filters, config: &GlobalConfig) {
    if config.processing.volume != 1.0 {
        chain.add_filter(
            format!("volume={}", config.processing.volume),
            "audio".into(),
        )
    }
}

fn aspect_calc(aspect_string: String) -> f64 {
    let aspect_vec: Vec<&str> = aspect_string.split(':').collect();
    let w: f64 = aspect_vec[0].parse().unwrap();
    let h: f64 = aspect_vec[1].parse().unwrap();
    let source_aspect: f64 = w as f64 / h as f64;

    source_aspect
}

fn fps_calc(r_frame_rate: String) -> f64 {
    let frame_rate_vec: Vec<&str> = r_frame_rate.split('/').collect();
    let rate: f64 = frame_rate_vec[0].parse().unwrap();
    let factor: f64 = frame_rate_vec[1].parse().unwrap();
    let fps: f64 = rate / factor;

    fps
}

fn realtime_filter(
    node: &mut Media,
    chain: &mut Filters,
    config: &GlobalConfig,
    codec_type: String,
    playout_stat: &PlayoutStatus,
) {
    // this realtime filter is important for HLS output to stay in sync

    let mut t = "".to_string();

    if codec_type == "audio".to_string() {
        t = "a".to_string()
    }

    if config.out.mode.to_lowercase() == "hls".to_string() {
        let mut speed_filter = format!("{t}realtime=speed=1");
        let (delta, _) = get_delta(&node.begin.unwrap(), &playout_stat, true);
        let duration = node.out - node.seek;

        if delta < 0.0 {
            let speed = duration / (duration + delta);

            if speed > 0.0 && speed < 1.1 && delta < config.general.stop_threshold {
                speed_filter = format!("{t}realtime=speed={speed}");
            }
        }

        chain.add_filter(speed_filter, codec_type);
    }
}

pub fn filter_chains(node: &mut Media, playout_stat: &PlayoutStatus) -> Vec<String> {
    let config = GlobalConfig::global();

    let mut filters = Filters::new();
    let mut audio_map = "1:a".to_string();
    filters.audio_map = Some(audio_map);

    if node.probe.is_some() {
        let probe = node.probe.clone();

        if node.probe.as_ref().unwrap().audio_streams.is_some() {
            audio_map = "0:a".to_string();
            filters.audio_map = Some(audio_map);
        }

        let v_stream = &probe.unwrap().video_streams.unwrap()[0];
        let aspect = aspect_calc(v_stream.display_aspect_ratio.clone().unwrap());
        let frame_per_sec = fps_calc(v_stream.r_frame_rate.clone());

        deinterlace(v_stream.field_order.clone(), &mut filters);
        pad(aspect, &mut filters, &config);
        fps(frame_per_sec, &mut filters, &config);
        scale(
            v_stream.width.unwrap(),
            v_stream.height.unwrap(),
            aspect,
            &mut filters,
            &config,
        );
        extend_video(node, &mut filters);

        add_audio(node, &mut filters);
        extend_audio(node, &mut filters);
    }

    add_text(node, &mut filters, &config);
    fade(node, &mut filters, "video".into());
    overlay(node, &mut filters, &config);
    realtime_filter(node, &mut filters, &config, "video".into(), &playout_stat);

    add_loudnorm(node, &mut filters, &config);
    fade(node, &mut filters, "audio".into());
    audio_volume(&mut filters, &config);
    realtime_filter(node, &mut filters, &config, "audio".into(), &playout_stat);

    let mut filter_cmd = vec![];
    let mut filter_str: String = "".to_string();
    let mut filter_map: Vec<String> = vec![];

    if filters.video_chain.is_some() {
        filter_str.push_str(filters.video_chain.unwrap().as_str());
        filter_str.push_str(filters.video_map.clone().unwrap().as_str());
        filter_map.append(&mut vec!["-map".to_string(), filters.video_map.unwrap()]);
    } else {
        filter_map.append(&mut vec!["-map".to_string(), "0:v".to_string()]);
    }

    if filters.audio_chain.is_some() {
        if filter_str.len() > 10 {
            filter_str.push_str(";")
        }
        filter_str.push_str(filters.audio_chain.unwrap().as_str());
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
