use std::{
    ffi::OsStr,
    fmt,
    io::Error,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{exit, Stdio},
    str::FromStr,
    sync::{atomic::Ordering, Arc},
};

use chrono::{prelude::*, TimeDelta};
use chrono_tz::Tz;
use log::*;
use probe::MediaProbe;
use rand::prelude::*;
use regex::Regex;
use reqwest::header;
use serde::{de::Deserializer, Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tokio::{
    fs::{metadata, File},
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{ChildStderr, Command},
    sync::Mutex,
};

pub mod import;
pub mod json_serializer;
pub mod json_validate;
pub mod probe;

use crate::player::{
    controller::{
        ChannelManager,
        ProcessUnit::{self, *},
    },
    filter::{filter_chains, Filters},
};
use crate::utils::{
    config::{OutputMode::*, PlayoutConfig, FFMPEG_IGNORE_ERRORS, FFMPEG_UNRECOVERABLE_ERRORS},
    errors::ServiceError,
    logging::Target,
    time_machine::time_now,
};
pub use json_serializer::{read_json, JsonPlaylist};

use crate::vec_strings;

/// Compare incoming stream name with expecting name, but ignore question mark.
pub fn valid_stream(msg: &str) -> bool {
    if let Some((unexpected, expected)) = msg.split_once(',') {
        let re = Regex::new(r".*Unexpected stream|App field don't match up|expecting|[\s]+|\?$")
            .unwrap();
        let unexpected = re.replace_all(unexpected, "");
        let expected = re.replace_all(expected, "");

        if unexpected == expected {
            return true;
        }
    }

    false
}

/// Prepare output parameters
///
/// Seek for multiple outputs and add mapping for it.
pub fn prepare_output_cmd(
    config: &PlayoutConfig,
    mut cmd: Vec<String>,
    filters: &Option<Filters>,
) -> Vec<String> {
    let mut output_params = config.output.clone().output_cmd.unwrap();
    let mut new_params = vec![];
    let mut count = 0;
    let re_v = Regex::new(r"\[?0:v(:0)?\]?").unwrap();

    if let Some(mut filter) = filters.clone() {
        for (i, param) in output_params.iter().enumerate() {
            if filter.video_out_link.len() > count && re_v.is_match(param) {
                // replace mapping with link from filter struct
                new_params.push(filter.video_out_link[count].clone());
            } else {
                new_params.push(param.clone());
            }

            // Check if parameter is a output
            if i > 0
                && !param.starts_with('-')
                && !output_params[i - 1].starts_with('-')
                && i < output_params.len() - 1
            {
                count += 1;

                if filter.video_out_link.len() > count
                    && !output_params.contains(&"-map".to_string())
                {
                    new_params.append(&mut vec_strings!["-map", filter.video_out_link[count]]);

                    for i in 0..config.processing.audio_tracks {
                        new_params.append(&mut vec_strings!["-map", format!("0:a:{i}")]);
                    }
                }
            }
        }

        output_params = new_params;

        cmd.append(&mut filter.cmd());

        // add mapping at the begin, if needed
        if !filter.map().iter().all(|item| output_params.contains(item))
            && filter.output_chain.is_empty()
            && filter.video_out_link.is_empty()
        {
            cmd.append(&mut filter.map());
        } else if &output_params[0] != "-map" && !filter.video_out_link.is_empty() {
            cmd.append(&mut vec_strings!["-map", filter.video_out_link[0].clone()]);

            for i in 0..config.processing.audio_tracks {
                cmd.append(&mut vec_strings!["-map", format!("0:a:{i}")]);
            }
        }
    }

    if config.processing.vtt_enable {
        let i = cmd.iter().filter(|&n| n == "-i").count().saturating_sub(1);

        cmd.append(&mut vec_strings!("-map", format!("{i}:s?")));
    }

    cmd.append(&mut output_params);

    cmd
}

/// map media struct to json object
pub fn get_media_map(media: Media) -> Value {
    let mut obj = json!({
        "in": media.seek,
        "out": media.out,
        "duration": media.duration,
        "category": media.category,
        "source": media.source,
    });

    if let Some(title) = media.title {
        obj.as_object_mut()
            .unwrap()
            .insert("title".to_string(), Value::String(title));
    }

    obj
}

/// prepare json object for response
pub async fn get_data_map(manager: &ChannelManager) -> Map<String, Value> {
    let media = manager
        .current_media
        .lock()
        .await
        .clone()
        .unwrap_or_else(Media::default);
    let channel = manager.channel.lock().await.clone();
    let config = manager.config.lock().await.processing.clone();
    let ingest_is_alive = manager.ingest_is_alive.load(Ordering::SeqCst);

    let mut data_map = Map::new();
    let current_time = time_in_seconds(&channel.timezone);
    let shift = channel.time_shift;
    let begin = media.begin.unwrap_or(0.0) - shift;
    let played_time = current_time - begin;

    data_map.insert("index".to_string(), json!(media.index));
    data_map.insert("ingest".to_string(), json!(ingest_is_alive));
    data_map.insert("mode".to_string(), json!(config.mode));
    data_map.insert(
        "shift".to_string(),
        json!((shift * 1000.0).round() / 1000.0),
    );
    data_map.insert(
        "elapsed".to_string(),
        json!((played_time * 1000.0).round() / 1000.0),
    );
    data_map.insert("media".to_string(), get_media_map(media));

    data_map
}

/// Video clip struct to hold some important states and comments for current media.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Media {
    #[serde(skip_serializing, skip_deserializing)]
    pub begin: Option<f64>,

    #[serde(skip_serializing, skip_deserializing)]
    pub index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(rename = "in")]
    pub seek: f64,
    pub out: f64,
    pub duration: f64,

    #[serde(skip_serializing, skip_deserializing)]
    pub duration_audio: f64,

    #[serde(
        default,
        deserialize_with = "null_string",
        skip_serializing_if = "is_empty_string"
    )]
    pub category: String,
    #[serde(deserialize_with = "null_string")]
    pub source: String,

    #[serde(
        default,
        deserialize_with = "null_string",
        skip_serializing_if = "is_empty_string"
    )]
    pub audio: String,

    #[serde(skip_serializing, skip_deserializing)]
    pub cmd: Option<Vec<String>>,

    #[serde(skip_serializing, skip_deserializing)]
    pub filter: Option<Filters>,

    #[serde(default, skip_serializing_if = "is_empty_string")]
    pub custom_filter: String,

    #[serde(skip_serializing, skip_deserializing)]
    pub probe: Option<MediaProbe>,

    #[serde(skip_serializing, skip_deserializing)]
    pub probe_audio: Option<MediaProbe>,

    #[serde(skip_serializing, skip_deserializing)]
    pub last_ad: bool,

    #[serde(skip_serializing, skip_deserializing)]
    pub next_ad: bool,

    #[serde(default, skip_serializing, skip_deserializing)]
    pub skip: bool,

    #[serde(default, skip_serializing)]
    pub unit: ProcessUnit,
}

