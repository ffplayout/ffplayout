use std::{
    ffi::OsStr,
    fmt,
    fs::{self, metadata, File},
    io::{BufRead, BufReader, Error},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{exit, ChildStderr, Command, Stdio},
    str::FromStr,
    sync::{Arc, Mutex},
};

use chrono::{prelude::*, TimeDelta};
use ffprobe::{ffprobe, Stream as FFStream};
use rand::prelude::*;
use regex::Regex;
use reqwest::header;
use serde::{de::Deserializer, Deserialize, Serialize};
use serde_json::json;
use simplelog::*;

pub mod advanced_config;
pub mod config;
pub mod controller;
pub mod errors;
pub mod folder;
pub mod generator;
pub mod import;
pub mod json_serializer;
mod json_validate;
mod logging;

pub use config::{
    self as playout_config,
    OutputMode::{self, *},
    PlayoutConfig,
    ProcessMode::{self, *},
    Template, DUMMY_LEN, FFMPEG_IGNORE_ERRORS, FFMPEG_UNRECOVERABLE_ERRORS, IMAGE_FORMAT,
};
pub use controller::{
    PlayerControl, PlayoutStatus, ProcessControl,
    ProcessUnit::{self, *},
};
use errors::ProcError;
pub use generator::generate_playlist;
pub use json_serializer::{read_json, JsonPlaylist};
pub use json_validate::validate_playlist;
pub use logging::{init_logging, send_mail};

use crate::{
    filter::{filter_chains, Filters},
    vec_strings,
};

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

    #[serde(skip_serializing, skip_deserializing)]
    pub process: Option<bool>,

    #[serde(default, skip_serializing)]
    pub unit: ProcessUnit,
}

