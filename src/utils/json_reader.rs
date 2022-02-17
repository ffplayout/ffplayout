use serde::{Deserialize, Serialize};
use std::fs::File;
use std::path::Path;

use crate::utils::Config;
use crate::utils::{get_date, get_sec, modified_time};

#[derive(Debug, Serialize, Deserialize)]
pub struct Playlist {
    pub date: String,
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
}

pub fn read_json(config: &Config, seek: bool) -> Playlist {
    let mut playlist_path = Path::new(&config.playlist.path).to_owned();
    let start = &config.playlist.day_start;
    let t: Vec<&str> = start.split(':').collect();
    let h: f64 = t[0].parse().unwrap();
    let m: f64 = t[1].parse().unwrap();
    let s: f64 = t[2].parse().unwrap();
    let mut start_sec = h * 3600.0 + m * 60.0 + s;
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

    println!(
        "Read Playlist: {}",
        playlist_path.as_path().display().to_string()
    );

    let modify = modified_time(playlist_path.as_path().display().to_string());
    let f = File::open(playlist_path).expect("Could not open json playlist file.");
    let mut playlist: Playlist =
        serde_json::from_reader(f).expect("Could not read json playlist file.");

    if modify.is_some() {
        playlist.modified = Some(modify.unwrap().to_string());
    }

    let time_sec = get_sec();

    for (i, item) in playlist.program.iter_mut().enumerate() {
        item.begin = Some(start_sec);
        item.index = Some(i);

        if seek_first && item.begin.unwrap() + (item.out - item.seek) > time_sec {
            seek_first = false;
            playlist.start_index = Some(i);
            item.seek = time_sec - item.begin.unwrap();
        }

        if item.seek > 0.0 {
            item.cmd = Some(vec![
                "-ss".to_string(),
                format!("{}", item.seek).to_string(),
                "-i".to_string(),
                item.source.clone(),
            ])
        } else {
            item.cmd = Some(vec![
                "-i".to_string(),
                item.source.clone(),
            ])
        }

        start_sec += item.out - item.seek;
    }

    println!("{:#?}", playlist);

    playlist
}