impl Media {
    pub async fn new(index: usize, src: &str, do_probe: bool) -> Self {
        let mut duration = 0.0;
        let mut probe = None;

        if do_probe && (is_remote(src) || Path::new(src).is_file()) {
            if let Ok(p) = MediaProbe::new(src).await {
                probe = Some(p.clone());

                duration = p.format.duration.unwrap_or_default();
            }
        }

        Self {
            begin: None,
            index: Some(index),
            title: None,
            seek: 0.0,
            out: duration,
            duration,
            duration_audio: 0.0,
            category: String::new(),
            source: src.to_string(),
            audio: String::new(),
            cmd: Some(vec_strings!["-i", src]),
            filter: None,
            custom_filter: String::new(),
            probe,
            probe_audio: None,
            last_ad: false,
            next_ad: false,
            skip: false,
            unit: Decoder,
        }
    }

    pub async fn add_probe(&mut self, check_audio: bool) -> Result<(), String> {
        let mut errors = vec![];

        if self.probe.is_none() {
            match MediaProbe::new(&self.source).await {
                Ok(probe) => {
                    self.probe = Some(probe.clone());

                    if let Some(dur) = probe
                        .format
                        .duration
                        .filter(|d| !is_close(*d, self.duration, 0.5))
                    {
                        self.duration = dur;

                        if self.out == 0.0 {
                            self.out = dur;
                        }
                    }
                }
                Err(e) => errors.push(e.to_string()),
            };

            if check_audio && Path::new(&self.audio).is_file() {
                match MediaProbe::new(&self.audio).await {
                    Ok(probe) => {
                        self.probe_audio = Some(probe.clone());

                        if !probe.audio.is_empty() {
                            self.duration_audio = probe.audio[0].duration.unwrap_or_default();
                        }
                    }
                    Err(e) => errors.push(e.to_string()),
                }
            }
        }

        if !errors.is_empty() {
            return Err(errors.join(", "));
        }

        Ok(())
    }

