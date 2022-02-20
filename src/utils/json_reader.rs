use serde::{Deserialize, Serialize};
use std::{fs::File, path::Path};

use crate::utils::Config;
use crate::utils::{get_date, get_sec, modified_time, MediaProbe};

#[derive(Debug, Serialize, Deserialize)]
pub struct Playlist {
    pub date: String,
    pub current_file: Option<String>,
    pub start_index: Option<usize>,
    pub modified: Option<String>,
    pub program: Vec<Program>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Program {
    pub begin: Option<f64>,
    pub index: Option<usize>,
    #[serde(rename = "in")]
    pub seek: f64,
    pub out: f64,
    pub duration: f64,
    pub category: String,
    pub source: String,
    pub cmd: Option<Vec<String>>,
    pub filter: Option<Vec<String>>,
    pub probe: Option<MediaProbe>,
    pub last_ad: Option<bool>,
    pub next_ad: Option<bool>,
}

pub fn read_json(config: &Config, seek: bool) -> Playlist {
    let mut playlist_path = Path::new(&config.playlist.path).to_owned();
    let start = &config.playlist.day_start;
    let length = &config.playlist.length;
    let t: Vec<&str> = start.split(':').collect();
    let h: f64 = t[0].parse().unwrap();
    let m: f64 = t[1].parse().unwrap();
    let s: f64 = t[2].parse().unwrap();
    let mut start_sec = h * 3600.0 + m * 60.0 + s;
    let mut length_sec: f64 = 86400.0;
    let mut seek_first = seek;

    if playlist_path.is_dir() {
        let date = get_date(true, start_sec, 0.0);
        let d: Vec<&str> = date.split('-').collect();
        playlist_path = playlist_path
            .join(d[0])
            .join(d[1])
            .join(date)
            .with_extension("json");
    }

    let current_file: String = playlist_path.as_path().display().to_string();

    if length.contains(":") {
        let t: Vec<&str> = length.split(':').collect();
        let h: f64 = t[0].parse().unwrap();
        let m: f64 = t[1].parse().unwrap();
        let s: f64 = t[2].parse().unwrap();
        length_sec = h * 3600.0 + m * 60.0 + s;
    }

    println!("Read Playlist: {}", &current_file);

    let modify = modified_time(current_file.clone());
    let f = File::open(&current_file).expect("Could not open json playlist file.");
    let mut playlist: Playlist =
        serde_json::from_reader(f).expect("Could not read json playlist file.");

    playlist.current_file = Some(current_file);

    if modify.is_some() {
        playlist.modified = Some(modify.unwrap().to_string());
    }

    let mut time_sec = get_sec();

    if seek_first && time_sec < start_sec {
        time_sec += length_sec
    }

    for (i, item) in playlist.program.iter_mut().enumerate() {
        item.begin = Some(start_sec);
        item.index = Some(i);
        let mut source_cmd: Vec<String> = vec![];

        if seek_first && start_sec + item.out - item.seek > time_sec {
            seek_first = false;
            playlist.start_index = Some(i);
            item.seek = time_sec - start_sec;
        }

        if item.seek > 0.0 {
            source_cmd.append(&mut vec![
                "-ss".to_string(),
                format!("{}", item.seek).to_string(),
                "-i".to_string(),
                item.source.clone(),
            ])
        } else {
            source_cmd.append(&mut vec!["-i".to_string(), item.source.clone()])
        }

        if item.duration > item.out {
            source_cmd.append(&mut vec![
                "-t".to_string(),
                format!("{}", item.out - item.seek).to_string(),
            ]);
        }

        item.cmd = Some(source_cmd);
        start_sec += item.out - item.seek;
    }

    // println!("{:#?}", playlist);

    playlist
}
