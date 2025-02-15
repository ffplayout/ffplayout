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

use crate::utils::{config::PlayoutConfig, logging::Target, s3_utils::{self, S3_MAX_KEYS}};
use crate::{
    player::utils::{include_file_extension, Media},
    utils::s3_utils::S3_DEFAULT_PRESIGNEDURL_EXP,
};

/// Create a watcher, which monitor file changes.
/// When a change is register, update the current file list.
/// This makes it possible, to play infinitely and and always new files to it.
pub async fn watchman(
    config: PlayoutConfig,
    is_alive: Arc<AtomicBool>,
    sources: Arc<Mutex<Vec<Media>>>,
) {
    let (bucket, s3_client, is_s3) = if let Some(s3_storage) = &config.channel.s3_storage {
        (
            Some(s3_storage.bucket.clone()),
            Some(s3_storage.client.clone()),
            true,
        )
    } else {
        (None, None, false)
    };

    let id = config.general.channel_id;
    let path = Path::new(&config.channel.storage);

    if is_s3 {
        let mut list_resp = s3_client
            .clone()
            .unwrap()
            .list_objects_v2()
            .bucket(bucket.clone().unwrap())
            .prefix(path.to_string_lossy())
            // .max_keys(S3_MAX_KEYS)
            .into_paginator()
            .send();

        while is_alive.load(Ordering::SeqCst) {
            while let Some(result) = list_resp.next().await {
                match result {
                    Ok(object) => {
                        let sources = Arc::clone(&sources);
                        let config = config.clone();
                        let client_clone = s3_client.clone();
                        let bucket_clone = bucket.clone();

                        tokio::spawn(async move {
                            let mut media_list = sources.lock().await;
                            if let Some(contents) = object.contents {
                                for object in contents {
                                    if let Some(key) = object.key {
                                        if include_file_extension(&config, Path::new(&key)) {
                                            let index = media_list.len();
                                            let presigned_url = s3_utils::s3_get_object(
                                                &client_clone.clone().unwrap(),
                                                &bucket_clone.clone().unwrap(),
                                                &key,
                                                S3_DEFAULT_PRESIGNEDURL_EXP as u64,
                                            )
                                            .await
                                            .unwrap();
                                            let media =
                                                Media::new(index, &presigned_url, false).await;

                                            media_list.push(media);
                                            info!(target: Target::file_mail(), channel = id; "Create new file: <b><magenta>{}</></b>", key);
                                        }
                                    }
                                }
                            }
                        });
                    }
                    Err(err) => {
                        eprintln!("{err:?}");
                    }
                }
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
        }
    } else {
        if !path.exists() {
            error!(target: Target::file_mail(), channel = id; "Folder path not exists: '{path:?}'");
            panic!("Folder path not exists: '{path:?}'");
        }

        let (tx, rx) = channel();

        let mut debouncer = new_debouncer(Duration::from_secs(3), None, tx).unwrap();

        debouncer.watch(path, RecursiveMode::Recursive).unwrap();

        while is_alive.load(Ordering::SeqCst) {
            if let Ok(result) = rx.try_recv() {
                match result {
                    Ok(events) => {
                        let sources = Arc::clone(&sources);
                        let config = config.clone();

                        tokio::spawn(async move {
                            let events: Vec<_> = events.to_vec();
                            for event in events {
                                match event.kind {
                                    Create(CreateKind::File)
                                    | Modify(ModifyKind::Name(RenameMode::To)) => {
                                        let new_path = &event.paths[0];

                                        if new_path.is_file()
                                            && include_file_extension(&config, new_path)
                                        {
                                            let index = sources.lock().await.len();
                                            let media = Media::new(
                                                index,
                                                &new_path.to_string_lossy(),
                                                false,
                                            )
                                            .await;

                                            sources.lock().await.push(media);
                                            info!(target: Target::file_mail(), channel = id; "Create new file: <b><magenta>{new_path:?}</></b>");
                                        }
                                    }
                                    Remove(RemoveKind::File)
                                    | Modify(ModifyKind::Name(RenameMode::From)) => {
                                        let old_path = &event.paths[0];

                                        if !old_path.is_file()
                                            && include_file_extension(&config, old_path)
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

                                        if let Some(index) = media_list.iter().position(|x| {
                                            *x.source == old_path.display().to_string()
                                        }) {
                                            let media = Media::new(
                                                index,
                                                &new_path.to_string_lossy(),
                                                false,
                                            )
                                            .await;
                                            media_list[index] = media;
                                            info!(target: Target::file_mail(), channel = id; "Move file: <b><magenta>{old_path:?}</></b> to <b><magenta>{new_path:?}</></b>");
                                        } else if include_file_extension(&config, new_path) {
                                            let index = media_list.len();
                                            let media = Media::new(
                                                index,
                                                &new_path.to_string_lossy(),
                                                false,
                                            )
                                            .await;

                                            media_list.push(media);
                                            info!(target: Target::file_mail(), channel = id; "Create new file: <b><magenta>{new_path:?}</></b>");
                                        }
                                    }
                                    _ => {
                                        trace!("Not tracked file event: {event:?}");
                                    }
                                }
                            }
                        });
                    }
                    Err(errors) => errors.iter().for_each(
                        |error| error!(target: Target::file_mail(), channel = id; "{error:?}"),
                    ),
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
        }
    }
}
