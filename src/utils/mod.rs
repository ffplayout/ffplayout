use chrono::prelude::*;
use chrono::Duration;
use ffprobe::{ffprobe, Format, Stream};
use std::{
    fs,
    fs::metadata,
    io::{BufRead, BufReader, Error},
    path::Path,
    process::exit,
    process::{ChildStderr, Command, Stdio},
    time,
    time::UNIX_EPOCH,
};

use regex::Regex;
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
pub use config::{init_config, GlobalConfig};
pub use controller::{PlayerControl, PlayoutStatus, ProcessControl, ProcessUnit::*};
pub use generator::generate_playlist;
pub use json_serializer::{read_json, Playlist, DUMMY_LEN};
pub use json_validate::validate_playlist;
pub use logging::init_logging;

use crate::filter::filter_chains;

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
        let mut duration: f64 = 0.0;
        let mut probe = None;

        if do_probe && Path::new(&src).is_file() {
            probe = Some(MediaProbe::new(src.clone()));

            duration = match probe.clone().unwrap().format.unwrap().duration {
                Some(dur) => dur.parse().unwrap(),
                None => 0.0,
            };
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
            let probe = MediaProbe::new(self.source.clone());
            self.probe = Some(probe.clone());

            if self.duration == 0.0 {
                let duration = match probe.format.unwrap().duration {
                    Some(dur) => dur.parse().unwrap(),
                    None => 0.0,
                };

                self.out = duration;
                self.duration = duration;
            }
        }
    }

    pub fn add_filter(&mut self) {
        let mut node = self.clone();
        self.filter = Some(filter_chains(&mut node))
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
    fn new(input: String) -> Self {
        let probe = ffprobe(&input);
        let mut a_stream: Vec<Stream> = vec![];
        let mut v_stream: Vec<Stream> = vec![];

        match probe {
            Ok(obj) => {
                for stream in obj.streams {
                    let cp_stream = stream.clone();
                    match cp_stream.codec_type {
                        Some(codec_type) => {
                            if codec_type == "audio" {
                                a_stream.push(stream)
                            } else if codec_type == "video" {
                                v_stream.push(stream)
                            }
                        }

                        _ => {
                            error!("No codec type found for stream: {stream:?}")
                        }
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
pub fn write_status(date: &str, shift: f64) {
    let config = GlobalConfig::global();
    let stat_file = config.general.stat_file.clone();

    let data = json!({
        "time_shift": shift,
        "date": date,
    });

    let status_data: String = serde_json::to_string(&data).expect("Serialize status data failed");
    if let Err(e) = fs::write(stat_file, &status_data) {
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

/// Get file modification time.
pub fn modified_time(path: &str) -> Option<DateTime<Local>> {
    let metadata = metadata(path).unwrap();

    if let Ok(time) = metadata.modified() {
        let date_time: DateTime<Local> = time.into();
        return Some(date_time);
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
pub fn get_delta(begin: &f64) -> (f64, f64) {
    let config = GlobalConfig::global();
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
pub fn check_sync(delta: f64) -> bool {
    let config = GlobalConfig::global();

    if delta.abs() > config.general.stop_threshold && config.general.stop_threshold > 0.0 {
        error!("Clip begin out of sync for <yellow>{}</> seconds", delta);
        return false;
    }

    true
}

/// Create a dummy clip as a placeholder for missing video files.
pub fn gen_dummy(duration: f64) -> (String, Vec<String>) {
    let config = GlobalConfig::global();
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

/// Read ffmpeg stderr decoder, encoder and server instance
/// and log the output.
pub fn stderr_reader(buffer: BufReader<ChildStderr>, suffix: &str) -> Result<(), Error> {
    fn format_line(line: String, level: &str) -> String {
        line.replace(&format!("[{level: >5}] "), "")
    }

    for line in buffer.lines() {
        let line = line?;

        if line.contains("[info]") {
            info!("<bright black>[{suffix}]</> {}", format_line(line, "info"))
        } else if line.contains("[warning]") {
            warn!(
                "<bright black>[{suffix}]</> {}",
                format_line(line, "warning")
            )
        } else if suffix != "server"
            && !line.contains("Input/output error")
            && !line.contains("Broken pipe")
        {
            error!(
                "<bright black>[{suffix}]</> {}",
                format_line(line.clone(), "error")
            );
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
pub fn validate_ffmpeg() {
    let config = GlobalConfig::global();

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

/// In test cases we override some configuration values to fit the needs.
pub struct TestConfig {
    pub mode: String,
    pub start: String,
    pub length: String,
    pub log_to_file: bool,
    pub mail_recipient: String,
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
        let date_obj = NaiveDateTime::parse_from_str(date_time, "%Y-%m-%dT%H:%M:%S");
        let time = Local.from_local_datetime(&date_obj.unwrap()).unwrap();

        DATE_TIME_DIFF.with(|cell| *cell.borrow_mut() = Some(Local::now() - time));
    }
}

#[cfg(test)]
pub use mock_time::time_now;
