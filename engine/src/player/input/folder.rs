use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::channel,
        {Arc, Mutex},
    },
    thread::sleep,
    time::Duration,
};

use log::*;
use notify::{
    event::{CreateKind, ModifyKind, RemoveKind, RenameMode},
    EventKind::{Create, Modify, Remove},
    RecursiveMode,
};
use notify_debouncer_full::new_debouncer;

use crate::player::utils::{include_file_extension, Media};
use crate::utils::{config::PlayoutConfig, logging::Target};

/// Create a watcher, which monitor file changes.
/// When a change is register, update the current file list.
/// This makes it possible, to play infinitely and and always new files to it.
pub async fn watchman(
    config: PlayoutConfig,
    is_terminated: Arc<AtomicBool>,
    sources: Arc<Mutex<Vec<Media>>>,
) {
    let id = config.general.channel_id;
    let path = Path::new(&config.channel.storage);
    if !path.exists() {
        error!(target: Target::file_mail(), channel = id; "Folder path not exists: '{path:?}'");
        panic!("Folder path not exists: '{path:?}'");
    }

    // let (tx, rx) = channel();
    let (tx, rx) = channel();

    let mut debouncer = new_debouncer(Duration::from_secs(1), None, tx).unwrap();

    debouncer.watch(path, RecursiveMode::Recursive).unwrap();

    while !is_terminated.load(Ordering::SeqCst) {
        if let Ok(result) = rx.try_recv() {
            match result {
                Ok(events) => events.iter().for_each(|event| match event.kind {
                    Create(CreateKind::File) | Modify(ModifyKind::Name(RenameMode::To)) => {
                        let new_path = &event.paths[0];

                        if new_path.is_file() && include_file_extension(&config, new_path) {
                            let index = sources.lock().unwrap().len();
                            let media = Media::new(index, &new_path.to_string_lossy(), false);

                            sources.lock().unwrap().push(media);
                            info!(target: Target::file_mail(), channel = id; "Create new file: <b><magenta>{new_path:?}</></b>");
                        }
                    }
                    Remove(RemoveKind::File) | Modify(ModifyKind::Name(RenameMode::From)) => {
                        let old_path = &event.paths[0];

                        if !old_path.is_file() && include_file_extension(&config, old_path) {
                            sources
                                .lock()
                                .unwrap()
                                .retain(|x| x.source != old_path.to_string_lossy());
                            info!(target: Target::file_mail(), channel = id; "Remove file: <b><magenta>{old_path:?}</></b>");
                        }
                    }
                    Modify(ModifyKind::Name(RenameMode::Both)) => {
                        let old_path = &event.paths[0];
                        let new_path = &event.paths[1];

                        let mut media_list = sources.lock().unwrap();

                        if let Some(index) = media_list
                        .iter()
                        .position(|x| *x.source == old_path.display().to_string()) {
                            let media = Media::new(index, &new_path.to_string_lossy(), false);
                            media_list[index] = media;
                            info!(target: Target::file_mail(), channel = id; "Move file: <b><magenta>{old_path:?}</></b> to <b><magenta>{new_path:?}</></b>");
                        } else if include_file_extension(&config, new_path) {
                            let index = media_list.len();
                            let media = Media::new(index, &new_path.to_string_lossy(), false);

                            media_list.push(media);
                            info!(target: Target::file_mail(), channel = id; "Create new file: <b><magenta>{new_path:?}</></b>");
                        }
                    }
                    _ => trace!(target: Target::file_mail(), channel = id; "Not tracked file event: {event:?}")
                }),
                Err(errors) => errors.iter().for_each(|error| error!(target: Target::file_mail(), channel = id; "{error:?}")),
            }
        }

        sleep(Duration::from_secs(3));
    }
}
