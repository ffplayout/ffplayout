use chrono::prelude::*;
use chrono::Duration;
use ffprobe::{ffprobe, Format, Stream};
use serde::{Deserialize, Serialize};
use std::fs::metadata;
use std::time::{UNIX_EPOCH};
use std::time;

mod arg_parse;
mod config;
// mod folder;
mod json_reader;
mod playlist;

pub use arg_parse::get_args;
pub use config::{get_config, Config};
// pub use folder::walk;
pub use json_reader::{read_json, Program};
pub use playlist::program;

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
                            println!("No codec type found for stream: {:?}", &stream)
                        }
                    }
                }

                MediaProbe {
                    format: Some(obj.format),
                    audio_streams: if a_stream.len() > 0 {Some(a_stream)} else {None},
                    video_streams: if v_stream.len() > 0 {Some(v_stream)} else {None},
                }
            }
            Err(err) => {
                println!(
                    "Can't read source '{}' with ffprobe, source is probably damaged! Error is: {:?}",
                    input,
                    err
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

// pub fn get_timestamp() -> i64 {
//     let local: DateTime<Local> = Local::now();

//     local.timestamp_millis() as i64
// }

pub fn get_sec() -> f64 {
    let local: DateTime<Local> = Local::now();

    (local.hour() * 3600 + local.minute() * 60 + local.second()) as f64
        + (local.nanosecond() as f64 / 1000000000.0)
}

pub fn get_date(seek: bool, start: f64, next: f64) -> String {
    let local: DateTime<Local> = Local::now();

    if seek && start > get_sec() {
        return (local - Duration::days(1)).format("%Y-%m-%d").to_string();
    }

    if start == 0.0 && next >= 86400.0 {
        return (local + Duration::days(1)).format("%Y-%m-%d").to_string();
    }

    local.format("%Y-%m-%d").to_string()
}

pub fn modified_time(path: String) -> Option<DateTime<Local>> {
    let metadata = metadata(path).unwrap();

    if let Ok(time) = metadata.modified() {
        let date_time: DateTime<Local> = time.into();
        return Some(date_time);
    }

    None
}

pub fn sec_to_time(sec: f64) -> String {
    let d = UNIX_EPOCH + time::Duration::from_secs(sec as u64);
    // Create DateTime from SystemTime
    let date_time = DateTime::<Utc>::from(d);

    date_time.format("%H:%M:%S").to_string()
}

pub fn is_close(a: f64, b: f64, to: f64) -> bool {
    if (a - b).abs() > to {
        return true
    }

    false
}
