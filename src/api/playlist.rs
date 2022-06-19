use std::{fs::File, path::PathBuf};

use crate::api::{errors::ServiceError, utils::playout_config};
use crate::utils::JsonPlaylist;

pub async fn read_playlist(id: i64, date: String) -> Result<JsonPlaylist, ServiceError> {
    let config = playout_config(&id).await?;
    let mut playlist_path = PathBuf::from(&config.playlist.path);
    let d: Vec<&str> = date.split('-').collect();
    playlist_path = playlist_path
        .join(d[0])
        .join(d[1])
        .join(date.clone())
        .with_extension("json");

    if let Ok(f) = File::options().read(true).write(false).open(&playlist_path) {
        if let Ok(p) = serde_json::from_reader(f) {
            return Ok(p);
        }
    };

    Err(ServiceError::InternalServerError)
}
