use std::{fmt, path::Path, sync::Arc};

use log::*;
use regex::Regex;
use shlex::split;
use tokio::sync::Mutex;

mod custom;
pub mod v_drawtext;

use crate::player::{
    controller::ProcessUnit::*,
    utils::{calc_aspect, custom_format, fps_calc, fraction, is_close, Media},
};
use crate::utils::{
    config::{OutputMode::*, PlayoutConfig},
    logging::Target,
};
use crate::vec_strings;

#[derive(Clone, Debug, Copy, Eq, PartialEq)]
pub enum FilterType {
    Audio,
    Video,
}

impl fmt::Display for FilterType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Self::Audio => write!(f, "a"),
            Self::Video => write!(f, "v"),
        }
    }
}

use FilterType::*;

const HW_FILTER_POSTFIX: &[&str; 6] = &["_cuda", "_npp", "_opencl", "_vaapi", "_vulkan", "_qsv"];

#[derive(Debug, Clone)]
pub struct Filters {
    hw_context: bool,
    a_chain: Vec<String>,
    v_chain: Vec<String>,
    pub audio_chain: String,
    pub video_chain: String,
    pub output_chain: Vec<String>,
    pub audio_map: Vec<String>,
    pub video_map: Vec<String>,
    pub audio_out_link: Vec<String>,
    pub video_out_link: Vec<String>,
    pub output_map: Vec<String>,
    config: PlayoutConfig,
    audio_position: i32,
    video_position: i32,
    audio_last: i32,
    video_last: i32,
}

impl Filters {
    pub fn new(config: PlayoutConfig, audio_position: i32) -> Self {
        let hw = config
            .advanced
            .decoder
            .input_param
            .as_ref()
            .is_some_and(|i| i.contains("-hw"));

        Self {
            hw_context: hw,
            a_chain: vec![],
            v_chain: vec![],
            audio_chain: String::new(),
            video_chain: String::new(),
            output_chain: vec![],
            audio_map: vec![],
            video_map: vec![],
            audio_out_link: vec![],
            video_out_link: vec![],
            output_map: vec![],
            config,
            audio_position,
            video_position: 0,
            audio_last: -1,
            video_last: -1,
        }
    }

    pub fn add(&mut self, filter: &str, track_nr: i32, filter_type: FilterType) {
        let (map, chain, position, last) = match filter_type {
            Audio => (
                &mut self.audio_map,
                &mut self.a_chain,
                self.audio_position,
                &mut self.audio_last,
            ),
            Video => (
                &mut self.video_map,
                &mut self.v_chain,
                self.video_position,
                &mut self.video_last,
            ),
        };

        if *last == track_nr {
            chain.push(filter.to_string());
        } else {
            // start new filter chain
            let (selector, sep) = if chain.is_empty() {
                (String::new(), String::new())
            } else {
                (format!("[{filter_type}out{last}]"), ";".to_string())
            };
            let mut chain_start = String::new();

            chain_start.push_str(&selector);

            if filter.starts_with("aevalsrc") || filter.starts_with("movie") {
                if filter_type == Video && chain.is_empty() {
                    chain.push(format!("[{position}:{filter_type}:{track_nr}]null"));
                }
                chain_start.push_str(&sep);
            } else {
                // build audio/video selector like [0:a:0]
                chain_start.push_str(&format!("{sep}[{position}:{filter_type}:{track_nr}]"));
            }

            if self.hw_context
                && !is_hw(filter)
                && !filter.contains("hwdownload")
                && !filter.contains("hwupload")
                && filter_type == Video
            {
                chain_start.push_str("hwdownload,format=nv12,");
            }

            chain_start.push_str(filter);

            chain.push(chain_start);

            *last = track_nr;
            let m = format!("[{filter_type}out{track_nr}]");
            map.push(m.clone());
            self.output_map.append(&mut vec_strings!["-map", m]);
        }
    }

