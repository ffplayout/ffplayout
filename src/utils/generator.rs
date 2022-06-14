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
    path::Path,
    process::exit,
    sync::{atomic::AtomicUsize, Arc, Mutex},
};

use chrono::{Duration, NaiveDate};
use simplelog::*;

use crate::input::FolderSource;
use crate::utils::{json_serializer::JsonPlaylist, Media, PlayoutConfig};

/// Generate a vector with dates, from given range.
fn get_date_range(date_range: &[String]) -> Vec<String> {
    let mut range = vec![];

    let start = match NaiveDate::parse_from_str(&date_range[0], "%Y-%m-%d") {
        Ok(s) => s,
        Err(_) => {
            error!("date format error in: <yellow>{:?}</>", date_range[0]);
            exit(1);
        }
    };

    let end = match NaiveDate::parse_from_str(&date_range[2], "%Y-%m-%d") {
        Ok(e) => e,
        Err(_) => {
            error!("date format error in: <yellow>{:?}</>", date_range[2]);
            exit(1);
        }
    };

    let duration = end.signed_duration_since(start);
    let days = duration.num_days() + 1;

    for day in 0..days {
        range.push((start + Duration::days(day)).format("%Y-%m-%d").to_string());
    }

    range
}

/// Generate playlists
pub fn generate_playlist(config: &PlayoutConfig, mut date_range: Vec<String>) {
    let total_length = config.playlist.length_sec.unwrap();
    let current_list = Arc::new(Mutex::new(vec![Media::new(0, "".to_string(), false)]));
    let index = Arc::new(AtomicUsize::new(0));
    let playlist_root = Path::new(&config.playlist.path);

    if !playlist_root.is_dir() {
        error!(
            "Playlist folder <b><magenta>{}</></b> not exists!",
            &config.playlist.path
        );

        exit(1);
    }

    if date_range.contains(&"-".to_string()) && date_range.len() == 3 {
        date_range = get_date_range(&date_range)
    }

    let media_list = FolderSource::new(config, current_list, index);
    let list_length = media_list.nodes.lock().unwrap().len();

    for date in date_range {
        let d: Vec<&str> = date.split('-').collect();
        let year = d[0];
        let month = d[1];
        let playlist_path = playlist_root.join(year).join(month);
        let playlist_file = &playlist_path.join(format!("{date}.json"));

        if let Err(e) = create_dir_all(playlist_path) {
            error!("Create folder failed: {e:?}");
            exit(1);
        }

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

        let mut filler = Media::new(0, config.storage.filler_clip.clone(), true);
        let filler_length = filler.duration;
        let mut length = 0.0;
        let mut round = 0;

        let mut playlist = JsonPlaylist {
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
                filler.out = filler_length - (total_length - length);
                playlist.program.push(filler);

                break;
            } else if round == list_length - 1 {
                break;
            } else {
                round += 1;
            }
        }

        let json: String = match serde_json::to_string_pretty(&playlist) {
            Ok(j) => j,
            Err(e) => {
                error!("Unable to serialize data: {e:?}");
                exit(0);
            }
        };

        if let Err(e) = write(playlist_file, &json) {
            error!("Unable to write playlist: {e:?}");
            exit(1)
        };
    }
}
