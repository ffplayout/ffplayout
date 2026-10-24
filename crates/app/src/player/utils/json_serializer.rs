use std::{
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};

use log::*;
use serde::{Deserialize, Serialize};
use tokio::{fs::File, io::AsyncReadExt, sync::Mutex};

use crate::{
    player::{
        controller::ChannelManager,
        utils::{
            Media, PlayoutConfig, is_remote, modified_time, normalize_live_source_timing,
            time_from_header,
        },
    },
    utils::config::DUMMY_LEN,
};

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

pub fn set_defaults(config: &PlayoutConfig, playlist: &mut JsonPlaylist) {
    let mut start_sec = playlist.start_sec.unwrap();
    let mut length = 0.0;

    // Add extra values to every media clip
    for (i, item) in playlist.program.iter_mut().enumerate() {
        normalize_live_source_timing(item);
        item.begin = Some(start_sec);
        item.index = Some(i);
        item.last_ad = false;
        item.next_ad = false;
        item.skip = false;

        for path in [&mut item.source, &mut item.audio] {
            if Path::new(path).is_relative() && !path.is_empty() && !is_remote(path) {
                let new_path = config.storage.path.join(&*path);
                *path = new_path.to_string_lossy().to_string();
            }
        }

        let dur = item.out - item.seek;
        start_sec += dur;
        length += dur;
    }

    playlist.length = Some(length);
}

/// Read json playlist file, fills JsonPlaylist struct and set some extra values,
/// which we need to process.
pub async fn read_json(
    manager: &ChannelManager,
    config: &mut PlayoutConfig,
    current_list: Arc<Mutex<Vec<Media>>>,
    path: Option<String>,
    is_alive: Arc<AtomicBool>,
    date: String,
) -> JsonPlaylist {
    let id = config.general.channel_id;
    let config_clone = config.clone();
    let mut playlist_path = config.channel.playlists.clone();
    let start_sec = config.playlist.start_sec.unwrap();

    let uses_daily_playlists =
        playlist_path.is_dir() || is_remote(&config.channel.playlists.to_string_lossy());

    if uses_daily_playlists {
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
        if let Ok(resp) = reqwest::Client::new().get(&current_file).send().await
            && resp.status().is_success()
        {
            let headers = resp.headers().clone();

            if let Ok(body) = resp.text().await {
                let mut playlist: JsonPlaylist = match serde_json::from_str(&body) {
                    Ok(p) => p,
                    Err(e) => {
                        error!(channel = id; "Could't read remote json playlist. {e:?}");
                        JsonPlaylist::new(date, start_sec)
                    }
                };

                playlist.path = Some(current_file);
                playlist.start_sec = Some(start_sec);

                if let Some(time) = time_from_header(&headers) {
                    playlist.modified = Some(time.to_string());
                }

                set_defaults(config, &mut playlist);
                let list_clone = playlist.clone();

                if !config.general.skip_validation {
                    manager
                        .spawn_validation(config_clone, current_list, list_clone, is_alive)
                        .await;
                }

                return playlist;
            }
        }
    } else if playlist_path.is_file() {
        let modified = modified_time(&current_file).await;

        let mut f = match File::options()
            .read(true)
            .write(false)
            .open(&current_file)
            .await
        {
            Ok(file) => file,
            Err(error) => {
                error!(channel = id; "Could not open playlist {current_file}: {error}");
                return JsonPlaylist::new(date, start_sec);
            }
        };
        let mut contents = String::new();
        if let Err(error) = f.read_to_string(&mut contents).await {
            error!(channel = id; "Could not read playlist {current_file}: {error}");
            return JsonPlaylist::new(date, start_sec);
        }
        let mut playlist: JsonPlaylist = match serde_json::from_str(&contents) {
            Ok(p) => p,
            Err(e) => {
                error!(channel = id; "Playlist file not readable! {e}");
                JsonPlaylist::new(date.clone(), start_sec)
            }
        };

        // An explicitly selected file is reusable on any broadcast day. Its
        // embedded date must not repeatedly trigger the next-day reload path.
        if !uses_daily_playlists {
            playlist.date.clone_from(&date);
        }

        // catch empty program list
        if playlist.program.is_empty() {
            error!("Playlist from <span class=\"log-number\">{date}</span> is empty!");
            playlist = JsonPlaylist::new(date, start_sec);
        }

        playlist.path = Some(current_file);
        playlist.start_sec = Some(start_sec);
        playlist.modified = modified;

        set_defaults(config, &mut playlist);
        let list_clone = playlist.clone();

        if !config.general.skip_validation {
            manager
                .spawn_validation(config_clone, current_list, list_clone, is_alive)
                .await;
        }

        return playlist;
    }

    error!(channel = id; "Playlist <span class=\"log-addr\">{current_file}</span> not exist!");

    JsonPlaylist::new(date, start_sec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn explicit_file_uses_broadcast_date_but_daily_directory_keeps_file_date() {
        let (manager, pool) = crate::test_support::channel_manager().await;
        let mut config = manager.config.read().await.clone();
        let root = config.storage.path.clone();
        config.general.skip_validation = true;
        config.playlist.start_sec = Some(0.0);
        let file = root.join("old-playlist.json");
        let contents = r#"{"date":"2026-10-20","program":[{"source":"clip.mp4","in":2.0,"out":12.0,"duration":30.0}]}"#;
        tokio::fs::write(&file, contents).await.unwrap();
        config.channel.playlists = file.clone();

        for date in ["2026-10-21", "2026-10-22"] {
            let playlist = read_json(
                &manager,
                &mut config,
                manager.current_list.clone(),
                None,
                manager.is_alive.clone(),
                date.to_string(),
            )
            .await;
            assert_eq!(playlist.date, date);
            assert_eq!(playlist.path.as_deref(), file.to_str());
            assert_eq!(playlist.program.len(), 1);
            assert_eq!(
                (playlist.program[0].seek, playlist.program[0].out),
                (2.0, 12.0)
            );
        }

        let daily_root = root.join("daily");
        let daily_file = daily_root.join("2026/10/2026-10-21.json");
        tokio::fs::create_dir_all(daily_file.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&daily_file, contents).await.unwrap();
        config.channel.playlists = daily_root;
        let playlist = read_json(
            &manager,
            &mut config,
            manager.current_list.clone(),
            None,
            manager.is_alive.clone(),
            "2026-10-21".to_string(),
        )
        .await;
        assert_eq!(playlist.date, "2026-10-20");
        assert_eq!(playlist.path.as_deref(), daily_file.to_str());
        pool.close().await;
        tokio::fs::remove_dir_all(root).await.unwrap();
    }
}
