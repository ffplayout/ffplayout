use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::channel,
        Arc,
    },
    time::Duration,
};

use log::*;
use notify::{
    event::{CreateKind, ModifyKind, RemoveKind, RenameMode},
    EventKind::{Create, Modify, Remove},
    RecursiveMode,
};
use notify_debouncer_full::new_debouncer;
use tokio::sync::Mutex;

use crate::player::utils::{include_file_extension, Media};
use crate::utils::{config::PlayoutConfig, logging::Target};

/// Create a watcher, which monitor file changes.
/// When a change is register, update the current file list.
/// This makes it possible, to play infinitely and and always new files to it.
pub async fn watch(
    config: PlayoutConfig,
    is_alive: Arc<AtomicBool>,
    sources: Arc<Mutex<Vec<Media>>>,
) {
    let id = config.general.channel_id;
    let path = Path::new(&config.channel.storage);

    if !path.exists() {
        error!(target: Target::file_mail(), channel = id; "Folder path not exists: '{path:?}'");
        panic!("Folder path not exists: '{path:?}'");
    }

    debug!(target: Target::file_mail(), channel = id;
        "Monitor folder: <b><magenta>{:?}</></b>",
        config.channel.storage
    );

    let (tx, rx) = channel();
    let mut debouncer = new_debouncer(Duration::from_secs(3), None, tx).unwrap();
    debouncer.watch(path, RecursiveMode::Recursive).unwrap();

    while is_alive.load(Ordering::SeqCst) {
        if let Ok(result) = rx.try_recv() {
            match result {
                Ok(events) => {
                    let events: Vec<_> = events.to_vec();
                    for event in events {
                        match event.kind {
                            Create(CreateKind::File) | Modify(ModifyKind::Name(RenameMode::To)) => {
                                let new_path = &event.paths[0];

                                if new_path.is_file() && include_file_extension(&config, new_path) {
                                    let index = sources.lock().await.len();
                                    let media =
                                        Media::new(index, &new_path.to_string_lossy(), false).await;

                                    sources.lock().await.push(media);
                                    info!(target: Target::file_mail(), channel = id; "Create new file: <b><magenta>{new_path:?}</></b>");
                                }
                            }
                            Remove(RemoveKind::File)
                            | Modify(ModifyKind::Name(RenameMode::From)) => {
                                let old_path = &event.paths[0];

                                if !old_path.is_file() && include_file_extension(&config, old_path)
                                {
                                    sources
                                        .lock()
                                        .await
                                        .retain(|x| x.source != old_path.to_string_lossy());
                                    info!(target: Target::file_mail(), channel = id; "Remove file: <b><magenta>{old_path:?}</></b>");
                                }
                            }
                            Modify(ModifyKind::Name(RenameMode::Both)) => {
                                let old_path = &event.paths[0];
                                let new_path = &event.paths[1];

                                let mut media_list = sources.lock().await;

                                if let Some(index) = media_list
                                    .iter()
                                    .position(|x| *x.source == old_path.display().to_string())
                                {
                                    let media =
                                        Media::new(index, &new_path.to_string_lossy(), false).await;
                                    media_list[index] = media;
                                    info!(target: Target::file_mail(), channel = id; "Move file: <b><magenta>{old_path:?}</></b> to <b><magenta>{new_path:?}</></b>");
                                } else if include_file_extension(&config, new_path) {
                                    let index = media_list.len();
                                    let media =
                                        Media::new(index, &new_path.to_string_lossy(), false).await;

                                    media_list.push(media);
                                    info!(target: Target::file_mail(), channel = id; "Create new file: <b><magenta>{new_path:?}</></b>");
                                }
                            }
                            _ => {
                                trace!("Not tracked file event: {event:?}");
                            }
                        }
                    }
                }
                Err(errors) => errors.iter().for_each(
                    |error| error!(target: Target::file_mail(), channel = id; "{error:?}"),
                ),
            }
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
    }
}
