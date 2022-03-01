use serde::{Deserialize, Serialize};
use std::{fs::File, path::Path};

use simplelog::*;

use crate::utils::{get_date, get_sec, modified_time, time_to_sec, Config, Media};

#[derive(Debug, Serialize, Deserialize)]
pub struct Playlist {
    pub date: String,
    pub current_file: Option<String>,
    pub start_index: Option<usize>,
    pub modified: Option<String>,
    pub program: Vec<Media>,
}

pub fn read_json(config: &Config, seek: bool) -> Playlist {
    let mut playlist_path = Path::new(&config.playlist.path).to_owned();
    let start = &config.playlist.day_start;
    let length = &config.playlist.length;
    let mut start_sec = time_to_sec(start);
    let mut length_sec: f64 = 86400.0;
    let mut seek_first = seek;

    if playlist_path.is_dir() {
        let date = get_date(seek, start_sec, 0.0);
        let d: Vec<&str> = date.split('-').collect();
        playlist_path = playlist_path
            .join(d[0])
            .join(d[1])
            .join(date)
            .with_extension("json");
    }

    let current_file: String = playlist_path.as_path().display().to_string();

    if length.contains(":") {
        length_sec = time_to_sec(length);
    }

    info!("Read Playlist: <b><magenta>{}</></b>", &current_file);

    let modify = modified_time(current_file.clone());
    let f = File::open(&current_file).expect("Could not open json playlist file.");
    let mut playlist: Playlist =
        serde_json::from_reader(f).expect("Could not read json playlist file.");

    playlist.current_file = Some(current_file);

    if modify.is_some() {
        playlist.modified = Some(modify.unwrap().to_string());
    }

    let mut time_sec = get_sec();

    if seek_first && time_sec < start_sec {
        time_sec += length_sec
    }

    let cloned_program = playlist.program.clone();

    for (i, item) in playlist.program.iter_mut().enumerate() {
        item.begin = Some(start_sec);
        item.index = Some(i);
        item.last_ad = Some(false);
        item.next_ad = Some(false);
        let mut source_cmd: Vec<String> = vec![];

        if i > 0 && cloned_program[i - 1].category == "advertisement".to_string() {
            item.last_ad = Some(true);
        }

        if i + 1 < cloned_program.len() {
            if cloned_program[i + 1].category == "advertisement".to_string() {
                item.next_ad = Some(true);
            }
        }

        if seek_first {
            let tmp_length = item.out - item.seek;

            if start_sec + item.out - item.seek > time_sec {
                seek_first = false;
                playlist.start_index = Some(i);

                item.seek = time_sec - start_sec;
            }
            start_sec += tmp_length;
        } else {
            start_sec += item.out - item.seek;
        }

        if item.seek > 0.0 {
            source_cmd.append(&mut vec![
                "-ss".to_string(),
                format!("{}", item.seek).to_string(),
                "-i".to_string(),
                item.source.clone(),
            ])
        } else {
            source_cmd.append(&mut vec!["-i".to_string(), item.source.clone()])
        }

        if item.duration > item.out {
            source_cmd.append(&mut vec![
                "-t".to_string(),
                format!("{}", item.out - item.seek).to_string(),
            ]);
        }

        item.cmd = Some(source_cmd);
    }

    playlist
}
