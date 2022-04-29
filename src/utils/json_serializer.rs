use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    path::Path,
    sync::{atomic::AtomicBool, Arc},
    thread,
};

use simplelog::*;

use crate::utils::{get_date, modified_time, validate_playlist, GlobalConfig, Media};

pub const DUMMY_LEN: f64 = 60.0;

/// This is our main playlist object, it holds all necessary information for the current day.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Playlist {
    pub date: String,

    #[serde(skip_serializing, skip_deserializing)]
    pub start_sec: Option<f64>,

    #[serde(skip_serializing, skip_deserializing)]
    pub current_file: Option<String>,

    #[serde(skip_serializing, skip_deserializing)]
    pub modified: Option<String>,

    pub program: Vec<Media>,
}

impl Playlist {
    fn new(date: String, start: f64) -> Self {
        let mut media = Media::new(0, String::new(), false);
        media.begin = Some(start);
        media.duration = DUMMY_LEN;
        media.out = DUMMY_LEN;
        Self {
            date,
            start_sec: Some(start),
            current_file: None,
            modified: Some(String::new()),
            program: vec![media],
        }
    }
}

/// Read json playlist file, fills Playlist struct and set some extra values,
/// which we need to process.
pub fn read_json(
    path: Option<String>,
    is_terminated: Arc<AtomicBool>,
    seek: bool,
    next_start: f64,
) -> Playlist {
    let config = GlobalConfig::global();

    let mut playlist_path = Path::new(&config.playlist.path).to_owned();
    let mut start_sec = config.playlist.start_sec.unwrap();
    let date = get_date(seek, start_sec, next_start);

    if playlist_path.is_dir() {
        let d: Vec<&str> = date.split('-').collect();
        playlist_path = playlist_path
            .join(d[0])
            .join(d[1])
            .join(date.clone())
            .with_extension("json");
    }

    let mut current_file: String = playlist_path.as_path().display().to_string();

    if let Some(p) = path {
        playlist_path = Path::new(&p).to_owned();
        current_file = p
    }

    if !playlist_path.is_file() {
        error!("Playlist <b><magenta>{current_file}</></b> not exists!");

        return Playlist::new(date, start_sec);
    }

    info!("Read Playlist: <b><magenta>{current_file}</></b>");

    let f = File::options()
        .read(true)
        .write(false)
        .open(&current_file)
        .expect("Could not open json playlist file.");
    let mut playlist: Playlist =
        serde_json::from_reader(f).expect("Could not read json playlist file.");

    playlist.current_file = Some(current_file.clone());
    playlist.start_sec = Some(start_sec);
    let modify = modified_time(&current_file);

    if let Some(modi) = modify {
        playlist.modified = Some(modi.to_string());
    }

    // Add extra values to every media clip
    for (i, item) in playlist.program.iter_mut().enumerate() {
        item.begin = Some(start_sec);
        item.index = Some(i);
        item.last_ad = Some(false);
        item.next_ad = Some(false);
        item.process = Some(true);
        item.filter = Some(vec![]);

        start_sec += item.out - item.seek;
    }

    let list_clone = playlist.clone();

    thread::spawn(move || validate_playlist(list_clone, is_terminated, config.clone()));

    playlist
}