    fn build(&mut self) {
        for (i, filter) in self.a_chain.iter().enumerate() {
            if i > 0 && i < self.a_chain.len() && !filter.starts_with("[") {
                self.audio_chain.push(',');
            }

            self.audio_chain.push_str(filter);
        }

        for (i, filter) in self.v_chain.iter().enumerate() {
            if filter.starts_with("movie=") {
                if self.hw_context
                    && !last_is_hw(&self.video_chain)
                    && !self.config.advanced.is_empty_filter()
                {
                    let hw_up = hw_upload_str(&self.config);

                    self.video_chain.push(',');
                    self.video_chain.push_str(&hw_up);
                }
                self.video_chain.push_str("[v];");
            } else if filter.starts_with("overlay") {
                let hw_up = hw_upload(&self.config, &self.video_chain, filter);

                if !hw_up.is_empty() {
                    self.video_chain.push(',');
                    self.video_chain.push_str(&hw_up);
                }

                self.video_chain.push_str("[l];[v][l]");
            } else if i > 0 && i < self.v_chain.len() && !filter.starts_with("[") {
                self.video_chain.push(',');

                let hw_dl = hw_download(&self.video_chain, filter);
                let hw_ul = hw_upload(&self.config, &self.video_chain, filter);

                if (!hw_dl.is_empty() || !hw_ul.is_empty())
                    && !filter.starts_with("movie=")
                    && !filter.starts_with("overlay")
                {
                    self.video_chain.push_str(&hw_dl);
                    self.video_chain.push_str(&hw_ul);
                    self.video_chain.push(',');
                }
            }

            self.video_chain.push_str(filter);
        }
    }

    pub fn cmd(&mut self) -> Vec<String> {
        if !self.output_chain.is_empty() {
            return self.output_chain.clone();
        }

        let mut cmd = vec![];

        if self.audio_last >= 0 && !self.audio_chain.ends_with(']') {
            self.audio_chain
                .push_str(&format!("[aout{}]", self.audio_last));
        }

        if self.video_last >= 0 && !self.video_chain.is_empty() && !self.video_chain.ends_with(']')
        {
            if self.hw_context && !last_is_hw(&self.video_chain) {
                let hw_up = hw_upload_str(&self.config);

                self.video_chain.push_str(",format=nv12,");
                self.video_chain.push_str(&hw_up);
            }

            self.video_chain
                .push_str(&format!("[vout{}]", self.video_last));
        }

        let mut f_chain = self.video_chain.clone();

        if !self.audio_chain.is_empty() {
            if !f_chain.is_empty() {
                f_chain.push(';');
            }

            f_chain.push_str(&self.audio_chain);
        }

        if !f_chain.is_empty() {
            cmd.push("-filter_complex".to_string());
            cmd.push(f_chain);
        }

        cmd
    }

    pub fn map(&mut self) -> Vec<String> {
        if !self.output_chain.is_empty() && self.config.processing.override_filter {
            return vec![];
        }

        let mut o_map = self.output_map.clone();

        if self.video_last == -1 && !self.config.processing.audio_only {
            let v_map = "0:v".to_string();

            if !o_map.contains(&v_map) {
                o_map.append(&mut vec_strings!["-map", v_map]);
            };
        }

        if self.audio_last == -1 {
            for i in 0..self.config.processing.audio_tracks {
                let a_map = format!("{}:a:{i}", self.audio_position);

                if !o_map.contains(&a_map) {
                    o_map.append(&mut vec_strings!["-map", a_map]);
                };
            }
        }

        o_map
    }
}

impl Default for Filters {
    fn default() -> Self {
        Self::new(PlayoutConfig::default(), 0)
    }
}

fn is_hw(filter: &str) -> bool {
    HW_FILTER_POSTFIX.iter().any(|p| filter.contains(p))
}

fn last_is_hw(chain: &str) -> bool {
    let parts: Vec<&str> = chain.split_terminator([',', ';']).map(str::trim).collect();

    match parts.len() {
        0 => false,
        1 => {
            let last = parts[0];
            HW_FILTER_POSTFIX.iter().any(|p| last.contains(p)) && !last.contains("hwdownload")
        }
        _ => {
            let last = parts[parts.len() - 1];
            let second_last = parts[parts.len() - 2];
            HW_FILTER_POSTFIX.iter().any(|p| last.contains(p))
                && !last.contains("hwdownload")
                && !second_last.contains("hwdownload")
        }
    }
}