impl Media {
    pub fn new(index: usize, src: &str, do_probe: bool) -> Self {
        let mut duration = 0.0;
        let mut probe = None;

        if do_probe && (is_remote(src) || Path::new(src).is_file()) {
            if let Ok(p) = MediaProbe::new(src) {
                probe = Some(p.clone());

                duration = p
                    .format
                    .duration
                    .unwrap_or_default()
                    .parse()
                    .unwrap_or_default();
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
            process: Some(true),
            unit: Decoder,
        }
    }

    pub fn add_probe(&mut self, check_audio: bool) -> Result<(), String> {
        let mut errors = vec![];

        if self.probe.is_none() {
            match MediaProbe::new(&self.source) {
                Ok(probe) => {
                    self.probe = Some(probe.clone());

                    if let Some(dur) = probe
                        .format
                        .duration
                        .map(|d| d.parse().unwrap_or_default())
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
                match MediaProbe::new(&self.audio) {
                    Ok(probe) => {
                        self.probe_audio = Some(probe.clone());

                        if !probe.audio_streams.is_empty() {
                            self.duration_audio = probe.audio_streams[0]
                                .duration
                                .clone()
                                .and_then(|d| d.parse::<f64>().ok())
                                .unwrap_or_default()
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

    pub fn add_filter(
        &mut self,
        config: &PlayoutConfig,
        filter_chain: &Option<Arc<Mutex<Vec<String>>>>,
    ) {
        let mut node = self.clone();
        self.filter = Some(filter_chains(config, &mut node, filter_chain))
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

/// We use the ffprobe crate, but we map the metadata to our needs.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct MediaProbe {
    pub format: ffprobe::Format,
    pub audio_streams: Vec<FFStream>,
    pub video_streams: Vec<FFStream>,
}

impl MediaProbe {
    pub fn new(input: &str) -> Result<Self, ProcError> {
        let probe = ffprobe(input);
        let mut a_stream = vec![];
        let mut v_stream = vec![];

        match probe {
            Ok(obj) => {
                for stream in obj.streams {
                    let cp_stream = stream.clone();

                    if let Some(c_type) = cp_stream.codec_type {
                        match c_type.as_str() {
                            "audio" => a_stream.push(stream),
                            "video" => v_stream.push(stream),
                            _ => {}
                        }
                    } else {
                        error!("No codec type found for stream: {stream:?}")
                    }
                }

                Ok(MediaProbe {
                    format: obj.format,
                    audio_streams: a_stream,
                    video_streams: v_stream,
                })
            }
            Err(e) => {
                if !Path::new(input).is_file() && !is_remote(input) {
                    Err(ProcError::Custom(format!(
                        "File <b><magenta>{input}</></b> not exist!"
                    )))
                } else {
                    Err(ProcError::Ffprobe(e))
                }
            }
        }
    }
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

pub fn json_reader(path: &PathBuf) -> Result<JsonPlaylist, Error> {
    let f = File::options().read(true).write(false).open(path)?;
    let p = serde_json::from_reader(f)?;

    Ok(p)
}

pub fn json_writer(path: &PathBuf, data: JsonPlaylist) -> Result<(), Error> {
    let f = File::options()
        .write(true)
        .truncate(true)
        .create(true)
        .open(path)?;
    serde_json::to_writer_pretty(f, &data)?;

    Ok(())
}

/// Write current status to status file in temp folder.
///
/// The status file is init in main function and mostly modified in RPC server.
pub fn write_status(config: &PlayoutConfig, date: &str, shift: f64) {
    let data = json!({
        "time_shift": shift,
        "date": date,
    });

    match serde_json::to_string(&data) {
        Ok(status) => {
            if let Err(e) = fs::write(&config.general.stat_file, status) {
                error!(
                    "Unable to write to status file <b><magenta>{}</></b>: {e}",
                    config.general.stat_file
                )
            };
        }
        Err(e) => error!("Serialize status data failed: {e}"),
    };
}

// pub fn get_timestamp() -> i32 {
//     let local: DateTime<Local> = time_now();

//     local.timestamp_millis() as i32
// }

/// Get current time in seconds.
pub fn time_in_seconds() -> f64 {
    let local: DateTime<Local> = time_now();

    (local.hour() * 3600 + local.minute() * 60 + local.second()) as f64
        + (local.nanosecond() as f64 / 1000000000.0)
}

/// Get current date for playlist, but check time with conditions:
///
/// - When time is before playlist start, get date from yesterday.
/// - When given next_start is over target length (normally a full day), get date from tomorrow.
pub fn get_date(seek: bool, start: f64, get_next: bool) -> String {
    let local: DateTime<Local> = time_now();

    if seek && start > time_in_seconds() {
        return (local - TimeDelta::try_days(1).unwrap())
            .format("%Y-%m-%d")
            .to_string();
    }

    if start == 0.0 && get_next && time_in_seconds() > 86397.9 {
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
pub fn modified_time(path: &str) -> Option<String> {
    if is_remote(path) {
        let response = reqwest::blocking::Client::new().head(path).send();

        if let Ok(resp) = response {
            if resp.status().is_success() {
                if let Some(time) = time_from_header(resp.headers()) {
                    return Some(time.to_string());
                }
            }
        }

        return None;
    }

    if let Ok(time) = metadata(path).and_then(|metadata| metadata.modified()) {
        let date_time: DateTime<Local> = time.into();
        return Some(date_time.to_string());
    }

    None
}

/// Convert a formatted time string to seconds.
pub fn time_to_sec(time_str: &str) -> f64 {
    if matches!(time_str, "now" | "" | "none") || !time_str.contains(':') {
        return time_in_seconds();
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
pub fn is_close<T: num_traits::Signed + std::cmp::PartialOrd>(a: T, b: T, to: T) -> bool {
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
    let mut current_time = time_in_seconds();
    let start = config.playlist.start_sec.unwrap();
    let length = config.playlist.length_sec.unwrap_or(86400.0);
    let mut target_length = 86400.0;

    if length > 0.0 && length != target_length {
        target_length = length
    }

    if begin == &start && start == 0.0 && 86400.0 - current_time < 4.0 {
        current_time -= 86400.0
    } else if start >= current_time && begin != &start {
        current_time += 86400.0
    }

    let mut current_delta = begin - current_time;

    if is_close(
        current_delta.abs(),
        86400.0,
        config.general.stop_threshold + 2.0,
    ) {
        current_delta = current_delta.abs() - 86400.0
    }

    let total_delta = if current_time < start {
        start - current_time
    } else {
        target_length + start - current_time
    };

    (current_delta, total_delta)
}

/// Loop image until target duration is reached.
pub fn loop_image(node: &Media) -> Vec<String> {
    let duration = node.out - node.seek;
    let mut source_cmd: Vec<String> = vec_strings!["-loop", "1", "-i", node.source.clone()];

    info!(
        "Loop image <b><magenta>{}</></b>, total duration: <yellow>{duration:.2}</>",
        node.source
    );

    if Path::new(&node.audio).is_file() {
        if node.seek > 0.0 {
            source_cmd.append(&mut vec_strings!["-ss", node.seek])
        }

        source_cmd.append(&mut vec_strings!["-i", node.audio.clone()]);
    }

    source_cmd.append(&mut vec_strings!["-t", duration]);

    source_cmd
}

/// Loop filler until target duration is reached.
pub fn loop_filler(node: &Media) -> Vec<String> {
    let loop_count = (node.out / node.duration).ceil() as i32;
    let mut source_cmd = vec![];

    if loop_count > 1 {
        info!("Loop <b><magenta>{}</></b> <yellow>{loop_count}</> times, total duration: <yellow>{:.2}</>", node.source, node.out);

        source_cmd.append(&mut vec_strings!["-stream_loop", loop_count]);
    }

    source_cmd.append(&mut vec_strings!["-i", node.source, "-t", node.out]);

    source_cmd
}

/// Set clip seek in and length value.
pub fn seek_and_length(node: &mut Media) -> Vec<String> {
    let loop_count = (node.out / node.duration).ceil() as i32;
    let mut source_cmd = vec![];
    let mut cut_audio = false;
    let mut loop_audio = false;
    let remote_source = is_remote(&node.source);

    if remote_source && node.probe.clone().and_then(|f| f.format.duration).is_none() {
        node.out -= node.seek;
        node.seek = 0.0;
    } else if node.seek > 0.5 {
        source_cmd.append(&mut vec_strings!["-ss", node.seek])
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

    source_cmd
}

/// Create a dummy clip as a placeholder for missing video files.
pub fn gen_dummy(config: &PlayoutConfig, duration: f64) -> (String, Vec<String>) {
    let color = "#121212";
    let source = format!(
        "color=c={color}:s={}x{}:d={duration}",
        config.processing.width, config.processing.height
    );
    let cmd: Vec<String> = vec_strings![
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

    (source, cmd)
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

    if config.out.mode == HLS {
        if let Some(ts_path) = config
            .out
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
            .out
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
pub fn stderr_reader(
    buffer: BufReader<ChildStderr>,
    ignore: Vec<String>,
    suffix: ProcessUnit,
    proc_control: ProcessControl,
) -> Result<(), Error> {
    for line in buffer.lines() {
        let line = line?;

        if FFMPEG_IGNORE_ERRORS.iter().any(|i| line.contains(*i))
            || ignore.iter().any(|i| line.contains(i))
        {
            continue;
        }

        if line.contains("[info]") {
            info!(
                "<bright black>[{suffix}]</> {}",
                line.replace("[info] ", "")
            )
        } else if line.contains("[warning]") {
            warn!(
                "<bright black>[{suffix}]</> {}",
                line.replace("[warning] ", "")
            )
        } else if line.contains("[error]") || line.contains("[fatal]") {
            error!(
                "<bright black>[{suffix}]</> {}",
                line.replace("[error] ", "").replace("[fatal] ", "")
            );

            if FFMPEG_UNRECOVERABLE_ERRORS
                .iter()
                .any(|i| line.contains(*i))
                || (line.contains("No such file or directory")
                    && !line.contains("failed to delete old segment"))
            {
                proc_control.stop_all();
                exit(1);
            }
        }
    }

    Ok(())
}

/// Run program to test if it is in system.
fn is_in_system(name: &str) -> Result<(), String> {
    match Command::new(name)
        .stderr(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
    {
        Ok(mut proc) => {
            if let Err(e) = proc.wait() {
                return Err(format!("{e}"));
            };
        }
        Err(e) => return Err(format!("{name} not found on system! {e}")),
    }

    Ok(())
}

fn ffmpeg_filter_and_libs(config: &mut PlayoutConfig) -> Result<(), String> {
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
    for line in err_buffer.lines().map_while(Result::ok) {
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
    for line in out_buffer.lines().map_while(Result::ok) {
        if line.contains('>') {
            let filter_line = line.split_whitespace().collect::<Vec<_>>();

            if filter_line.len() > 2 {
                config
                    .general
                    .ffmpeg_filters
                    .push(filter_line[1].to_string())
            }
        }
    }

    if let Err(e) = ff_proc.wait() {
        error!("{e}")
    };

    Ok(())
}

/// Validate ffmpeg/ffprobe/ffplay.
///
/// Check if they are in system and has all libs and codecs we need.
pub fn validate_ffmpeg(config: &mut PlayoutConfig) -> Result<(), String> {
    is_in_system("ffmpeg")?;
    is_in_system("ffprobe")?;

    if config.out.mode == Desktop {
        is_in_system("ffplay")?;
    }

    ffmpeg_filter_and_libs(config)?;

    if config
        .out
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
        .out
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
pub fn free_tcp_socket(exclude_socket: String) -> Option<String> {
    for _ in 0..100 {
        let port = rand::thread_rng().gen_range(45321..54268);
        let socket = format!("127.0.0.1:{port}");

        if socket != exclude_socket && TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return Some(socket);
        }
    }

    None
}

/// check if tcp port is free
pub fn test_tcp_port(url: &str) -> bool {
    let re = Regex::new(r"^[\w]+\://").unwrap();
    let mut addr = url.to_string();

    if re.is_match(url) {
        addr = re.replace(url, "").to_string();
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

    error!("Address <b><magenta>{url}</></b> already in use!");

    false
}

/// Generate a vector with dates, from given range.
pub fn get_date_range(date_range: &[String]) -> Vec<String> {
    let mut range = vec![];

    let start = match NaiveDate::parse_from_str(&date_range[0], "%Y-%m-%d") {
        Ok(s) => s,
        Err(_) => {
            error!("date format error in: <yellow>{:?}</>", date_range[0]);
            exit(1);
        }
    };

    let end = match NaiveDate::parse_from_str(&date_range[2], "%Y-%m-%d") {
        Ok(e) => e,
        Err(_) => {
            error!("date format error in: <yellow>{:?}</>", date_range[2]);
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
    let mut arg_iter = args.iter().map(|x| format!("{}", x));
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
                } else {
                    filled_template.push(nc);
                }
            }
        } else {
            filled_template.push(c);
        }
    }

    filled_template
}

/// Get system time, in non test/debug case.
#[cfg(not(any(test, debug_assertions)))]
pub fn time_now() -> DateTime<Local> {
    Local::now()
}

/// Get mocked system time, in test/debug case.
#[cfg(any(test, debug_assertions))]
pub mod mock_time {
    use super::*;
    use std::cell::RefCell;

    thread_local! {
        static DATE_TIME_DIFF: RefCell<Option<TimeDelta>> = const { RefCell::new(None) };
    }

    pub fn time_now() -> DateTime<Local> {
        DATE_TIME_DIFF.with(|cell| match cell.borrow().as_ref().cloned() {
            Some(diff) => Local::now() - diff,
            None => Local::now(),
        })
    }

    pub fn set_mock_time(date_time: &str) {
        if let Ok(d) = NaiveDateTime::parse_from_str(date_time, "%Y-%m-%dT%H:%M:%S") {
            let time = Local.from_local_datetime(&d).unwrap();

            DATE_TIME_DIFF.with(|cell| *cell.borrow_mut() = Some(Local::now() - time));
        }
    }
}

#[cfg(any(test, debug_assertions))]
pub use mock_time::time_now;
