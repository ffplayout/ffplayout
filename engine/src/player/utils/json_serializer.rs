use std::{
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};

use log::*;
use serde::{Deserialize, Serialize};
use tokio::{fs::File, io::AsyncReadExt, sync::Mutex};

use crate::player::utils::{
    Media, PlayoutConfig, get_date, is_remote, json_validate::validate_playlist, modified_time,
    time_from_header,
};
use crate::utils::{config::DUMMY_LEN, logging::Target};

/// This is our main playlist object, it holds all necessary information for the current day.
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
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
        let media = Media {
            begin: Some(start),
            title: None,
            duration: DUMMY_LEN,
            out: DUMMY_LEN,
            ..Media::default()
        };
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
        item.skip = false;
        item.filter = None;

        let dur = item.out - item.seek;
        start_sec += dur;
        length += dur;
    }

    playlist.length = Some(length);
}

/// Read json playlist file, fills JsonPlaylist struct and set some extra values,
/// which we need to process.
pub async fn read_json(
    config: &mut PlayoutConfig,
    current_list: Arc<Mutex<Vec<Media>>>,
    path: Option<String>,
    is_alive: Arc<AtomicBool>,
    seek: bool,
    get_next: bool,
) -> JsonPlaylist {
    let id = config.general.channel_id;
    let config_clone = config.clone();
    let mut playlist_path = config.channel.playlists.clone();
    let start_sec = config.playlist.start_sec.unwrap();
    let date = get_date(seek, start_sec, get_next, &config.channel.timezone);

    if playlist_path.is_dir() || is_remote(&config.channel.playlists.to_string_lossy()) {
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
        current_file = p;
    }

    if is_remote(&current_file) {
        if let Ok(resp) = reqwest::Client::new().get(&current_file).send().await {
            if resp.status().is_success() {
                let headers = resp.headers().clone();

                if let Ok(body) = resp.text().await {
                    let mut playlist: JsonPlaylist = match serde_json::from_str(&body) {
                        Ok(p) => p,
                        Err(e) => {
                            error!(target: Target::file_mail(), channel = id; "Could't read remote json playlist. {e:?}");
                            JsonPlaylist::new(date, start_sec)
                        }
                    };

                    playlist.path = Some(current_file);
                    playlist.start_sec = Some(start_sec);

                    if let Some(time) = time_from_header(&headers) {
                        playlist.modified = Some(time.to_string());
                    }

                    let list_clone = playlist.clone();

                    if !config.general.skip_validation {
                        tokio::spawn(validate_playlist(
                            config_clone,
                            current_list,
                            list_clone,
                            is_alive,
                        ));
                    }

                    set_defaults(&mut playlist);

                    return playlist;
                }
            }
        }
    } else if playlist_path.is_file() {
        let modified = modified_time(&current_file).await;

        let mut f = File::options()
            .read(true)
            .write(false)
            .open(&current_file)
            .await
            .expect("Open json playlist file.");
        let mut contents = String::new();
        f.read_to_string(&mut contents)
            .await
            .expect("Read playlist content.");
        let mut playlist: JsonPlaylist = match serde_json::from_str(&contents) {
            Ok(p) => p,
            Err(e) => {
                error!(target: Target::file_mail(), channel = id; "Playlist file not readable! {e}");
                JsonPlaylist::new(date.clone(), start_sec)
            }
        };

        // catch empty program list
        if playlist.program.is_empty() {
            playlist = JsonPlaylist::new(date, start_sec);
        }

        playlist.path = Some(current_file);
        playlist.start_sec = Some(start_sec);
        playlist.modified = modified;

        let list_clone = playlist.clone();

        if !config.general.skip_validation {
            tokio::spawn(validate_playlist(
                config_clone,
                current_list,
                list_clone,
                is_alive,
            ));
        }

        set_defaults(&mut playlist);

        return playlist;
    }

    error!(target: Target::file_mail(), channel = id; "Playlist <span class=\"log-addr\">{current_file}</span> not exist!");

    JsonPlaylist::new(date, start_sec)
}