fn hw_download(chain: &str, f: &str) -> String {
    let mut filter = String::new();

    if last_is_hw(chain)
        && !is_hw(f)
        && !f.starts_with("null[")
        && !f.starts_with("[")
        && !f.contains("hwdownload")
        && !f.contains("hwupload")
    {
        filter = "hwdownload".to_string();

        if !chain.contains("_cuda") {
            filter.push_str(",format=nv12");
        }
    }

    filter
}

fn hw_upload_str(config: &PlayoutConfig) -> String {
    if config
        .advanced
        .decoder
        .input_param
        .as_ref()
        .is_some_and(|p| p.contains("cuda"))
    {
        return "hwupload_cuda".to_string();
    }

    "hwupload".to_string()
}

fn hw_upload(config: &PlayoutConfig, chain: &str, f: &str) -> String {
    let mut filter = String::new();

    if !last_is_hw(chain) && is_hw(f) && !f.contains("hwdownload") && !f.contains("hwupload") {
        filter = hw_upload_str(config);
    }

    filter
}

fn deinterlace(config: &PlayoutConfig, chain: &mut Filters, field_order: &Option<String>) {
    if let Some(order) = field_order {
        if order != "progressive" {
            let deinterlace = match config.advanced.filter.deinterlace.clone() {
                Some(deinterlace) => deinterlace,
                None => "yadif=0:-1:0".to_string(),
            };

            chain.add(&deinterlace, 0, Video);
        }
    }
}

fn pad(config: &PlayoutConfig, chain: &mut Filters, aspect: f64) {
    if !is_close(aspect, config.processing.aspect, 0.03) {
        let (numerator, denominator) = fraction(config.processing.aspect, 100);

        let pad = match config.advanced.filter.pad_video.clone() {
            Some(pad_video) => custom_format(
                &pad_video,
                &[&numerator.to_string(), &denominator.to_string()],
            ),
            None => format!("pad='ih*{numerator}/{denominator}:ih:(ow-iw)/2:(oh-ih)/2'"),
        };

        chain.add(&pad, 0, Video);
    }
}

fn fps(config: &PlayoutConfig, chain: &mut Filters, fps: f64) {
    if fps != config.processing.fps {
        let fps_filter = match config.advanced.filter.fps.clone() {
            Some(fps) => custom_format(&fps, &[&config.processing.fps]),
            None => format!("fps={}", config.processing.fps),
        };

        chain.add(&fps_filter, 0, Video);
    }
}

fn scale(config: &PlayoutConfig, chain: &mut Filters, width: Option<i64>, height: Option<i64>) {
    if let Some(scale) = &config.advanced.filter.scale {
        chain.add(
            &custom_format(
                scale,
                &[&config.processing.width, &config.processing.height],
            ),
            0,
            Video,
        );
    } else if width.is_some_and(|w| w != config.processing.width)
        || height.is_some_and(|w| w != config.processing.height)
    {
        chain.add(
            &format!(
                "scale={}:{}",
                config.processing.width, config.processing.height
            ),
            0,
            Video,
        );
    }
}

fn setdar(config: &PlayoutConfig, chain: &mut Filters, aspect: f64) {
    if !is_close(aspect, config.processing.aspect, 0.03) {
        let dar = match config.advanced.filter.set_dar.clone() {
            Some(set_dar) => custom_format(&set_dar, &[&config.processing.aspect]),
            None => format!("setdar=dar={}", config.processing.aspect),
        };

        chain.add(&dar, 0, Video);
    }
}

