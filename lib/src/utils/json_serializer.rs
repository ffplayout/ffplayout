use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    path::Path,
    sync::{atomic::AtomicBool, Arc},
    thread,
};

use simplelog::*;

use crate::utils::{
    get_date, is_remote, modified_time, time_from_header, validate_playlist, Media, PlayoutConfig,
};

pub const DUMMY_LEN: f64 = 60.0;

/// This is our main playlist object, it holds all necessary information for the current day.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct JsonPlaylist {
    pub channel: String,
    pub date: String,

    #[serde(skip_serializing, skip_deserializing)]
    pub start_sec: Option<f64>,

    #[serde(skip_serializing, skip_deserializing)]
    pub current_file: Option<String>,

    #[serde(skip_serializing, skip_deserializing)]
    pub modified: Option<String>,

    pub program: Vec<Media>,
}

impl JsonPlaylist {
    fn new(date: String, start: f64) -> Self {
        let mut media = Media::new(0, String::new(), false);
        media.begin = Some(start);
        media.duration = DUMMY_LEN;
        media.out = DUMMY_LEN;
        Self {
            channel: "Channel 1".into(),
            date,
            start_sec: Some(start),
            current_file: None,
            modified: None,
            program: vec![media],
        }
    }
}

impl PartialEq for JsonPlaylist {
    fn eq(&self, other: &Self) -> bool {
        self.channel == other.channel && self.date == other.date && self.program == other.program
    }
}

impl Eq for JsonPlaylist {}

fn set_defaults(
    mut playlist: JsonPlaylist,
    current_file: String,
    mut start_sec: f64,
) -> JsonPlaylist {
    playlist.current_file = Some(current_file);
    playlist.start_sec = Some(start_sec);

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

    playlist
}

/// Read json playlist file, fills JsonPlaylist struct and set some extra values,
/// which we need to process.
pub fn read_json(
    config: &PlayoutConfig,
    path: Option<String>,
    is_terminated: Arc<AtomicBool>,
    seek: bool,
    next_start: f64,
) -> JsonPlaylist {
    let config_clone = config.clone();
    let mut playlist_path = Path::new(&config.playlist.path).to_owned();
    let start_sec = config.playlist.start_sec.unwrap();
    let date = get_date(seek, start_sec, next_start);

    if playlist_path.is_dir() || is_remote(&config.playlist.path) {
        let d: Vec<&str> = date.split('-').collect();
        playlist_path = playlist_path
            .join(d[0])
            .join(d[1])
            .join(date.clone())
            .with_extension("json");
    }

    let mut current_file = playlist_path.as_path().display().to_string();

    if let Some(p) = path {
        playlist_path = Path::new(&p).to_owned();
        current_file = p
    }

    if is_remote(&current_file) {
        let response = reqwest::blocking::Client::new().get(&current_file).send();

        if let Ok(resp) = response {
            if resp.status().is_success() {
                let headers = resp.headers().clone();

                if let Ok(body) = resp.text() {
                    let mut playlist: JsonPlaylist =
                        serde_json::from_str(&body).expect("Could't read remote json playlist.");

                    if let Some(time) = time_from_header(&headers) {
                        playlist.modified = Some(time.to_string());
                    }

                    let list_clone = playlist.clone();

                    thread::spawn(move || {
                        validate_playlist(list_clone, is_terminated, config_clone)
                    });

                    return set_defaults(playlist, current_file, start_sec);
                }
            }
        }
    } else if playlist_path.is_file() {
        let f = File::options()
            .read(true)
            .write(false)
            .open(&current_file)
            .expect("Could not open json playlist file.");
        let mut playlist: JsonPlaylist =
            serde_json::from_reader(f).expect("Could't read json playlist file.");
        playlist.modified = modified_time(&current_file);

        let list_clone = playlist.clone();

        thread::spawn(move || validate_playlist(list_clone, is_terminated, config_clone));

        return set_defaults(playlist, current_file, start_sec);
    }

    error!("Read playlist error, on: <b><magenta>{current_file}</></b>");

    JsonPlaylist::new(date, start_sec)
}
