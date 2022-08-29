use std::{
    ffi::OsStr,
    fs::{self, metadata},
    io::{BufRead, BufReader, Error},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{ChildStderr, Command, Stdio},
    sync::{Arc, Mutex},
    time::{self, UNIX_EPOCH},
};

#[cfg(not(windows))]
use std::env;

use chrono::{prelude::*, Duration};
use ffprobe::{ffprobe, Format, Stream};
use jsonrpc_http_server::hyper::HeaderMap;
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
pub mod json_serializer;
mod json_validate;
mod logging;

#[cfg(windows)]
mod windows;

pub use config::{self as playout_config, PlayoutConfig, DUMMY_LEN, IMAGE_FORMAT};
pub use controller::{PlayerControl, PlayoutStatus, ProcessControl, ProcessUnit::*};
pub use generator::generate_playlist;
pub use json_serializer::{read_json, JsonPlaylist};
pub use json_validate::validate_playlist;
pub use logging::{init_logging, send_mail};

use crate::{filter::filter_chains, vec_strings};

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

    #[serde(default, deserialize_with = "null_string")]
    pub category: String,
    #[serde(deserialize_with = "null_string")]
    pub source: String,

    #[serde(default, deserialize_with = "null_string")]
    pub audio: String,

    #[serde(skip_serializing, skip_deserializing)]
    pub cmd: Option<Vec<String>>,

    #[serde(skip_serializing, skip_deserializing)]
    pub filter: Option<Vec<String>>,

    #[serde(skip_serializing, skip_deserializing)]
    pub probe: Option<MediaProbe>,

    #[serde(skip_serializing, skip_deserializing)]
    pub last_ad: Option<bool>,

    #[serde(skip_serializing, skip_deserializing)]
    pub next_ad: Option<bool>,

    #[serde(skip_serializing, skip_deserializing)]
    pub process: Option<bool>,
}

impl Media {
    pub fn new(index: usize, src: String, do_probe: bool) -> Self {
        let mut duration = 0.0;
        let mut probe = None;

        if do_probe && Path::new(&src).is_file() {
            probe = Some(MediaProbe::new(&src));

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
            source: src.clone(),
            audio: String::new(),
            cmd: Some(vec!["-i".to_string(), src]),
            filter: Some(vec![]),
            probe,
            last_ad: Some(false),
            next_ad: Some(false),
            process: Some(true),
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

    pub fn add_filter(&mut self, config: &PlayoutConfig, filter_chain: &Arc<Mutex<Vec<String>>>) {
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
    }
}

impl Eq for Media {}

fn null_string<'de, D>(d: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    Deserialize::deserialize(d).map(|x: Option<_>| x.unwrap_or_default())
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
                error!(
                    "Can't read source <b><magenta>{input}</></b> with ffprobe, source not exists or damaged! Error is: {e:?}"
                );

                MediaProbe {
                    format: None,
                    audio_streams: vec![],
                    video_streams: vec![],
                }
            }
        }
    }
}

