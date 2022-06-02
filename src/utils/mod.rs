use std::{
    fs::{self, metadata},
    io::{BufRead, BufReader, Error},
    path::Path,
    process::{exit, ChildStderr, Command, Stdio},
    time::{self, UNIX_EPOCH},
};

use chrono::{prelude::*, Duration};
use ffprobe::{ffprobe, Format, Stream};
use jsonrpc_http_server::hyper::HeaderMap;
use regex::Regex;
use reqwest::header;
use serde::{Deserialize, Serialize};
use serde_json::json;
use simplelog::*;

mod arg_parse;
mod config;
pub mod controller;
mod generator;
pub mod json_serializer;
mod json_validate;
mod logging;

pub use arg_parse::get_args;
pub use config::GlobalConfig;
pub use controller::{PlayerControl, PlayoutStatus, ProcessControl, ProcessUnit::*};
pub use generator::generate_playlist;
pub use json_serializer::{read_json, Playlist, DUMMY_LEN};
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

    #[serde(skip_serializing)]
    pub category: Option<String>,
    pub source: String,

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
            category: None,
            source: src.clone(),
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

    pub fn add_filter(&mut self, config: &GlobalConfig) {
        let mut node = self.clone();
        self.filter = Some(filter_chains(config, &mut node))
    }
}

/// We use the ffprobe crate, but we map the metadata to our needs.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MediaProbe {
    pub format: Option<Format>,
    pub audio_streams: Option<Vec<Stream>>,
    pub video_streams: Option<Vec<Stream>>,
}

impl MediaProbe {
    fn new(input: &str) -> Self {
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
                    audio_streams: if !a_stream.is_empty() {
                        Some(a_stream)
                    } else {
                        None
                    },
                    video_streams: if !v_stream.is_empty() {
                        Some(v_stream)
                    } else {
                        None
                    },
                }
            }
            Err(e) => {
                error!(
                    "Can't read source '{input}' with ffprobe, source is probably damaged! Error is: {e:?}"
                );

                MediaProbe {
                    format: None,
                    audio_streams: None,
                    video_streams: None,
                }
            }
        }
    }
}

/// Write current status to status file in temp folder.
///
/// The status file is init in main function and mostly modified in RPC server.
pub fn write_status(config: &GlobalConfig, date: &str, shift: f64) {
    let data = json!({
        "time_shift": shift,
        "date": date,
    });

    let status_data: String = serde_json::to_string(&data).expect("Serialize status data failed");
    if let Err(e) = fs::write(&config.general.stat_file, &status_data) {
        error!("Unable to write file: {e:?}")
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
pub fn get_delta(config: &GlobalConfig, begin: &f64) -> (f64, f64) {
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
pub fn check_sync(config: &GlobalConfig, delta: f64) -> bool {
    if delta.abs() > config.general.stop_threshold && config.general.stop_threshold > 0.0 {
        error!("Clip begin out of sync for <yellow>{delta:.3}</> seconds. Stop playout!");
        return false;
    }

    true
}

/// Create a dummy clip as a placeholder for missing video files.
pub fn gen_dummy(config: &GlobalConfig, duration: f64) -> (String, Vec<String>) {
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

/// Set clip seek in and length value.
pub fn seek_and_length(src: String, seek: f64, out: f64, duration: f64) -> Vec<String> {
    let mut source_cmd: Vec<String> = vec![];

    if seek > 0.0 {
        source_cmd.append(&mut vec!["-ss".to_string(), format!("{seek}")])
    }

    source_cmd.append(&mut vec!["-i".to_string(), src]);

    if duration > out {
        source_cmd.append(&mut vec!["-t".to_string(), format!("{}", out - seek)]);
    }

    source_cmd
}

pub fn format_log_line(line: String, level: &str) -> String {
    line.replace(&format!("[{level: >5}] "), "")
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

        for (i, param) in params.iter().enumerate() {
            output_params.push(param.clone());

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
    if is_remote(source) && MediaProbe::new(source).video_streams.is_some() {
        return true;
    }

    Path::new(&source).is_file()
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
        }
    }

    Ok(())
}

/// Run program to test if it is in system.
fn is_in_system(name: &str) {
    if let Ok(mut proc) = Command::new(name)
        .stderr(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
    {
        if let Err(e) = proc.wait() {
            error!("{e:?}")
        };
    } else {
        error!("{name} not found on system!");
        exit(0x0100);
    }
}

fn ffmpeg_libs_and_filter() -> (Vec<String>, Vec<String>) {
    let mut libs: Vec<String> = vec![];
    let mut filters: Vec<String> = vec![];

    // filter lines which contains filter
    let re: Regex = Regex::new(r"^( ?) [TSC.]+").unwrap();

    let mut ff_proc = match Command::new("ffmpeg")
        .arg("-filters")
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
    {
        Err(e) => {
            error!("couldn't spawn ffmpeg process: {}", e);
            exit(0x0100);
        }
        Ok(proc) => proc,
    };

    let err_buffer = BufReader::new(ff_proc.stderr.take().unwrap());
    let out_buffer = BufReader::new(ff_proc.stdout.take().unwrap());

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
        }
    }

    // stdout shows filter help text
    // get filters
    for line in out_buffer.lines().flatten() {
        if re.captures(line.as_str()).is_some() {
            let filter_line = line.split_whitespace();

            filters.push(filter_line.collect::<Vec<&str>>()[1].to_string());
        }
    }

    if let Err(e) = ff_proc.wait() {
        error!("{:?}", e)
    };

    (libs, filters)
}

/// Validate ffmpeg/ffprobe/ffplay.
///
/// Check if they are in system and has all filters and codecs we need.
pub fn validate_ffmpeg(config: &GlobalConfig) {
    is_in_system("ffmpeg");
    is_in_system("ffprobe");

    if config.out.mode == "desktop" {
        is_in_system("ffplay");
    }

    let (libs, filters) = ffmpeg_libs_and_filter();

    if !libs.contains(&"libx264".to_string()) {
        error!("ffmpeg contains no libx264!");
        exit(0x0100);
    }

    if !libs.contains(&"libfdk-aac".to_string()) {
        warn!("ffmpeg contains no libfdk-aac! Can't use high quality aac encoder...");
    }

    if !filters.contains(&"tpad".to_string()) {
        error!("ffmpeg contains no tpad filter!");
        exit(0x0100);
    }

    if !filters.contains(&"zmq".to_string()) {
        warn!("ffmpeg contains no zmq filter! Text messages will not work...");
    }
}

/// Get system time, in non test case.
#[cfg(not(test))]
pub fn time_now() -> DateTime<Local> {
    Local::now()
}

/// Get mocked system time, in test case.
#[cfg(test)]
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

#[cfg(test)]
pub use mock_time::time_now;
