use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::channel,
    },
    time::Duration,
};

use log::*;
use notify::{
    EventKind::{Create, Modify, Remove},
    RecursiveMode,
    event::{CreateKind, ModifyKind, RemoveKind, RenameMode},
};
use notify_debouncer_full::new_debouncer;
use tokio::sync::Mutex;

use crate::{
    player::utils::{Media, include_file_extension},
    utils::{config::PlayoutConfig, errors::ServiceError},
};

/// Create a watcher, which monitor file changes.
/// When a change is register, update the current file list.
/// This makes it possible, to play infinitely and and always new files to it.
pub async fn watch(
    config: PlayoutConfig,
    is_alive: Arc<AtomicBool>,
    sources: Arc<Mutex<Vec<Media>>>,
) -> Result<(), ServiceError> {
    let id = config.general.channel_id;
    let path = Path::new(&config.channel.storage);

    if !path.exists() {
        return Err(ServiceError::NotFound(format!(
            "Folder path does not exist: {path:?}"
        )));
    }

    debug!(channel = id;
        "Monitor folder: <span class=\"log-addr\">{:?}</span>",
        config.channel.storage
    );

    let (tx, rx) = channel();
    let mut debouncer = new_debouncer(Duration::from_secs(3), None, tx).map_err(|error| {
        ServiceError::Conflict(format!("Failed to create file watcher: {error}"))
    })?;
    debouncer
        .watch(path, RecursiveMode::Recursive)
        .map_err(|error| ServiceError::Conflict(format!("Failed to watch {path:?}: {error}")))?;

    while is_alive.load(Ordering::SeqCst) {
        while let Ok(result) = rx.try_recv() {
            match result {
                Ok(events) => {
                    for event in events {
                        match event.kind {
                            Create(CreateKind::File) | Modify(ModifyKind::Name(RenameMode::To)) => {
                                let Some(new_path) = event.paths.first() else {
                                    warn!(channel = id; "File create event has no path");
                                    continue;
                                };

                                if new_path.is_file() && include_file_extension(&config, new_path) {
                                    let index = sources.lock().await.len();
                                    let media =
                                        Media::new(index, &new_path.to_string_lossy(), false).await;

                                    sources.lock().await.push(media);
                                    info!(channel = id; "Create new file: <span class=\"log-addr\">{new_path:?}</span>");
                                }
                            }
                            Remove(RemoveKind::File)
                            | Modify(ModifyKind::Name(RenameMode::From)) => {
                                let Some(old_path) = event.paths.first() else {
                                    warn!(channel = id; "File remove event has no path");
                                    continue;
                                };

                                if !old_path.is_file() && include_file_extension(&config, old_path)
                                {
                                    sources
                                        .lock()
                                        .await
                                        .retain(|x| x.source != old_path.to_string_lossy());
                                    info!(channel = id; "Remove file: <span class=\"log-addr\">{old_path:?}</span>");
                                }
                            }
                            Modify(ModifyKind::Name(RenameMode::Both)) => {
                                let (Some(old_path), Some(new_path)) =
                                    (event.paths.first(), event.paths.get(1))
                                else {
                                    warn!(channel = id; "File rename event has fewer than two paths");
                                    continue;
                                };

                                let mut media_list = sources.lock().await;

                                if let Some(index) = media_list
                                    .iter()
                                    .position(|x| *x.source == old_path.display().to_string())
                                {
                                    let media =
                                        Media::new(index, &new_path.to_string_lossy(), false).await;
                                    media_list[index] = media;
                                    info!(channel = id; "Move file: <span class=\"log-addr\">{old_path:?}</span> to <span class=\"log-addr\">{new_path:?}</span>");
                                } else if include_file_extension(&config, new_path) {
                                    let index = media_list.len();
                                    let media =
                                        Media::new(index, &new_path.to_string_lossy(), false).await;

                                    media_list.push(media);
                                    info!(channel = id; "Create new file: <span class=\"log-addr\">{new_path:?}</span>");
                                }
                            }
                            _ => {
                                trace!("Not tracked file event: {event:?}");
                            }
                        }
                    }
                }
                Err(errors) => errors
                    .iter()
                    .for_each(|error| error!(channel = id; "{error:?}")),
            }
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn missing_watch_directory_returns_error() {
        let mut config = PlayoutConfig::default();
        config.channel.storage = std::env::temp_dir().join(format!(
            "ffplayout-missing-watch-directory-{}",
            std::process::id()
        ));

        let result = watch(
            config,
            Arc::new(AtomicBool::new(true)),
            Arc::new(Mutex::new(Vec::new())),
        )
        .await;

        assert!(matches!(result, Err(ServiceError::NotFound(_))));
    }
}