    pub async fn add_filter(
        &mut self,
        config: &PlayoutConfig,
        filter_chain: &Option<Arc<Mutex<Vec<String>>>>,
    ) {
        let mut node = self.clone();
        self.filter = Some(filter_chains(config, &mut node, filter_chain).await);
    }
}

impl Default for Media {
    fn default() -> Self {
        Self {
            begin: None,
            index: Some(0),
            title: None,
            seek: 0.0,
            out: 0.0,
            duration: 0.0,
            duration_audio: 0.0,
            category: String::new(),
            source: String::new(),
            audio: String::new(),
            cmd: Some(vec_strings!["-i", String::new()]),
            filter: None,
            custom_filter: String::new(),
            probe: None,
            probe_audio: None,
            last_ad: false,
            next_ad: false,
            skip: false,
            unit: Decoder,
        }
    }
}

impl PartialEq for Media {
    fn eq(&self, other: &Self) -> bool {
        self.title == other.title
            && self.seek == other.seek
            && self.out == other.out
            && self.duration == other.duration
            && self.source == other.source
            && self.category == other.category
            && self.audio == other.audio
            && self.custom_filter == other.custom_filter
    }
}

impl Eq for Media {}

fn null_string<'de, D>(d: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    Deserialize::deserialize(d).map(|x: Option<_>| x.unwrap_or_default())
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_empty_string(st: &String) -> bool {
    *st == String::new()
}

/// Calculate fps from rate/factor string
pub fn fps_calc(r_frame_rate: &str, default: f64) -> f64 {
    if let Some((r, f)) = r_frame_rate.split_once('/') {
        if let (Ok(r_value), Ok(f_value)) = (r.parse::<f64>(), f.parse::<f64>()) {
            return r_value / f_value;
        }
    }

    default
}

pub async fn json_reader(path: &PathBuf) -> Result<JsonPlaylist, Error> {
    let mut f = File::options().read(true).write(false).open(path).await?;
    let mut contents = String::new();
    f.read_to_string(&mut contents).await?;
    let p = serde_json::from_str(&contents)?;

    Ok(p)
}

pub async fn json_writer(path: &PathBuf, data: JsonPlaylist) -> Result<(), Error> {
    let mut f = File::options()
        .write(true)
        .truncate(true)
        .create(true)
        .open(path)
        .await?;
    let contents = serde_json::to_string_pretty(&data)?;
    f.write_all(contents.as_bytes()).await?;

    Ok(())
}

/// Get current time in seconds.
pub fn time_in_seconds(timezone: &Option<Tz>) -> f64 {
    let local: DateTime<Tz> = time_now(timezone);

    (local.hour() * 3600 + local.minute() * 60 + local.second()) as f64
        + (local.nanosecond() as f64 / 1000000000.0)
}

/// Get current date for playlist, but check time with conditions:
///
/// - When time is before playlist start, get date from yesterday.
/// - When given next_start is over target length (normally a full day), get date from tomorrow.
pub fn get_date(seek: bool, start: f64, get_next: bool, timezone: &Option<Tz>) -> String {
    let local: DateTime<Tz> = time_now(timezone);

    if seek && start > time_in_seconds(timezone) {
        return (local - TimeDelta::try_days(1).unwrap())
            .format("%Y-%m-%d")
            .to_string();
    }

    if start == 0.0 && get_next && time_in_seconds(timezone) > 86397.9 {
        return (local + TimeDelta::try_days(1).unwrap())
            .format("%Y-%m-%d")
            .to_string();
    }

    local.format("%Y-%m-%d").to_string()
}

pub fn time_from_header(headers: &header::HeaderMap) -> Option<DateTime<Local>> {
    if let Some(time) = headers.get(header::LAST_MODIFIED) {
        if let Ok(t) = time.to_str() {
            let time = DateTime::parse_from_rfc2822(t);
            let date_time: DateTime<Local> = time.unwrap().into();
            return Some(date_time);
        };
    }

    None
}