fn fade(
    config: &PlayoutConfig,
    chain: &mut Filters,
    node: &mut Media,
    nr: i32,
    filter_type: FilterType,
) {
    let mut t = "";
    let mut fade_audio = false;

    if filter_type == Audio {
        t = "a";

        if node.duration_audio > 0.0 && node.duration_audio != node.duration {
            fade_audio = true;
        }
    }

    if node.seek > 0.0 || node.unit == Ingest {
        let mut fade_in = format!("{t}fade=in:st=0:d=0.5");

        if t == "a" {
            if let Some(fade) = &config.advanced.filter.afade_in {
                fade_in = custom_format(fade, &[t]);
            }
        } else if let Some(fade) = &config.advanced.filter.fade_in {
            fade_in = custom_format(fade, &[t]);
        };

        chain.add(&fade_in, nr, filter_type);
    }

    if (node.out != node.duration && node.out - node.seek > 1.0) || fade_audio {
        let mut fade_out = format!("{t}fade=out:st={}:d=1.0", (node.out - node.seek - 1.0));

        if t == "a" {
            if let Some(fade) = &config.advanced.filter.afade_out {
                fade_out = custom_format(fade, &[node.out - node.seek - 1.0]);
            }
        } else if let Some(fade) = &config.advanced.filter.fade_out {
            fade_out = custom_format(fade, &[node.out - node.seek - 1.0]);
        };

        chain.add(&fade_out, nr, filter_type);
    }
}

fn overlay(config: &PlayoutConfig, chain: &mut Filters, node: &mut Media) {
    if config.processing.add_logo
        && Path::new(&config.processing.logo_path).is_file()
        && &node.category != "advertisement"
    {
        let logo_path = config
            .processing
            .logo_path
            .replace('\\', "/")
            .replace(':', "\\\\:");

        let movie = match &config.advanced.filter.logo {
            Some(logo) => {
                custom_format(logo, &[logo_path, config.processing.logo_opacity.to_string()])
        },
            None => format!(
                "movie={logo_path}:loop=0,setpts=N/(FRAME_RATE*TB),format=rgba,colorchannelmixer=aa={}",
                config.processing.logo_opacity,
            ),
        };

        chain.add(&movie, 0, Video);

        if node.last_ad {
            let fade_in = match config.advanced.filter.overlay_logo_fade_in.clone() {
                Some(fade_in) => fade_in,
                None => "fade=in:st=0:d=1.0:alpha=1".to_string(),
            };

            chain.add(&fade_in, 0, Video);
        }

        if node.next_ad {
            let length = node.out - node.seek - 1.0;

            let fade_out = match &config.advanced.filter.overlay_logo_fade_out {
                Some(fade_out) => custom_format(fade_out, &[length]),
                None => format!("fade=out:st={length}:d=1.0:alpha=1"),
            };

            chain.add(&fade_out, 0, Video);
        }

        if !config.processing.logo_scale.is_empty() {
            let scale = match &config.advanced.filter.overlay_logo_scale {
                Some(logo_scale) => custom_format(logo_scale, &[&config.processing.logo_scale]),
                None => format!("scale={}", config.processing.logo_scale),
            };

            chain.add(&scale, 0, Video);
        }

        let overlay = match &config.advanced.filter.overlay_logo {
            Some(ov) => custom_format(ov, &[&config.processing.logo_position]),
            None => format!("overlay={}:shortest=1", config.processing.logo_position),
        };

        chain.add(&overlay, 0, Video);
    }
}

fn extend_video(config: &PlayoutConfig, chain: &mut Filters, node: &mut Media) {
    if let Some(video_duration) = node
        .probe
        .as_ref()
        .and_then(|p| p.video.first())
        .and_then(|v| v.duration.as_ref())
    {
        if node.out - node.seek > video_duration - node.seek + 0.1 && node.duration >= node.out {
            let duration = (node.out - node.seek) - (video_duration - node.seek);

            let tpad = match config.advanced.filter.tpad.clone() {
                Some(pad) => custom_format(&pad, &[duration]),
                None => format!("tpad=stop_mode=add:stop_duration={duration}"),
            };

            chain.add(&tpad, 0, Video);
        }
    }
}

/// add drawtext filter for lower thirds messages
async fn add_text(
    config: &PlayoutConfig,
    chain: &mut Filters,
    node: &mut Media,
    filter_chain: &Option<Arc<Mutex<Vec<String>>>>,
) {
    if config.text.add_text
        && (config.text.text_from_filename || config.output.mode == HLS || node.unit == Encoder)
    {
        let filter = v_drawtext::filter_node(config, Some(node), filter_chain).await;

        chain.add(&filter, 0, Video);
    }
}

