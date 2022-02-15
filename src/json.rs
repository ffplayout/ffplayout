use serde::{Deserialize, Serialize};
use std::fs::File;
use std::path::Path;

use crate::config::Config;
use crate::utils::{get_date, modified_time};

#[derive(Debug, Serialize, Deserialize)]
pub struct Playlist {
    pub date: String,
    pub modified: Option<String>,
    pub program: Vec<Program>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Program {
    #[serde(rename = "in")]
    pub seek: f32,
    pub out: f32,
    pub duration: f32,
    pub category: String,
    pub source: String,
}

pub fn read(config: &Config) -> Playlist {
    let mut playlist_path = Path::new(&config.playlist.path).to_owned();
    let start = &config.playlist.day_start;

    if playlist_path.is_dir() {
        let t: Vec<&str> = start.split(':').collect();
        let h: f64 = t[0].parse().unwrap();
        let m: f64 = t[1].parse().unwrap();
        let s: f64 = t[2].parse().unwrap();
        let start_sec = h * 3600.0 + m * 60.0 + s;

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
    println!("{:?}", modify);

    let f = File::open(playlist_path).expect("Could not open json playlist file.");
    let mut playlist: Playlist =
        serde_json::from_reader(f).expect("Could not read json playlist file.");

    if modify.is_some() {
        playlist.modified = Some(modify.unwrap().to_string());
    }

    println!("{:#?}", playlist);

    playlist
}