/// Get file modification time.
pub async fn modified_time(path: &str) -> Option<String> {
    if is_remote(path) {
        let response = reqwest::Client::new().head(path).send().await;

        if let Ok(resp) = response {
            if resp.status().is_success() {
                if let Some(time) = time_from_header(resp.headers()) {
                    return Some(time.to_string());
                }
            }
        }

        return None;
    }

    if let Ok(time) = metadata(path)
        .await
        .and_then(|metadata| metadata.modified())
    {
        let date_time: DateTime<Local> = time.into();
        return Some(date_time.to_string());
    }

    None
}

/// Convert a formatted time string to seconds.
pub fn time_to_sec(time_str: &str, timezone: &Option<Tz>) -> f64 {
    if matches!(time_str, "now" | "" | "none") || !time_str.contains(':') {
        return time_in_seconds(timezone);
    }

    let mut t = time_str.split(':').filter_map(|n| f64::from_str(n).ok());

    t.next().unwrap_or(0.0) * 3600.0 + t.next().unwrap_or(0.0) * 60.0 + t.next().unwrap_or(0.0)
}

/// Convert floating number (seconds) to a formatted time string.
pub fn sec_to_time(sec: f64) -> String {
    let s = (sec * 1000.0).round() / 1000.0;

    format!(
        "{:0>2}:{:0>2}:{:06.3}",
        (s / 3600.0) as i32,
        (s / 60.0 % 60.0) as i32,
        (s % 60.0),
    )
}

/// get file extension
pub fn file_extension(filename: &Path) -> Option<&str> {
    filename.extension().and_then(OsStr::to_str)
}

/// Test if given numbers are close to each other,
/// with a third number for setting the maximum range.
pub fn is_close(a: f64, b: f64, to: f64) -> bool {
    (a - b).abs() < to
}

/// add duration from all media clips
pub fn sum_durations(clip_list: &[Media]) -> f64 {
    clip_list.iter().map(|item| item.out).sum()
}

/// Get delta between clip start and current time. This value we need to check,
/// if we still in sync.
///
/// We also get here the global delta between clip start and time when a new playlist should start.
pub fn get_delta(config: &PlayoutConfig, begin: &f64) -> (f64, f64) {
    let mut current_time = time_in_seconds(&config.channel.timezone);
    let start = config.playlist.start_sec.unwrap();
    let length = config.playlist.length_sec.unwrap_or(86400.0);
    let mut target_length = 86400.0;

    if length > 0.0 && length != target_length {
        target_length = length;
    }

    if begin == &start && start == 0.0 && 86400.0 - current_time < 4.0 {
        current_time -= 86400.0;
    } else if start >= current_time && begin != &start {
        current_time += 86400.0;
    }

    let mut current_delta = begin - current_time;

    if is_close(
        current_delta.abs(),
        86400.0,
        config.general.stop_threshold + 2.0,
    ) {
        current_delta = current_delta.abs() - 86400.0;
    }

    let total_delta = if current_time < start {
        start - current_time
    } else {
        target_length + start - current_time
    };

    (current_delta, total_delta)
}

/// Loop image until target duration is reached.
pub fn loop_image(config: &PlayoutConfig, node: &Media) -> Vec<String> {
    let duration = node.out - node.seek;
    let mut source_cmd: Vec<String> = vec_strings!["-loop", "1", "-i", node.source.clone()];

    info!(
        "Loop image <b><magenta>{}</></b>, total duration: <yellow>{duration:.2}</>",
        node.source
    );

    if Path::new(&node.audio).is_file() {
        if node.seek > 0.0 {
            source_cmd.append(&mut vec_strings!["-ss", node.seek]);
        }

        source_cmd.append(&mut vec_strings!["-i", node.audio.clone()]);
    }

    source_cmd.append(&mut vec_strings!["-t", duration]);

    if config.processing.vtt_enable {
        let vtt_file = Path::new(&node.source).with_extension("vtt");
        let vtt_dummy = config
            .channel
            .storage
            .join(config.processing.vtt_dummy.clone().unwrap_or_default());

        if node.seek > 0.5 {
            source_cmd.append(&mut vec_strings!["-ss", node.seek]);
        }

        if vtt_file.is_file() {
            source_cmd.append(&mut vec_strings![
                "-i",
                vtt_file.to_string_lossy(),
                "-t",
                node.out
            ]);
        } else if vtt_dummy.is_file() {
            source_cmd.append(&mut vec_strings!["-i", vtt_dummy.to_string_lossy()]);
        } else {
            error!("WebVTT enabled, but no vtt or dummy file found!");
        }
    }

    source_cmd
}

