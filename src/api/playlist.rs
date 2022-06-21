use std::{
    fs::{self, File},
    io::Error,
    path::PathBuf,
};

use simplelog::*;

use crate::api::{errors::ServiceError, utils::playout_config};
use crate::utils::{generate_playlist as playlist_generator, JsonPlaylist};

fn json_reader(path: &PathBuf) -> Result<JsonPlaylist, Error> {
    let f = File::options().read(true).write(false).open(&path)?;
    let p = serde_json::from_reader(f)?;

    Ok(p)
}

fn json_writer(path: &PathBuf, data: JsonPlaylist) -> Result<(), Error> {
    let f = File::options()
        .write(true)
        .truncate(true)
        .create(true)
        .open(&path)?;
    serde_json::to_writer_pretty(f, &data)?;

    Ok(())
}

pub async fn read_playlist(id: i64, date: String) -> Result<JsonPlaylist, ServiceError> {
    let (config, _) = playout_config(&id).await?;
    let mut playlist_path = PathBuf::from(&config.playlist.path);
    let d: Vec<&str> = date.split('-').collect();
    playlist_path = playlist_path
        .join(d[0])
        .join(d[1])
        .join(date.clone())
        .with_extension("json");

    if let Ok(p) = json_reader(&playlist_path) {
        return Ok(p);
    };

    Err(ServiceError::InternalServerError)
}

pub async fn write_playlist(id: i64, json_data: JsonPlaylist) -> Result<String, ServiceError> {
    let (config, _) = playout_config(&id).await?;
    let date = json_data.date.clone();
    let mut playlist_path = PathBuf::from(&config.playlist.path);
    let d: Vec<&str> = date.split('-').collect();
    playlist_path = playlist_path
        .join(d[0])
        .join(d[1])
        .join(date.clone())
        .with_extension("json");

    if playlist_path.is_file() {
        if let Ok(existing_data) = json_reader(&playlist_path) {
            if json_data == existing_data {
                return Err(ServiceError::Conflict(format!(
                    "Playlist from {date}, already exists!"
                )));
            }
        }
    }

    match json_writer(&playlist_path, json_data) {
        Ok(_) => return Ok(format!("Write playlist from {date} success!")),
        Err(e) => {
            error!("{e}");
        }
    }

    Err(ServiceError::InternalServerError)
}

pub async fn generate_playlist(id: i64, date: String) -> Result<JsonPlaylist, ServiceError> {
    let (config, settings) = playout_config(&id).await?;

    match playlist_generator(&config, vec![date], Some(settings.channel_name)) {
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

pub async fn delete_playlist(id: i64, date: &str) -> Result<(), ServiceError> {
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
