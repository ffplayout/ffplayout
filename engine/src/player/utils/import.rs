/// Import text/m3u file and create a playlist out of it
use std::{io::Error, path::Path};

use tokio::{
    fs::{File, create_dir_all},
    io::{AsyncBufReadExt, BufReader},
};

use crate::player::utils::{Media, json_reader, json_serializer::JsonPlaylist, json_writer};

pub async fn import_file(
    playlist_root: &Path,
    date: &str,
    channel_name: Option<String>,
    path: &Path,
) -> Result<String, Error> {
    let file = File::open(path).await?;
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

    if !playlist_root.is_dir() {
        return Err(Error::other(format!(
            "Playlist folder <span class=\"log-addr\">{playlist_root:?}</span> not exists!"
        )));
    }

    let d: Vec<&str> = date.split('-').collect();
    let year = d[0];
    let month = d[1];
    let playlist_path = playlist_root.join(year).join(month);
    let playlist_file = &playlist_path.join(format!("{date}.json"));

    create_dir_all(playlist_path).await?;

    let mut lines = reader.lines();
    while let Some(line) = lines.next_line().await? {
        if !line.starts_with('#') {
            let item = Media::new(0, &line, true).await;

            if item.duration > 0.0 {
                playlist.program.push(item);
            }
        }
    }

    let mut file_exists = false;

    if playlist_file.is_file() {
        file_exists = true;
        let mut existing_data = json_reader(playlist_file).await?;
        existing_data.program.append(&mut playlist.program);

        playlist.program = existing_data.program;
    };

    let msg = if file_exists {
        format!("Update playlist from {date} success!")
    } else {
        format!("Write playlist from {date} success!")
    };

    match json_writer(playlist_file, playlist).await {
        Ok(_) => Ok(msg),
        Err(e) => Err(Error::other(e)),
    }
}