/// Loop filler until target duration is reached.
pub fn loop_filler(config: &PlayoutConfig, node: &Media) -> Vec<String> {
    let loop_count = (node.out / node.duration).ceil() as i32;
    let mut source_cmd = vec![];

    if loop_count > 1 {
        info!("Loop <b><magenta>{}</></b> <yellow>{loop_count}</> times, total duration: <yellow>{:.2}</>", node.source, node.out);

        source_cmd.append(&mut vec_strings!["-stream_loop", loop_count]);
    }

    source_cmd.append(&mut vec_strings!["-i", node.source, "-t", node.out]);

    if config.processing.vtt_enable {
        let vtt_file = Path::new(&node.source).with_extension("vtt");
        let vtt_dummy = config
            .channel
            .storage
            .join(config.processing.vtt_dummy.clone().unwrap_or_default());

        if vtt_file.is_file() {
            if loop_count > 1 {
                source_cmd.append(&mut vec_strings!["-stream_loop", loop_count]);
            }

            source_cmd.append(&mut vec_strings![
                "-i",
                vtt_file.to_string_lossy(),
                "-t",
                node.out
            ]);
        } else if vtt_dummy.is_file() {
            source_cmd.append(&mut vec_strings!["-i", vtt_dummy.to_string_lossy()]);
        } else {
            error!("WebVTT enabled, but no vtt or dummy file found!");
        }
    }

    source_cmd
}

/// Set clip seek in and length value.
pub fn seek_and_length(config: &PlayoutConfig, node: &mut Media) -> Vec<String> {
    let loop_count = (node.out / node.duration).ceil() as i32;
    let mut source_cmd = vec![];
    let mut cut_audio = false;
    let mut loop_audio = false;
    let remote_source = is_remote(&node.source);

    if remote_source && node.probe.clone().and_then(|f| f.format.duration).is_none() {
        node.out -= node.seek;
        node.seek = 0.0;
    } else if node.seek > 0.5 {
        source_cmd.append(&mut vec_strings!["-ss", node.seek]);
    }

    if loop_count > 1 {
        info!("Loop <b><magenta>{}</></b> <yellow>{loop_count}</> times, total duration: <yellow>{:.2}</>", node.source, node.out);

        source_cmd.append(&mut vec_strings!["-stream_loop", loop_count]);
    }

    source_cmd.append(&mut vec_strings!["-i", node.source.clone()]);

    if node.duration > node.out || remote_source || loop_count > 1 {
        source_cmd.append(&mut vec_strings!["-t", node.out - node.seek]);
    }

    if !node.audio.is_empty() {
        if node.seek > 0.5 {
            source_cmd.append(&mut vec_strings!["-ss", node.seek]);
        }

        if node.duration_audio > node.out {
            cut_audio = true;
        } else if node.duration_audio < node.out {
            source_cmd.append(&mut vec_strings!["-stream_loop", -1]);
            loop_audio = true;
        }

        source_cmd.append(&mut vec_strings!["-i", node.audio.clone()]);

        if cut_audio || loop_audio || remote_source {
            source_cmd.append(&mut vec_strings!["-t", node.out - node.seek]);
        }
    }

    if config.processing.vtt_enable {
        let vtt_file = Path::new(&node.source).with_extension("vtt");
        let vtt_dummy = config
            .channel
            .storage
            .join(config.processing.vtt_dummy.clone().unwrap_or_default());

        if node.seek > 0.5 {
            source_cmd.append(&mut vec_strings!["-ss", node.seek]);
        }

        if vtt_file.is_file() {
            if loop_count > 1 {
                source_cmd.append(&mut vec_strings!["-stream_loop", loop_count]);
            }

            source_cmd.append(&mut vec_strings!["-i", vtt_file.to_string_lossy()]);

            if node.duration > node.out || remote_source || loop_count > 1 {
                source_cmd.append(&mut vec_strings!["-t", node.out - node.seek]);
            }
        } else if vtt_dummy.is_file() {
            source_cmd.append(&mut vec_strings!["-i", vtt_dummy.to_string_lossy()]);
        } else {
            error!("<b><magenta>{:?}</></b> not found!", vtt_dummy);
        }
    }

    source_cmd
}

