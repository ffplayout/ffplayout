use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    path::Path,
    sync::{Arc, Mutex},
};

use simplelog::*;
use tokio::runtime::Handle;

use crate::utils::{get_date, modified_time, validate_playlist, GlobalConfig, Media};

pub const DUMMY_LEN: f64 = 20.0;

#[derive(Debug, Serialize, Deserialize, Clone)]
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
            modified: Some("".to_string()),
            program: vec![media],
        }
    }
}

pub fn read_json(
    path: Option<String>,
    rt_handle: Handle,
    is_terminated: Arc<Mutex<bool>>,
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
        current_file = p
    }

    if !playlist_path.is_file() {
        error!("Playlist <b><magenta>{}</></b> not exists!", current_file);

        return Playlist::new(date, start_sec);
    }

    info!("Read Playlist: <b><magenta>{}</></b>", &current_file);

    let f = File::open(&current_file).expect("Could not open json playlist file.");
    let mut playlist: Playlist =
        serde_json::from_reader(f).expect("Could not read json playlist file.");

    playlist.current_file = Some(current_file.clone());
    playlist.start_sec = Some(start_sec.clone());
    let modify = modified_time(&current_file);

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

    rt_handle.spawn(validate_playlist(
        playlist.clone(),
        is_terminated,
        config.clone(),
    ));

    playlist
}