/// Covert JSON string to ffmpeg filter command.
pub fn get_filter_from_json(raw_text: String) -> String {
    let re1 = Regex::new(r#""|}|\{"#).unwrap();
    let re2 = Regex::new(r#"id:[0-9]+,?|name:[^,]?,?"#).unwrap();
    let re3 = Regex::new(r#"text:([^,]*)"#).unwrap();
    let text = re1.replace_all(&raw_text, "");
    let text = re2.replace_all(&text, "").clone();
    let filter = re3
        .replace_all(&text, "text:'$1'")
        .replace(':', "=")
        .replace(',', ":");

    filter
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
    if let Err(e) = fs::write(&config.general.stat_file, &status_data) {
        error!(
            "Unable to write to status file <b><magenta>{}</></b>: {e}",
            config.general.stat_file
        )
    };
}

// pub fn get_timestamp() -> i64 {
//     let local: DateTime<Local> = time_now();

//     local.timestamp_millis() as i64
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

pub fn time_from_header(headers: &HeaderMap) -> Option<DateTime<Local>> {
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

    if node.seek > 0.0 {
        source_cmd.append(&mut vec_strings!["-ss", node.seek])
    }

    source_cmd.append(&mut vec_strings![
        "-ignore_chapters",
        "1",
        "-i",
        node.source.clone()
    ]);

    if Path::new(&node.audio).is_file() {
        let audio_probe = MediaProbe::new(&node.audio);

        if node.seek > 0.0 {
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
    let cmd: Vec<String> = vec![
        "-f".to_string(),
        "lavfi".to_string(),
        "-i".to_string(),
        format!(
            "{source}:r={},format=pix_fmts=yuv420p",
            config.processing.fps
        ),
        "-f".to_string(),
        "lavfi".to_string(),
        "-i".to_string(),
        format!("anoisesrc=d={duration}:c=pink:r=48000:a=0.3"),
    ];

    (source, cmd)
}

/// Prepare output parameters
///
/// seek for multiple outputs and add mapping for it
pub fn prepare_output_cmd(
    prefix: Vec<String>,
    mut filter: Vec<String>,
    params: Vec<String>,
    mode: &str,
) -> Vec<String> {
    let params_len = params.len();
    let mut output_params = params.clone();
    let mut output_a_map = "[a_out1]".to_string();
    let mut output_v_map = "[v_out1]".to_string();
    let mut output_count = 1;
    let mut cmd = prefix;

    if !filter.is_empty() {
        output_params.clear();

        for (i, p) in params.iter().enumerate() {
            let mut param = p.clone();

            param = param.replace("[0:v]", "[vout1]");
            param = param.replace("[0:a]", "[aout1]");

            if param != "-filter_complex" {
                output_params.push(param.clone());
            }

            if i > 0
                && !param.starts_with('-')
                && !params[i - 1].starts_with('-')
                && i < params_len - 1
            {
                output_count += 1;
                let mut a_map = "0:a".to_string();
                let v_map = format!("[v_out{output_count}]");
                output_v_map.push_str(v_map.as_str());

                if mode == "hls" {
                    a_map = format!("[a_out{output_count}]");
                }

                output_a_map.push_str(a_map.as_str());

                let mut map = vec!["-map".to_string(), v_map, "-map".to_string(), a_map];

                output_params.append(&mut map);
            }
        }

        if output_count > 1 && mode == "hls" {
            filter[1].push_str(format!(";[vout1]split={output_count}{output_v_map}").as_str());
            filter[1].push_str(format!(";[aout1]asplit={output_count}{output_a_map}").as_str());
            filter.drain(2..);
            cmd.append(&mut filter);
            cmd.append(&mut vec_strings!["-map", "[v_out1]", "-map", "[a_out1]"]);
        } else if output_count == 1 && mode == "hls" && output_params[0].contains("split") {
            let out_filter = output_params.remove(0);
            filter[1].push_str(format!(";{out_filter}").as_str());
            filter.drain(2..);
            cmd.append(&mut filter);
        } else if output_count > 1 && mode == "stream" {
            filter[1].push_str(format!(",split={output_count}{output_v_map}").as_str());
            cmd.append(&mut filter);
            cmd.append(&mut vec_strings!["-map", "[v_out1]", "-map", "0:a"]);
        } else {
            cmd.append(&mut filter);
        }
    }

    cmd.append(&mut output_params);

    cmd
}

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
pub fn include_file(config: PlayoutConfig, file_path: &Path) -> bool {
    let mut include = false;

    if let Some(ext) = file_extension(file_path) {
        if config.storage.extensions.contains(&ext.to_lowercase()) {
            include = true;
        }
    }

    if config.out.mode.to_lowercase() == "hls" {
        if let Some(ts_path) = config
            .out
            .output_cmd
            .clone()
            .unwrap()
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
            .unwrap()
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

pub fn format_log_line(line: String, level: &str) -> String {
    line.replace(&format!("[{level: >5}] "), "")
}

/// Read ffmpeg stderr decoder and encoder instance
/// and log the output.
pub fn stderr_reader(buffer: BufReader<ChildStderr>, suffix: &str) -> Result<(), Error> {
    for line in buffer.lines() {
        let line = line?;

        if line.contains("[info]") {
            info!(
                "<bright black>[{suffix}]</> {}",
                format_log_line(line, "info")
            )
        } else if line.contains("[warning]") {
            warn!(
                "<bright black>[{suffix}]</> {}",
                format_log_line(line, "warning")
            )
        } else if line.contains("[error]") {
            error!(
                "<bright black>[{suffix}]</> {}",
                format_log_line(line, "error")
            )
        } else if line.contains("[fatal]") {
            error!(
                "<bright black>[{suffix}]</> {}",
                format_log_line(line, "fatal")
            )
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

fn ffmpeg_libs() -> Result<Vec<String>, String> {
    let mut libs: Vec<String> = vec![];

    let mut ff_proc = match Command::new("ffmpeg").stderr(Stdio::piped()).spawn() {
        Err(e) => {
            return Err(format!("couldn't spawn ffmpeg process: {e}"));
        }
        Ok(proc) => proc,
    };

    let err_buffer = BufReader::new(ff_proc.stderr.take().unwrap());

    // stderr shows only the ffmpeg configuration
    // get codec library's
    for line in err_buffer.lines().flatten() {
        if line.contains("configuration:") {
            let configs = line.split_whitespace();

            for config in configs {
                if config.contains("--enable-lib") {
                    libs.push(config.replace("--enable-", ""));
                }
            }
            break;
        }
    }

    if let Err(e) = ff_proc.wait() {
        error!("{:?}", e)
    };

    Ok(libs)
}

/// Validate ffmpeg/ffprobe/ffplay.
///
/// Check if they are in system and has all libs and codecs we need.
pub fn validate_ffmpeg(config: &PlayoutConfig) -> Result<(), String> {
    is_in_system("ffmpeg")?;
    is_in_system("ffprobe")?;

    if config.out.mode == "desktop" {
        is_in_system("ffplay")?;
    }

    let libs = ffmpeg_libs()?;

    if !libs.contains(&"libx264".to_string()) {
        return Err("ffmpeg contains no libx264!".to_string());
    }

    if config.text.add_text
        && !config.text.text_from_filename
        && !libs.contains(&"libzmq".to_string())
    {
        return Err(
            "ffmpeg contains no libzmq! Disable add_text in config or compile ffmpeg with libzmq."
                .to_string(),
        );
    }

    if !libs.contains(&"libfdk-aac".to_string()) {
        warn!("ffmpeg contains no libfdk-aac! Can't use high quality aac encoder...");
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
    let raw_addr = url.split('/').collect::<Vec<&str>>();

    if raw_addr.len() > 1 {
        if let Some(socket) = raw_addr[2].split_once(':') {
            if TcpListener::bind((
                socket.0,
                socket.1.to_string().parse::<u16>().unwrap_or_default(),
            ))
            .is_ok()
            {
                return true;
            }
        };
    }

    error!("Address <b><magenta>{url}</></b> already in use!");

    false
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