/// Create a dummy clip as a placeholder for missing video files.
pub fn gen_dummy(config: &PlayoutConfig, duration: f64) -> (String, Vec<String>) {
    let color = "#121212";
    let source = format!(
        "color=c={color}:s={}x{}:d={duration}",
        config.processing.width, config.processing.height
    );
    let mut source_cmd: Vec<String> = vec_strings![
        "-f",
        "lavfi",
        "-i",
        format!(
            "{source}:r={},format=pix_fmts=yuv420p",
            config.processing.fps
        ),
        "-f",
        "lavfi",
        "-i",
        format!("anoisesrc=d={duration}:c=pink:r=48000:a=0.3")
    ];

    if config.processing.vtt_enable {
        let vtt_dummy = config
            .channel
            .storage
            .join(config.processing.vtt_dummy.clone().unwrap_or_default());

        if vtt_dummy.is_file() {
            source_cmd.append(&mut vec_strings!["-i", vtt_dummy.to_string_lossy()]);
        } else {
            error!("WebVTT enabled, but no vtt or dummy file found!");
        }
    }

    (source, source_cmd)
}

// fn get_output_count(cmd: &[String]) -> i32 {
//     let mut count = 0;

//     if let Some(index) = cmd.iter().position(|c| c == "-var_stream_map") {
//         if let Some(mapping) = cmd.get(index + 1) {
//             return mapping.split(' ').count() as i32;
//         };
//     };

//     for (i, param) in cmd.iter().enumerate() {
//         if i > 0 && !param.starts_with('-') && !cmd[i - 1].starts_with('-') {
//             count += 1;
//         }
//     }

//     count
// }

pub fn is_remote(path: &str) -> bool {
    Regex::new(r"^(https?|rtmps?|rts?p|udp|tcp|srt)://.*")
        .unwrap()
        .is_match(&path.to_lowercase())
}

/// Check if file can include or has to exclude.
/// For example when a file is on given HLS output path, it should exclude.
/// Or when the file extension is set under storage config it can be include.
pub fn include_file_extension(config: &PlayoutConfig, file_path: &Path) -> bool {
    let mut include = false;

    if let Some(ext) = file_extension(file_path) {
        if config.storage.extensions.contains(&ext.to_lowercase()) {
            include = true;
        }
    }

    if config.output.mode == HLS {
        if let Some(ts_path) = config
            .output
            .output_cmd
            .clone()
            .unwrap_or_else(|| vec![String::new()])
            .iter()
            .find(|s| s.contains(".ts"))
        {
            if let Some(p) = Path::new(ts_path).parent() {
                if file_path.starts_with(p) {
                    include = false;
                }
            }
        }

        if let Some(m3u8_path) = config
            .output
            .output_cmd
            .clone()
            .unwrap_or_else(|| vec![String::new()])
            .iter()
            .find(|s| s.contains(".m3u8") && !s.contains("master.m3u8"))
        {
            if let Some(p) = Path::new(m3u8_path).parent() {
                if file_path.starts_with(p) {
                    include = false;
                }
            }
        }
    }

    include
}

/// Read ffmpeg stderr decoder and encoder instance
/// and log the output.
pub async fn stderr_reader(
    buffer: tokio::io::BufReader<ChildStderr>,
    ignore: Vec<String>,
    suffix: ProcessUnit,
    channel_id: i32,
) -> Result<(), ServiceError> {
    let mut lines = buffer.lines();

    while let Some(line) = lines.next_line().await? {
        if FFMPEG_IGNORE_ERRORS.iter().any(|i| line.contains(*i))
            || ignore.iter().any(|i| line.contains(i))
        {
            continue;
        }

        if line.contains("[info]") {
            info!(target: Target::file_mail(), channel = channel_id;
                "<bright black>[{suffix}]</> {}",
                line.replace("[info] ", "")
            );
        } else if line.contains("[warning]") {
            warn!(target: Target::file_mail(), channel = channel_id;
                "<bright black>[{suffix}]</> {}",
                line.replace("[warning] ", "")
            );
        } else if line.contains("[error]") || line.contains("[fatal]") {
            error!(target: Target::file_mail(), channel = channel_id;
                "<bright black>[{suffix}]</> {}",
                line.replace("[error] ", "").replace("[fatal] ", "")
            );

            if FFMPEG_UNRECOVERABLE_ERRORS
                .iter()
                .any(|i| line.contains(*i))
                || (line.contains("No such file or directory")
                    && !line.contains("failed to delete old segment"))
            {
                return Err(ServiceError::Conflict(
                    "Hit unrecoverable error!".to_string(),
                ));
            }
        }
    }

    Ok(())
}

