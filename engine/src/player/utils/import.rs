/// Import text/m3u file and create a playlist out of it
use std::{
    //error::Error,
    fs::{create_dir_all, File},
    io::{BufRead, BufReader, Error, ErrorKind},
    path::Path,
};

use crate::player::utils::{
    json_reader, json_serializer::JsonPlaylist, json_writer, Media, PlayoutConfig,
};

pub fn import_file(
    config: &PlayoutConfig,
    date: &str,
    channel_name: Option<String>,
    path: &Path,
) -> Result<String, Error> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut playlist = JsonPlaylist {
        channel: channel_name.unwrap_or_else(|| "Channel 1".to_string()),
        date: date.to_string(),
        path: None,
        start_sec: None,
        length: None,
        modified: None,
        program: vec![],
    };

    let playlist_root = &config.channel.playlists;
    if !playlist_root.is_dir() {
        return Err(Error::new(
            ErrorKind::Other,
            format!(
                "Playlist folder <b><magenta>{:?}</></b> not exists!",
                config.channel.playlists,
            ),
        ));
    }

    let d: Vec<&str> = date.split('-').collect();
    let year = d[0];
    let month = d[1];
    let playlist_path = playlist_root.join(year).join(month);
    let playlist_file = &playlist_path.join(format!("{date}.json"));

    create_dir_all(playlist_path)?;

    for line in reader.lines() {
        let line = line?;

        if !line.starts_with('#') {
            let item = Media::new(0, &line, true);

            if item.duration > 0.0 {
                playlist.program.push(item);
            }
        }
    }

    let mut file_exists = false;

    if playlist_file.is_file() {
        file_exists = true;
        let mut existing_data = json_reader(playlist_file)?;
        existing_data.program.append(&mut playlist.program);

        playlist.program = existing_data.program;
    };

    let msg = if file_exists {
        format!("Update playlist from {date} success!")
    } else {
        format!("Write playlist from {date} success!")
    };

    match json_writer(playlist_file, playlist) {
        Ok(_) => Ok(msg),
        Err(e) => Err(Error::new(ErrorKind::Other, e)),
    }
}