fn add_audio(config: &PlayoutConfig, chain: &mut Filters, node: &Media, nr: i32) {
    let audio = match config.advanced.filter.aevalsrc.clone() {
        Some(aevalsrc) => custom_format(&aevalsrc, &[node.out - node.seek]),
        None => format!(
            "aevalsrc=0:channel_layout=stereo:duration={}:sample_rate=48000",
            node.out - node.seek
        ),
    };

    chain.add(&audio, nr, Audio);
}

fn extend_audio(config: &PlayoutConfig, chain: &mut Filters, node: &mut Media, nr: i32) {
    if !Path::new(&node.audio).is_file() {
        if let Some(audio_duration) = node
            .probe
            .as_ref()
            .and_then(|p| p.audio.first())
            .and_then(|a| a.duration)
        {
            if node.out - node.seek > audio_duration - node.seek + 0.1 && node.duration >= node.out
            {
                let apad = match config.advanced.filter.apad.clone() {
                    Some(apad) => custom_format(&apad, &[node.out - node.seek]),
                    None => format!("apad=whole_dur={}", node.out - node.seek),
                };

                chain.add(&apad, nr, Audio);
            }
        }
    }
}

fn audio_volume(config: &PlayoutConfig, chain: &mut Filters, nr: i32) {
    if config.processing.volume != 1.0 {
        let volume = match config.advanced.filter.volume.clone() {
            Some(volume) => custom_format(&volume, &[config.processing.volume]),
            None => format!("volume={}", config.processing.volume),
        };

        chain.add(&volume, nr, Audio);
    }
}

pub fn split_filter(config: &PlayoutConfig, chain: &mut Filters, nr: i32, filter_type: FilterType) {
    let count = config.output.output_count;

    if count > 1 {
        let out_link = match filter_type {
            Audio => &mut chain.audio_out_link,
            Video => &mut chain.video_out_link,
        };

        for i in 0..count {
            let link = format!("[{filter_type}out_{nr}_{i}]");
            if !out_link.contains(&link) {
                out_link.push(link);
            }
        }

        let split = match config.advanced.filter.split.clone() {
            Some(split) => custom_format(&split, &[count.to_string(), out_link.join("")]),
            None => format!("split={count}{}", out_link.join("")),
        };

        chain.add(&split, nr, filter_type);
    }
}

/// Process output filter chain and add new filters to existing ones.
fn process_output_filters(config: &PlayoutConfig, chain: &mut Filters, output_filter: &str) {
    let filter =
        if (config.text.add_text && !config.text.text_from_filename) || config.output.mode == HLS {
            let re_v = Regex::new(r"\[[0:]+[v^\[]+([:0]+)?\]").unwrap(); // match video filter input link
            let _re_a = Regex::new(r"\[[0:]+[a^\[]+([:0]+)?\]").unwrap(); // match audio filter input link
            let re_a_out = Regex::new(r"\[aout[0-9]+\];?").unwrap(); // match audio output link
            let mut o_filter = output_filter.to_string();

            if let Some(first) = chain.v_chain.first() {
                o_filter = re_v.replace(&o_filter, &format!("{},", first)).to_string();
            }

            if !chain.a_chain.is_empty() {
                let audio_split = chain
                    .a_chain
                    .iter()
                    .filter(|f| f.contains("0:a") || f.contains("1:a"))
                    .map(|p| re_a_out.replace(p, "").to_string())
                    .collect::<Vec<String>>();

                for i in 0..config.processing.audio_tracks {
                    o_filter = o_filter.replace(
                        &format!("[0:a:{i}]"),
                        &format!("{},", &audio_split[i as usize]),
                    );
                }
            }

            o_filter
        } else {
            output_filter.to_string()
        };

    chain.output_chain = vec_strings!["-filter_complex", filter];
}

fn custom(filter: &str, chain: &mut Filters, nr: i32, filter_type: FilterType) {
    if !filter.is_empty() {
        chain.add(filter, nr, filter_type);
    }
}