/// Run program to test if it is in system.
async fn is_in_system(name: &str) -> Result<(), String> {
    match Command::new(name)
        .stderr(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
    {
        Ok(mut proc) => {
            if let Err(e) = proc.wait().await {
                return Err(format!("{e}"));
            };
        }
        Err(e) => return Err(format!("{name} not found on system! {e}")),
    }

    Ok(())
}

async fn ffmpeg_filter_and_libs(config: &mut PlayoutConfig) -> Result<(), String> {
    let id = config.general.channel_id;
    let ignore_flags = [
        "--enable-gpl",
        "--enable-version3",
        "--enable-runtime-cpudetect",
        "--enable-avfilter",
        "--enable-zlib",
        "--enable-pic",
        "--enable-nonfree",
    ];

    let mut ff_proc = match Command::new("ffmpeg")
        .args(["-filters"])
        .kill_on_drop(true)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Err(e) => {
            return Err(format!("couldn't spawn ffmpeg process: {e}"));
        }
        Ok(proc) => proc,
    };

    let out_buffer = BufReader::new(ff_proc.stdout.take().unwrap());
    let err_buffer = BufReader::new(ff_proc.stderr.take().unwrap());

    // stderr shows only the ffmpeg configuration
    // get codec library's
    let mut lines = err_buffer.lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if line.contains("configuration:") {
            let configs = line.split_whitespace();

            for flag in configs {
                if flag.contains("--enable") && !ignore_flags.contains(&flag) {
                    config
                        .general
                        .ffmpeg_libs
                        .push(flag.replace("--enable-", ""));
                }
            }
            break;
        }
    }

    // stdout shows filter from ffmpeg
    // get filters
    let mut lines = out_buffer.lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if line.contains('>') {
            let filter_line = line.split_whitespace().collect::<Vec<_>>();

            if filter_line.len() > 2 {
                config
                    .general
                    .ffmpeg_filters
                    .push(filter_line[1].to_string());
            }
        }
    }

    if let Err(e) = ff_proc.wait().await {
        error!(target: Target::file_mail(), channel = id; "{e}");
    };

    Ok(())
}

/// Validate ffmpeg/ffprobe/ffplay.
///
/// Check if they are in system and has all libs and codecs we need.
pub async fn validate_ffmpeg(config: &mut PlayoutConfig) -> Result<(), String> {
    is_in_system("ffmpeg").await?;
    is_in_system("ffprobe").await?;

    if config.output.mode == Desktop {
        is_in_system("ffplay").await?;
    }

    ffmpeg_filter_and_libs(config).await?;

    if config
        .output
        .output_cmd
        .as_ref()
        .unwrap()
        .contains(&"libx264".to_string())
        && !config.general.ffmpeg_libs.contains(&"libx264".to_string())
    {
        return Err("ffmpeg contains no libx264!".to_string());
    }

    if config.text.add_text
        && !config.text.text_from_filename
        && !config.general.ffmpeg_libs.contains(&"libzmq".to_string())
    {
        return Err(
            "ffmpeg contains no libzmq! Disable add_text in config or compile ffmpeg with libzmq."
                .to_string(),
        );
    }

    if config
        .output
        .output_cmd
        .as_ref()
        .unwrap()
        .contains(&"libfdk_aac".to_string())
        && !config
            .general
            .ffmpeg_libs
            .contains(&"libfdk-aac".to_string())
    {
        return Err("ffmpeg contains no libfdk-aac!".to_string());
    }

    Ok(())
}

/// get a free tcp socket
pub fn gen_tcp_socket(exclude_socket: String) -> Option<String> {
    for _ in 0..100 {
        let port = rand::rng().random_range(45321..54268);
        let socket = format!("127.0.0.1:{port}");

        if socket != exclude_socket && TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return Some(socket);
        }
    }

    None
}

