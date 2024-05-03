use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    path::Path,
    sync::{atomic::AtomicBool, Arc},
    thread,
};

use simplelog::*;

use crate::utils::{
    get_date, is_remote, modified_time, time_from_header, validate_playlist, Media, PlayerControl,
    PlayoutConfig, DUMMY_LEN,
};

/// This is our main playlist object, it holds all necessary information for the current day.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct JsonPlaylist {
    #[serde(default = "default_channel")]
    pub channel: String,
    pub date: String,

    #[serde(skip_serializing, skip_deserializing)]
    pub start_sec: Option<f64>,

    #[serde(skip_serializing, skip_deserializing)]
    pub length: Option<f64>,

    #[serde(skip_serializing, skip_deserializing)]
    pub path: Option<String>,

    #[serde(skip_serializing, skip_deserializing)]
    pub modified: Option<String>,

    pub program: Vec<Media>,
}

impl JsonPlaylist {
    pub fn new(date: String, start: f64) -> Self {
        let mut media = Media::new(0, "", false);
        media.begin = Some(start);
        media.title = None;
        media.duration = DUMMY_LEN;
        media.out = DUMMY_LEN;
        Self {
            channel: "Channel 1".into(),
            date,
            start_sec: Some(start),
            length: Some(86400.0),
            path: None,
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

fn default_channel() -> String {
    "Channel 1".to_string()
}

pub fn set_defaults(playlist: &mut JsonPlaylist) {
    let mut start_sec = playlist.start_sec.unwrap();
    let mut length = 0.0;

    // Add extra values to every media clip
    for (i, item) in playlist.program.iter_mut().enumerate() {
        item.begin = Some(start_sec);
        item.index = Some(i);
        item.last_ad = false;
        item.next_ad = false;
        item.process = Some(true);
        item.filter = None;

        let dur = item.out - item.seek;
        start_sec += dur;
        length += dur;
    }

    playlist.length = Some(length)
}

/// Read json playlist file, fills JsonPlaylist struct and set some extra values,
/// which we need to process.
pub fn read_json(
    config: &mut PlayoutConfig,
    player_control: &PlayerControl,
    path: Option<String>,
    is_terminated: Arc<AtomicBool>,
    seek: bool,
    get_next: bool,
) -> JsonPlaylist {
    let config_clone = config.clone();
    let control_clone = player_control.clone();
    let mut playlist_path = config.playlist.path.clone();
    let start_sec = config.playlist.start_sec.unwrap();
    let date = get_date(seek, start_sec, get_next);

    if playlist_path.is_dir() || is_remote(&config.playlist.path.to_string_lossy()) {
        let d: Vec<&str> = date.split('-').collect();
        playlist_path = playlist_path
            .join(d[0])
            .join(d[1])
            .join(date.clone())
            .with_extension("json");
    }

    let mut current_file = playlist_path.as_path().display().to_string();

    if let Some(p) = path {
        Path::new(&p).clone_into(&mut playlist_path);
        current_file = p
    }

    if is_remote(&current_file) {
        let response = reqwest::blocking::Client::new().get(&current_file).send();

        if let Ok(resp) = response {
            if resp.status().is_success() {
                let headers = resp.headers().clone();

                if let Ok(body) = resp.text() {
                    let mut playlist: JsonPlaylist = match serde_json::from_str(&body) {
                        Ok(p) => p,
                        Err(e) => {
                            error!("Could't read remote json playlist. {e:?}");
                            JsonPlaylist::new(date.clone(), start_sec)
                        }
                    };

                    playlist.path = Some(current_file);
                    playlist.start_sec = Some(start_sec);

                    if let Some(time) = time_from_header(&headers) {
                        playlist.modified = Some(time.to_string());
                    }

                    let list_clone = playlist.clone();

                    if !config.general.skip_validation {
                        thread::spawn(move || {
                            validate_playlist(
                                config_clone,
                                control_clone,
                                list_clone,
                                is_terminated,
                            )
                        });
                    }

                    set_defaults(&mut playlist);

                    return playlist;
                }
            }
        }
    } else if playlist_path.is_file() {
        let modified = modified_time(&current_file);

        let f = File::options()
            .read(true)
            .write(false)
            .open(&current_file)
            .expect("Could not open json playlist file.");
        let mut playlist: JsonPlaylist = match serde_json::from_reader(f) {
            Ok(p) => p,
            Err(e) => {
                error!("Playlist file not readable! {e}");
                JsonPlaylist::new(date.clone(), start_sec)
            }
        };

        // catch empty program list
        if playlist.program.is_empty() {
            playlist = JsonPlaylist::new(date, start_sec)
        }

        playlist.path = Some(current_file);
        playlist.start_sec = Some(start_sec);
        playlist.modified = modified;

        let list_clone = playlist.clone();

        if !config.general.skip_validation {
            thread::spawn(move || {
                validate_playlist(config_clone, control_clone, list_clone, is_terminated)
            });
        }

        set_defaults(&mut playlist);

        return playlist;
    }

    error!("Playlist <b><magenta>{current_file}</></b> not exist!");

    JsonPlaylist::new(date, start_sec)
}