pub async fn filter_chains(
    config: &PlayoutConfig,
    node: &mut Media,
    filter_chain: &Option<Arc<Mutex<Vec<String>>>>,
) -> Filters {
    let mut filters = Filters::new(config.clone(), 0);

    if config.processing.override_filter {
        //override hole filtering
        if node.unit == Ingest && !config.ingest.custom_filter.is_empty() {
            filters.output_chain = split(&config.ingest.custom_filter).unwrap_or_default();
        } else {
            filters.output_chain = split(&config.processing.custom_filter).unwrap_or_default();
        }

        return filters;
    }

    if node.source.contains("color=c=") {
        filters.audio_position = 1;
    }

    if node.unit == Encoder {
        if !config.processing.audio_only {
            add_text(config, &mut filters, node, filter_chain).await;
        }

        if let Some(f) = config.output.output_filter.clone() {
            process_output_filters(config, &mut filters, &f);
        } else if config.output.output_count > 1 && !config.processing.audio_only {
            split_filter(config, &mut filters, 0, Video);
        }

        filters.build();

        return filters;
    }

    if !config.processing.audio_only && !config.processing.copy_video {
        if let Some(probe) = node.probe.as_ref() {
            if Path::new(&node.audio).is_file() {
                filters.audio_position = 1;
            }

            if let Some(v_stream) = &probe.video.first() {
                let aspect = calc_aspect(config, &v_stream.aspect_ratio);
                let frame_per_sec = fps_calc(&v_stream.frame_rate, 1.0);

                deinterlace(config, &mut filters, &v_stream.field_order);
                pad(config, &mut filters, aspect);
                fps(config, &mut filters, frame_per_sec);
                scale(config, &mut filters, v_stream.width, v_stream.height);
                setdar(config, &mut filters, aspect);
            }

            extend_video(config, &mut filters, node);
        } else {
            fps(config, &mut filters, 0.0);
            scale(config, &mut filters, None, None);
        }

        add_text(config, &mut filters, node, filter_chain).await;
        fade(config, &mut filters, node, 0, Video);
        overlay(config, &mut filters, node);
    }

    let (proc_vf, proc_af) = if node.unit == Ingest {
        custom::filter_node(config.general.channel_id, &config.ingest.custom_filter)
    } else {
        custom::filter_node(config.general.channel_id, &config.processing.custom_filter)
    };

    let (list_vf, list_af) = custom::filter_node(config.general.channel_id, &node.custom_filter);

    if !config.processing.copy_video {
        custom(&proc_vf, &mut filters, 0, Video);
        custom(&list_vf, &mut filters, 0, Video);
    }

    let mut audio_indexes = vec![];

    if config.processing.audio_track_index == -1 {
        for i in 0..config.processing.audio_tracks {
            audio_indexes.push(i);
        }
    } else {
        audio_indexes.push(config.processing.audio_track_index);
    }

    if !config.processing.copy_audio {
        for i in audio_indexes {
            if node
                .probe
                .as_ref()
                .and_then(|p| p.audio.get(i as usize))
                .is_some()
                || Path::new(&node.audio).is_file()
            {
                extend_audio(config, &mut filters, node, i);
            } else if node.unit == Decoder && !node.source.contains("color=c=") {
                warn!(target: Target::file_mail(), channel = config.general.channel_id;
                    "Missing audio track (id {i}) from <b><magenta>{}</></b>",
                    node.source
                );

                add_audio(config, &mut filters, node, i);
            }

            // add at least anull filter, for correct filter construction,
            // is important for split filter in HLS mode
            filters.add("anull", i, Audio);

            fade(config, &mut filters, node, i, Audio);
            audio_volume(config, &mut filters, i);

            custom(&proc_af, &mut filters, i, Audio);
            custom(&list_af, &mut filters, i, Audio);
        }
    } else if config.processing.audio_track_index > -1 {
        error!(target: Target::file_mail(), channel = config.general.channel_id; "Setting 'audio_track_index' other than '-1' is not allowed in audio copy mode!");
    }

    if config.output.mode == HLS {
        if let Some(f) = config.output.output_filter.clone() {
            process_output_filters(config, &mut filters, &f);
        }
    }

    filters.build();

    filters
}
