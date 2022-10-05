use std::{fs, path::PathBuf};

use simplelog::*;

use crate::utils::{errors::ServiceError, playout_config};
use ffplayout_lib::utils::{
    generate_playlist as playlist_generator, json_reader, json_writer, JsonPlaylist,
};

pub async fn read_playlist(id: i32, date: String) -> Result<JsonPlaylist, ServiceError> {
    let (config, _) = playout_config(&id).await?;
    let mut playlist_path = PathBuf::from(&config.playlist.path);
    let d: Vec<&str> = date.split('-').collect();
    playlist_path = playlist_path
        .join(d[0])
        .join(d[1])
        .join(date.clone())
        .with_extension("json");

    match json_reader(&playlist_path) {
        Ok(p) => Ok(p),
        Err(e) => Err(ServiceError::NoContent(e.to_string())),
    }
}

pub async fn write_playlist(id: i32, json_data: JsonPlaylist) -> Result<String, ServiceError> {
    let (config, _) = playout_config(&id).await?;
    let date = json_data.date.clone();
    let mut playlist_path = PathBuf::from(&config.playlist.path);
    let d: Vec<&str> = date.split('-').collect();
    playlist_path = playlist_path
        .join(d[0])
        .join(d[1])
        .join(date.clone())
        .with_extension("json");
    let mut file_exists = false;

    if let Some(p) = playlist_path.parent() {
        fs::create_dir_all(p)?;
    }

    if playlist_path.is_file() {
        file_exists = true;
        if let Ok(existing_data) = json_reader(&playlist_path) {
            if json_data == existing_data {
                return Err(ServiceError::Conflict(format!(
                    "Playlist from {date}, already exists!"
                )));
            }
        }
    }

    match json_writer(&playlist_path, json_data) {
        Ok(_) => {
            let mut msg = format!("Write playlist from {date} success!");

            if file_exists {
                msg = format!("Update playlist from {date} success!");
            }

            return Ok(msg);
        }
        Err(e) => {
            error!("{e}");
        }
    }

    Err(ServiceError::InternalServerError)
}

pub async fn generate_playlist(id: i32, date: String) -> Result<JsonPlaylist, ServiceError> {
    let (mut config, channel) = playout_config(&id).await?;
    config.general.generate = Some(vec![date.clone()]);

    match playlist_generator(&config, Some(channel.name)) {
        Ok(playlists) => {
            if !playlists.is_empty() {
                Ok(playlists[0].clone())
            } else {
                Err(ServiceError::Conflict(
                    "Playlist could not be written, possible already exists!".into(),
                ))
            }
        }
        Err(e) => {
            error!("{e}");
            Err(ServiceError::InternalServerError)
        }
    }
}

pub async fn delete_playlist(id: i32, date: &str) -> Result<(), ServiceError> {
    let (config, _) = playout_config(&id).await?;
    let mut playlist_path = PathBuf::from(&config.playlist.path);
    let d: Vec<&str> = date.split('-').collect();
    playlist_path = playlist_path
        .join(d[0])
        .join(d[1])
        .join(date)
        .with_extension("json");

    if playlist_path.is_file() {
        if let Err(e) = fs::remove_file(playlist_path) {
            error!("{e}");
            return Err(ServiceError::InternalServerError);
        };
    }

    Ok(())
}
