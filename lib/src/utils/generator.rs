/// Simple Playlist Generator
///
/// You can call ffplayout[.exe] -g YYYY-mm-dd - YYYY-mm-dd to generate JSON playlists.
///
/// The generator takes the files from storage, which are set in config.
/// It also respect the shuffle/sort mode.
///
/// Beside that it is really very basic, without any logic.
use std::{
    fs::{create_dir_all, write},
    io::Error,
    path::Path,
    process::exit,
};

use simplelog::*;

use super::{folder::FolderSource, PlayerControl};
use crate::utils::{
    get_date_range, json_serializer::JsonPlaylist, time_to_sec, Media, PlayoutConfig,
};

/// Generate playlists
pub fn generate_playlist(
    config: &PlayoutConfig,
    channel_name: Option<String>,
) -> Result<Vec<JsonPlaylist>, Error> {
    let total_length = match config.playlist.length_sec {
        Some(length) => length,
        None => {
            if config.playlist.length.contains(':') {
                time_to_sec(&config.playlist.length)
            } else {
                86400.0
            }
        }
    };
    let player_control = PlayerControl::new();
    let playlist_root = Path::new(&config.playlist.path);
    let mut playlists = vec![];
    let mut date_range = vec![];

    let channel = match channel_name {
        Some(name) => name,
        None => "Channel 1".to_string(),
    };

    if !playlist_root.is_dir() {
        error!(
            "Playlist folder <b><magenta>{}</></b> not exists!",
            &config.playlist.path
        );

        exit(1);
    }

    if let Some(range) = config.general.generate.clone() {
        date_range = range;
    }

    if date_range.contains(&"-".to_string()) && date_range.len() == 3 {
        date_range = get_date_range(&date_range)
    }

    let media_list = FolderSource::new(config, None, &player_control);
    let list_length = media_list.player_control.current_list.lock().unwrap().len();

    for date in date_range {
        let d: Vec<&str> = date.split('-').collect();
        let year = d[0];
        let month = d[1];
        let playlist_path = playlist_root.join(year).join(month);
        let playlist_file = &playlist_path.join(format!("{date}.json"));

        create_dir_all(playlist_path)?;

        if playlist_file.is_file() {
            warn!(
                "Playlist exists, skip: <b><magenta>{}</></b>",
                playlist_file.display()
            );

            continue;
        }

        info!(
            "Generate playlist: <b><magenta>{}</></b>",
            playlist_file.display()
        );

        // TODO: handle filler folder
        let mut filler = Media::new(0, &config.storage.filler, true);
        let filler_length = filler.duration;
        let mut length = 0.0;
        let mut round = 0;

        let mut playlist = JsonPlaylist {
            channel: channel.clone(),
            date,
            current_file: None,
            start_sec: None,
            modified: None,
            program: vec![],
        };

        for item in media_list.clone() {
            let duration = item.duration;

            if total_length > length + duration {
                playlist.program.push(item);

                length += duration;
            } else if filler_length > 0.0 && filler_length > total_length - length {
                filler.out = total_length - length;
                playlist.program.push(filler);

                break;
            } else if round == list_length - 1 {
                break;
            } else {
                round += 1;
            }
        }

        playlists.push(playlist.clone());

        let json: String = serde_json::to_string_pretty(&playlist)?;

        write(playlist_file, json)?;
    }

    Ok(playlists)
}
