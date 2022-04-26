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
            duration: duration,
            category: None,
            source: src.clone(),
            cmd: Some(vec!["-i".to_string(), src]),
            filter: Some(vec![]),
            probe: probe,
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
                    audio_streams: if a_stream.len() > 0 {
                        Some(a_stream)
                    } else {
                        None
                    },
                    video_streams: if v_stream.len() > 0 {
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
//     let local: DateTime<Local> = Local::now();

//     local.timestamp_millis() as i64
// }

pub fn get_sec() -> f64 {
    let local: DateTime<Local> = Local::now();

    (local.hour() * 3600 + local.minute() * 60 + local.second()) as f64
        + (local.nanosecond() as f64 / 1000000000.0)
}

pub fn get_date(seek: bool, start: f64, next_start: f64) -> String {
    let local: DateTime<Local> = Local::now();

    if seek && start > get_sec() {
        return (local - Duration::days(1)).format("%Y-%m-%d").to_string();
    }

    if start == 0.0 && next_start >= 86400.0 {
        return (local + Duration::days(1)).format("%Y-%m-%d").to_string();
    }

    local.format("%Y-%m-%d").to_string()
}

pub fn modified_time(path: &str) -> Option<DateTime<Local>> {
    let metadata = metadata(path).unwrap();

    if let Ok(time) = metadata.modified() {
        let date_time: DateTime<Local> = time.into();
        return Some(date_time);
    }

    None
}

pub fn time_to_sec(time_str: &str) -> f64 {
    if ["now", "", "none"].contains(&time_str) || !time_str.contains(":") {
        return get_sec();
    }

    let t: Vec<&str> = time_str.split(':').collect();
    let h: f64 = t[0].parse().unwrap();
    let m: f64 = t[1].parse().unwrap();
    let s: f64 = t[2].parse().unwrap();

    h * 3600.0 + m * 60.0 + s
}

pub fn sec_to_time(sec: f64) -> String {
    let d = UNIX_EPOCH + time::Duration::from_millis((sec * 1000.0) as u64);
    // Create DateTime from SystemTime
    let date_time = DateTime::<Utc>::from(d);

    date_time.format("%H:%M:%S%.3f").to_string()
}

pub fn is_close(a: f64, b: f64, to: f64) -> bool {
    if (a - b).abs() < to {
        return true;
    }

    false
}

pub fn get_delta(begin: &f64) -> (f64, f64) {
    let config = GlobalConfig::global();
    let mut current_time = get_sec();
    let start = config.playlist.start_sec.unwrap();
    let length = time_to_sec(&config.playlist.length);
    let mut target_length = 86400.0;
    let total_delta;

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

    if current_time < start {
        total_delta = start - current_time;
    } else {
        total_delta = target_length + start - current_time;
    }

    (current_delta, total_delta)
}

pub fn check_sync(delta: f64) -> bool {
    let config = GlobalConfig::global();

    if delta.abs() > config.general.stop_threshold && config.general.stop_threshold > 0.0 {
        error!("Clip begin out of sync for <yellow>{}</> seconds", delta);
        return false;
    }

    true
}

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

pub fn seek_and_length(src: String, seek: f64, out: f64, duration: f64) -> Vec<String> {
    let mut source_cmd: Vec<String> = vec![];

    if seek > 0.0 {
        source_cmd.append(&mut vec!["-ss".to_string(), format!("{seek}")])
    }

    source_cmd.append(&mut vec!["-i".to_string(), src]);

    if duration > out {
        source_cmd.append(&mut vec![
            "-t".to_string(),
            format!("{}", out - seek).to_string(),
        ]);
    }

    source_cmd
}

pub fn stderr_reader(buffer: BufReader<ChildStderr>, suffix: &str) -> Result<(), Error> {
    // read ffmpeg stderr decoder, encoder and server instance
    // and log the output

    fn format_line(line: String, level: &str) -> String {
        line.replace(&format!("[{level: >5}] "), "")
    }

    // let buffer = BufReader::new(std_errors);

    for line in buffer.lines() {
        let line = line?;

        if line.contains("[info]") {
            info!("<bright black>[{suffix}]</> {}", format_line(line, "info"))
        } else if line.contains("[warning]") {
            warn!(
                "<bright black>[{suffix}]</> {}",
                format_line(line, "warning")
            )
        } else {
            if suffix != "server"
                && !line.contains("Input/output error")
                && !line.contains("Broken pipe")
            {
                error!(
                    "<bright black>[{suffix}]</> {}",
                    format_line(line.clone(), "error")
                );
            }
        }
    }

    Ok(())
}

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

    for line in err_buffer.lines() {
        if let Ok(line) = line {
            if line.contains("configuration:") {
                let configs = line.split_whitespace();

                for config in configs {
                    if config.contains("--enable-lib") {
                        libs.push(config.replace("--enable-", ""));
                    }
                }
            }
        }
    }

    for line in out_buffer.lines() {
        if let Ok(line) = line {
            if let Some(_) = re.captures(line.as_str()) {
                let filter_line = line.split_whitespace();

                filters.push(filter_line.collect::<Vec<&str>>()[1].to_string());
            }
        }
    }

    if let Err(e) = ff_proc.wait() {
        error!("{:?}", e)
    };

    (libs, filters)
}
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
