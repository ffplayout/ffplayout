use serde::{Deserialize, Serialize};
use std::{fs::File, path::Path};

use simplelog::*;

use crate::utils::{get_date, modified_time, time_to_sec, Config, Media};

pub const DUMMY_LEN: f64 = 20.0;

#[derive(Debug, Serialize, Deserialize)]
pub struct Playlist {
    pub date: String,
    pub start_sec: Option<f64>,
    pub current_file: Option<String>,
    pub modified: Option<String>,
    pub program: Vec<Media>,
}

impl Playlist {
    fn new(date: String, start: f64) -> Self {
        let mut media = Media::new(0, "".to_string());
        media.begin = Some(start);
        media.duration = DUMMY_LEN;
        media.out = DUMMY_LEN;
        Self {
            date,
            start_sec: Some(start),
            current_file: None,
            modified: None,
            program: vec![media],
        }
    }
}

pub fn read_json(config: &Config, seek: bool, next_start: f64) -> Playlist {
    let mut playlist_path = Path::new(&config.playlist.path).to_owned();
    let start = &config.playlist.day_start;
    let mut start_sec = time_to_sec(start);
    let date = get_date(seek, start_sec, next_start);

    if playlist_path.is_dir() {
        let d: Vec<&str> = date.split('-').collect();
        playlist_path = playlist_path
            .join(d[0])
            .join(d[1])
            .join(date.clone())
            .with_extension("json");
    }

    let current_file: String = playlist_path.as_path().display().to_string();

    if !playlist_path.is_file() {
        error!("Playlist <b><magenta>{}</></b> not exists!", current_file);
        // let dummy_playlist = Playlist::new(date, get_sec());
        return Playlist::new(date, start_sec);
    }

    info!("Read Playlist: <b><magenta>{}</></b>", &current_file);

    let f = File::open(&current_file).expect("Could not open json playlist file.");
    let mut playlist: Playlist =
        serde_json::from_reader(f).expect("Could not read json playlist file.");

    playlist.current_file = Some(current_file.clone());
    playlist.start_sec = Some(start_sec.clone());
    let modify = modified_time(current_file);

    if modify.is_some() {
        playlist.modified = Some(modify.unwrap().to_string());
    }

    for (i, item) in playlist.program.iter_mut().enumerate() {
        item.begin = Some(start_sec);
        item.index = Some(i);
        item.last_ad = Some(false);
        item.next_ad = Some(false);
        item.process = Some(true);
        item.filter = Some(vec![]);

        start_sec += item.out - item.seek;
    }

    playlist
}
