use serde::{Deserialize, Serialize};
use std::{fs::File, path::Path};

use simplelog::*;

use crate::utils::{get_date, modified_time, seek_and_length, time_to_sec, Config, Media};

#[derive(Debug, Serialize, Deserialize)]
pub struct Playlist {
    pub date: String,
    pub start_sec: Option<f64>,
    pub current_file: Option<String>,
    pub start_index: Option<usize>,
    pub modified: Option<String>,
    pub program: Vec<Media>,
}

pub fn read_json(config: &Config, seek: bool, next_start: f64) -> Playlist {
    let mut playlist_path = Path::new(&config.playlist.path).to_owned();
    let start = &config.playlist.day_start;
    let mut start_sec = time_to_sec(start);

    if playlist_path.is_dir() {
        let date = get_date(seek, start_sec, next_start);
        let d: Vec<&str> = date.split('-').collect();
        playlist_path = playlist_path
            .join(d[0])
            .join(d[1])
            .join(date)
            .with_extension("json");
    }

    let current_file: String = playlist_path.as_path().display().to_string();

    info!("Read Playlist: <b><magenta>{}</></b>", &current_file);

    let modify = modified_time(current_file.clone());
    let f = File::open(&current_file).expect("Could not open json playlist file.");
    let mut playlist: Playlist =
        serde_json::from_reader(f).expect("Could not read json playlist file.");

    playlist.current_file = Some(current_file);
    playlist.start_sec = Some(start_sec.clone());

    if modify.is_some() {
        playlist.modified = Some(modify.unwrap().to_string());
    }

    for (i, item) in playlist.program.iter_mut().enumerate() {
        item.begin = Some(start_sec);
        item.index = Some(i);
        item.last_ad = Some(false);
        item.next_ad = Some(false);
        item.process = Some(true);
        item.cmd = Some(seek_and_length(
            item.source.clone(),
            item.seek,
            item.out,
            item.duration,
        ));
        item.filter = Some(vec![]);

        start_sec += item.out - item.seek;
    }

    playlist
}
