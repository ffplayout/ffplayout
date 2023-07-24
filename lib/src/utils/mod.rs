use std::{
    ffi::OsStr,
    fs::{self, metadata, File},
    io::{BufRead, BufReader, Error},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{exit, ChildStderr, Command, Stdio},
    sync::{Arc, Mutex},
    time::{self, UNIX_EPOCH},
};

#[cfg(not(windows))]
use std::env;

use chrono::{prelude::*, Duration};
use ffprobe::{ffprobe, Format, Stream};
use rand::prelude::*;
use regex::Regex;
use reqwest::header;
use serde::{de::Deserializer, Deserialize, Serialize};
use serde_json::json;
use simplelog::*;

pub mod config;
pub mod controller;
pub mod folder;
mod generator;
pub mod import;
pub mod json_serializer;
mod json_validate;
mod logging;

#[cfg(windows)]
mod windows;

pub use config::{
    self as playout_config,
    OutputMode::{self, *},
    PlayoutConfig,
    ProcessMode::{self, *},
    DUMMY_LEN, FFMPEG_IGNORE_ERRORS, FFMPEG_UNRECOVERABLE_ERRORS, IMAGE_FORMAT,
};
pub use controller::{
    PlayerControl, PlayoutStatus, ProcessControl,
    ProcessUnit::{self, *},
};
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
    #[serde(rename = "in")]
    pub seek: f64,
    pub out: f64,
    pub duration: f64,

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
    pub last_ad: Option<bool>,

    #[serde(skip_serializing, skip_deserializing)]
    pub next_ad: Option<bool>,

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
            probe = Some(MediaProbe::new(src));

            if let Some(dur) = probe
                .as_ref()
                .and_then(|p| p.format.as_ref())
                .and_then(|f| f.duration.as_ref())
            {
                duration = dur.parse().unwrap()
            }
        }

        Self {
            begin: None,
            index: Some(index),
            seek: 0.0,
            out: duration,
            duration,
            category: String::new(),
            source: src.to_string(),
            audio: String::new(),
            cmd: Some(vec_strings!["-i", src]),
            filter: None,
            custom_filter: String::new(),
            probe,
            last_ad: Some(false),
            next_ad: Some(false),
            process: Some(true),
            unit: Decoder,
        }
    }

    pub fn add_probe(&mut self) {
        if self.probe.is_none() {
            let probe = MediaProbe::new(&self.source);
            self.probe = Some(probe.clone());

            if let Some(dur) = probe
                .format
                .and_then(|f| f.duration)
                .map(|d| d.parse().unwrap())
                .filter(|d| !is_close(*d, self.duration, 0.5))
            {
                self.duration = dur;

                if self.out == 0.0 {
                    self.out = dur;
                }
            }
        }
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
        self.seek == other.seek
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
    pub format: Option<Format>,
    pub audio_streams: Vec<Stream>,
    pub video_streams: Vec<Stream>,
}

impl MediaProbe {
    pub fn new(input: &str) -> Self {
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

                MediaProbe {
                    format: Some(obj.format),
                    audio_streams: a_stream,
                    video_streams: v_stream,
                }
            }
            Err(e) => {
                if Path::new(input).is_file() {
                    error!(
                        "Can't read source <b><magenta>{input}</></b> with ffprobe! Error: {e:?}"
                    );
                } else if !input.is_empty() {
                    error!("File not exists: <b><magenta>{input}</></b>");
                }

                MediaProbe {
                    format: None,
                    audio_streams: vec![],
                    video_streams: vec![],
                }
            }
        }
    }
}

/// Calculate fps from rate/factor string
pub fn fps_calc(r_frame_rate: &str, default: f64) -> f64 {
    let mut fps = default;

    if let Some((r, f)) = r_frame_rate.split_once('/') {
        fps = r.parse::<f64>().unwrap_or(1.0) / f.parse::<f64>().unwrap_or(1.0);
    }

    fps
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

    let status_data: String = serde_json::to_string(&data).expect("Serialize status data failed");
    if let Err(e) = fs::write(&config.general.stat_file, status_data) {
        error!(
            "Unable to write to status file <b><magenta>{}</></b>: {e}",
            config.general.stat_file
        )
    };
}

// pub fn get_timestamp() -> i32 {
//     let local: DateTime<Local> = time_now();

//     local.timestamp_millis() as i32
// }

/// Get current time in seconds.
pub fn get_sec() -> f64 {
    let local: DateTime<Local> = time_now();

    (local.hour() * 3600 + local.minute() * 60 + local.second()) as f64
        + (local.nanosecond() as f64 / 1000000000.0)
}

