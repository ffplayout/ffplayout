use std::path::PathBuf;

use chrono::NaiveDate;
use log::*;
use tokio::fs;

use crate::{
    file::{ensure_path_within_root, norm_abs_path},
    player::{
        controller::ChannelManager,
        utils::{JsonPlaylist, json_reader, json_writer},
    },
    utils::{config::PlayoutConfig, errors::ServiceError, generator::playlist_generator},
};

pub(crate) fn playlist_path(config: &PlayoutConfig, date: &str) -> Result<PathBuf, ServiceError> {
    let invalid_date =
        || ServiceError::BadRequest("Invalid playlist date; expected YYYY-MM-DD".into());
    let parsed = NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|_| invalid_date())?;
    let normalized = parsed.format("%Y-%m-%d").to_string();

    if date.len() != 10 || date != normalized {
        return Err(invalid_date());
    }

    let root = &config.channel.playlists;

    // A configured single-file playlist is an explicit trusted target.
    if root
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
    {
        return Ok(root.clone());
    }

    let path = root
        .join(parsed.format("%Y").to_string())
        .join(parsed.format("%m").to_string())
        .join(normalized)
        .with_extension("json");

    // Resolve existing ancestors too, so new files through directory links
    // cannot escape the root. Keep the lexical path: deletion must unlink a
    // day alias, rather than delete its shared source file. This preflight
    // check assumes links are not concurrently swapped by filesystem writers.
    ensure_path_within_root(root, &path)?;

    Ok(path)
}

pub async fn read_playlist(
    config: &PlayoutConfig,
    date: String,
) -> Result<JsonPlaylist, ServiceError> {
    let playlist_path = playlist_path(config, &date)?;

    if !fs::try_exists(&playlist_path).await? {
        return Err(ServiceError::NoContent());
    }

    match json_reader(&playlist_path).await {
        Ok(p) => Ok(p),
        Err(e) => Err(ServiceError::Conflict(e.to_string())),
    }
}

pub async fn write_playlist(
    config: &PlayoutConfig,
    json_data: JsonPlaylist,
) -> Result<String, ServiceError> {
    let date = json_data.date.clone();
    let playlist_path = playlist_path(config, &date)?;

    let mut file_exists = false;

    if let Some(p) = playlist_path.parent() {
        fs::create_dir_all(p).await?;
    }

    if playlist_path.is_file() {
        file_exists = true;
        if let Ok(existing_data) = json_reader(&playlist_path).await
            && json_data == existing_data
        {
            return Err(ServiceError::Conflict(format!(
                "Playlist from {date}, already exists!"
            )));
        }
    }

    match json_writer(&playlist_path, json_data).await {
        Ok(..) => {
            return if file_exists {
                Ok(format!("Update playlist from {date} success!"))
            } else {
                Ok(format!("Write playlist from {date} success!"))
            };
        }
        Err(e) => {
            error!("{e}");
        }
    }

    Err(ServiceError::InternalServerError)
}

pub async fn generate_playlist(
    manager: ChannelManager,
    shuffle_override: Option<bool>,
) -> Result<JsonPlaylist, ServiceError> {
    let (template, storage_root) = {
        let mut config = manager.config.write().await;
        (
            config.general.template.take(),
            config.channel.storage.clone(),
        )
    };

    if let Some(mut template) = template {
        for source in &mut template.sources {
            let mut resolved_paths = Vec::with_capacity(source.paths.len());

            for path in &source.paths {
                let (safe_path, _, _) = norm_abs_path(&storage_root, &path.to_string_lossy())?;
                resolved_paths.push(safe_path);
            }

            source.paths = resolved_paths;
        }

        {
            let mut config = manager.config.write().await;
            config.general.template = Some(template);
        }
    }

    match playlist_generator(&manager, shuffle_override).await {
        Ok(playlists) => {
            if playlists.is_empty() {
                Err(ServiceError::Conflict(
                    "The playlist could not be written, maybe it already exists!".into(),
                ))
            } else {
                Ok(playlists[0].clone())
            }
        }
        Err(e) => {
            error!("{e}");
            Err(e)
        }
    }
}

pub async fn delete_playlist(config: &PlayoutConfig, date: &str) -> Result<String, ServiceError> {
    let playlist_path = playlist_path(config, date)?;

    if playlist_path == config.channel.playlists {
        return Err(ServiceError::BadRequest(
            "A fixed playlist file cannot be deleted by date".into(),
        ));
    }

    if playlist_path.is_file() || playlist_path.is_symlink() {
        match fs::remove_file(playlist_path).await {
            Ok(..) => Ok(format!("Delete playlist from {date} success!")),
            Err(e) => {
                error!("{e}");
                Err(ServiceError::InternalServerError)
            }
        }
    } else {
        Ok(format!("No playlist to delete on: {date}"))
    }
}