/// check if tcp port is free
pub fn is_free_tcp_port(url: &str) -> bool {
    let re = Regex::new(r"^[\w]+://([^/]+)").unwrap();
    let mut addr = url.to_string();

    if let Some(base_url) = re.captures(url).and_then(|u| u.get(1)) {
        addr = base_url.as_str().to_string();
    }

    if let Some(socket) = addr.split_once(':') {
        if TcpListener::bind((
            socket.0,
            socket.1.to_string().parse::<u16>().unwrap_or_default(),
        ))
        .is_ok()
        {
            return true;
        }
    };

    false
}

/// Generate a vector with dates, from given range.
pub fn get_date_range(id: i32, date_range: &[String]) -> Vec<String> {
    let mut range = vec![];

    let start = match NaiveDate::parse_from_str(&date_range[0], "%Y-%m-%d") {
        Ok(s) => s,
        Err(_) => {
            error!(target: Target::file_mail(), channel = id; "date format error in: <yellow>{:?}</>", date_range[0]);
            exit(1);
        }
    };

    let end = match NaiveDate::parse_from_str(&date_range[2], "%Y-%m-%d") {
        Ok(e) => e,
        Err(_) => {
            error!(target: Target::file_mail(), channel = id; "date format error in: <yellow>{:?}</>", date_range[2]);
            exit(1);
        }
    };

    let duration = end.signed_duration_since(start);
    let days = duration.num_days() + 1;

    for day in 0..days {
        range.push(
            (start + TimeDelta::try_days(day).unwrap())
                .format("%Y-%m-%d")
                .to_string(),
        );
    }

    range
}

pub fn parse_log_level_filter(s: &str) -> Result<LevelFilter, &'static str> {
    match s.to_lowercase().as_str() {
        "debug" => Ok(LevelFilter::Debug),
        "error" => Ok(LevelFilter::Error),
        "info" => Ok(LevelFilter::Info),
        "trace" => Ok(LevelFilter::Trace),
        "warning" => Ok(LevelFilter::Warn),
        "off" => Ok(LevelFilter::Off),
        _ => Err("Error level not exists!"),
    }
}

pub fn custom_format<T: fmt::Display>(template: &str, args: &[T]) -> String {
    let mut filled_template = String::new();
    let mut arg_iter = args.iter().map(T::to_string);
    let mut template_iter = template.chars();

    while let Some(c) = template_iter.next() {
        if c == '{' {
            if let Some(nc) = template_iter.next() {
                if nc == '{' {
                    filled_template.push('{');
                } else if nc == '}' {
                    if let Some(arg) = arg_iter.next() {
                        filled_template.push_str(&arg);
                    } else {
                        filled_template.push(c);
                        filled_template.push(nc);
                    }
                } else if let Some(n) = nc.to_digit(10) {
                    filled_template.push_str(&args[n as usize].to_string());
                } else {
                    filled_template.push(nc);
                }
            }
        } else if c == '}' {
            if let Some(nc) = template_iter.next() {
                if nc == '}' {
                    filled_template.push('}');
                    continue;
                }
                filled_template.push(nc);
            }
        } else {
            filled_template.push(c);
        }
    }

    filled_template
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

pub fn fraction(d: f64, max_denominator: u32) -> (u32, u32) {
    let mut best_numerator = 1;
    let mut best_denominator = 1;
    let mut min_error = f64::MAX;

    for denominator in 1..=max_denominator {
        let numerator = (d * denominator as f64).round() as u32;
        let error = (d - (numerator as f64 / denominator as f64)).abs();

        if error < min_error {
            best_numerator = numerator;
            best_denominator = denominator;
            min_error = error;
        }
    }

    let divisor = gcd(best_numerator, best_denominator);
    (best_numerator / divisor, best_denominator / divisor)
}

pub fn calc_aspect(config: &PlayoutConfig, aspect_string: &Option<String>) -> f64 {
    let mut source_aspect = config.processing.aspect;

    if let Some(aspect) = aspect_string {
        let aspect_vec: Vec<&str> = aspect.split(':').collect();
        let w = aspect_vec[0].parse::<f64>().unwrap();
        let h = aspect_vec[1].parse::<f64>().unwrap();
        source_aspect = w / h;
    }

    source_aspect
}