/// Get current date for playlist, but check time with conditions:
///
/// - When time is before playlist start, get date from yesterday.
/// - When given next_start is over target length (normally a full day), get date from tomorrow.
pub fn get_date(seek: bool, start: f64, next_start: f64) -> String {
    let local: DateTime<Local> = time_now();

    if seek && start > get_sec() {
        return (local - Duration::days(1)).format("%Y-%m-%d").to_string();
    }

    if start == 0.0 && next_start >= 86400.0 {
        return (local + Duration::days(1)).format("%Y-%m-%d").to_string();
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
    if ["now", "", "none"].contains(&time_str) || !time_str.contains(':') {
        return get_sec();
    }

    let t: Vec<&str> = time_str.split(':').collect();
    let h: f64 = t[0].parse().unwrap();
    let m: f64 = t[1].parse().unwrap();
    let s: f64 = t[2].parse().unwrap();

    h * 3600.0 + m * 60.0 + s
}

/// Convert floating number (seconds) to a formatted time string.
pub fn sec_to_time(sec: f64) -> String {
    let d = UNIX_EPOCH + time::Duration::from_millis((sec * 1000.0) as u64);
    // Create DateTime from SystemTime
    let date_time = DateTime::<Utc>::from(d);

    date_time.format("%H:%M:%S%.3f").to_string()
}

/// get file extension
pub fn file_extension(filename: &Path) -> Option<&str> {
    filename.extension().and_then(OsStr::to_str)
}

/// Test if given numbers are close to each other,
/// with a third number for setting the maximum range.
pub fn is_close(a: f64, b: f64, to: f64) -> bool {
    if (a - b).abs() < to {
        return true;
    }

    false
}

/// Get delta between clip start and current time. This value we need to check,
/// if we still in sync.
///
/// We also get here the global delta between clip start and time when a new playlist should start.
pub fn get_delta(config: &PlayoutConfig, begin: &f64) -> (f64, f64) {
    let mut current_time = get_sec();
    let start = config.playlist.start_sec.unwrap();
    let length = time_to_sec(&config.playlist.length);
    let mut target_length = 86400.0;

    if length > 0.0 && length != target_length {
        target_length = length
    }
    if begin == &start && start == 0.0 && 86400.0 - current_time < 4.0 {
        current_time -= target_length
    } else if start >= current_time && begin != &start {
        current_time += target_length
    }

    let mut current_delta = begin - current_time;

    if is_close(current_delta, 86400.0, config.general.stop_threshold) {
        current_delta -= 86400.0
    }

    let total_delta = if current_time < start {
        start - current_time
    } else {
        target_length + start - current_time
    };

    (current_delta, total_delta)
}

/// Check if clip in playlist is in sync with global time.
pub fn check_sync(config: &PlayoutConfig, delta: f64) -> bool {
    if delta.abs() > config.general.stop_threshold && config.general.stop_threshold > 0.0 {
        error!("Clip begin out of sync for <yellow>{delta:.3}</> seconds. Stop playout!");
        return false;
    }

    true
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
pub fn seek_and_length(node: &Media) -> Vec<String> {
    let mut source_cmd = vec![];
    let mut cut_audio = false;

    if node.seek > 0.5 {
        source_cmd.append(&mut vec_strings!["-ss", node.seek])
    }

    source_cmd.append(&mut vec_strings!["-i", node.source.clone()]);

    if Path::new(&node.audio).is_file() {
        let audio_probe = MediaProbe::new(&node.audio);

        if node.seek > 0.5 {
            source_cmd.append(&mut vec_strings!["-ss", node.seek])
        }

        source_cmd.append(&mut vec_strings!["-i", node.audio.clone()]);

        if !audio_probe.audio_streams.is_empty()
            && audio_probe.audio_streams[0]
                .duration
                .clone()
                .and_then(|d| d.parse::<f64>().ok())
                > Some(node.out - node.seek)
        {
            cut_audio = true;
        }
    }

    if node.duration > node.out || cut_audio {
        source_cmd.append(&mut vec_strings!["-t", node.out - node.seek]);
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
    Regex::new(r"^https?://.*").unwrap().is_match(path)
}

/// Validate input
///
/// Check if input is a remote source, or from storage and see if it exists.
pub fn valid_source(source: &str) -> bool {
    if is_remote(source) && !MediaProbe::new(source).video_streams.is_empty() {
        return true;
    }

    Path::new(&source).is_file()
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
    suffix: ProcessUnit,
    proc_control: ProcessControl,
) -> Result<(), Error> {
    for line in buffer.lines() {
        let line = line?;

        if FFMPEG_IGNORE_ERRORS.iter().any(|i| line.contains(*i)) {
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
    for line in err_buffer.lines().flatten() {
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
    for line in out_buffer.lines().flatten() {
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
        error!("{:?}", e)
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
        range.push((start + Duration::days(day)).format("%Y-%m-%d").to_string());
    }

    range
}

pub fn home_dir() -> Option<PathBuf> {
    home_dir_inner()
}

#[cfg(windows)]
use windows::home_dir_inner;

#[cfg(any(unix, target_os = "redox"))]
fn home_dir_inner() -> Option<PathBuf> {
    #[allow(deprecated)]
    env::home_dir()
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
        static DATE_TIME_DIFF: RefCell<Option<Duration>> = RefCell::new(None);
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
