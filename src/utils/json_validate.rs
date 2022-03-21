use std::{path::Path, sync::{Arc, Mutex},};

use simplelog::*;

use crate::utils::{sec_to_time, GlobalConfig, MediaProbe, Playlist};

pub async fn validate_playlist(playlist: Playlist, is_terminated: Arc<Mutex<bool>>, config: GlobalConfig) {
    let date = playlist.date;
    let length = config.playlist.length_sec.unwrap();
    let mut start_sec = 0.0;

    debug!("validate playlist from: <yellow>{date}</>");

    for item in playlist.program.iter() {
        if *is_terminated.lock().unwrap() {
            break
        }

        if Path::new(&item.source).is_file() {
            let probe = MediaProbe::new(item.source.clone());

            if probe.format.is_none() {
                error!(
                    "No Metadata from file <b><magenta>{}</></b> at <yellow>{}</>",
                    sec_to_time(start_sec),
                    item.source
                );
            }
        } else {
            error!(
                "File on position <yellow>{}</> not exists: <b><magenta>{}</></b>",
                sec_to_time(start_sec),
                item.source
            );
        }

        start_sec += item.out - item.seek;
    }

    if length > start_sec + 1.0 && !*is_terminated.lock().unwrap() {
        error!(
            "Playlist from <yellow>{date}</> not long enough, <yellow>{}</> needed!",
            sec_to_time(length - start_sec),
        );
    }
}
