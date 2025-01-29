use std::{
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
};

use actix_multipart::Multipart;
use futures_util::TryStreamExt as _;
use lexical_sort::{natural_lexical_cmp, PathSort};
use log::*;
use rand::{distr::Alphanumeric, Rng};
use tokio::{fs, io::AsyncWriteExt, sync::Mutex, task::JoinHandle};

use crate::file::{norm_abs_path, watcher::watch, MoveObject, PathObject, Storage, VideoFile};
use crate::player::utils::{file_extension, probe::MediaProbe, Media};
use crate::utils::{config::PlayoutConfig, errors::ServiceError};

#[derive(Clone, Debug)]
pub struct LocalStorage {
    pub root: PathBuf,
    pub extensions: Vec<String>,
    pub watch_handler: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl LocalStorage {
    pub fn new(root: PathBuf, extensions: Vec<String>) -> Self {
        Self {
            root,
            extensions,
            watch_handler: Arc::new(Mutex::new(None)),
        }
    }
}

impl Drop for LocalStorage {
    fn drop(&mut self) {
        let watch_handler = self.watch_handler.clone();

        tokio::spawn(async move {
            let mut watch_handler = watch_handler.lock().await;

            if let Some(handler) = watch_handler.as_mut() {
                handler.abort();
            }
        });
    }
}

impl Storage for LocalStorage {
    async fn browser(&self, path_obj: &PathObject) -> Result<PathObject, ServiceError> {
        let (path, parent, path_component) = norm_abs_path(&self.root, &path_obj.source)?;
        let mut parent_folders = vec![];

        let parent_path = if path_component.is_empty() {
            self.root.clone()
        } else {
            path.parent().unwrap().to_path_buf()
        };

        let mut obj = PathObject::new(path_component, Some(parent));
        obj.folders_only = path_obj.folders_only;

        if path != parent_path && !path_obj.folders_only {
            let mut parents = fs::read_dir(&parent_path).await?;

            while let Some(child) = parents.next_entry().await? {
                if child.metadata().await?.is_dir() {
                    parent_folders.push(
                        child
                            .path()
                            .file_name()
                            .unwrap()
                            .to_string_lossy()
                            .to_string(),
                    );
                }
            }

            parent_folders.path_sort(natural_lexical_cmp);

            obj.parent_folders = Some(parent_folders);
        }

        let mut paths_obj = fs::read_dir(path).await?;

        let mut files = vec![];
        let mut folders = vec![];

        while let Some(child) = paths_obj.next_entry().await? {
            let f_meta = child.metadata().await?;

            // ignore hidden files/folders on unix
            if child.path().to_string_lossy().to_string().contains("/.") {
                continue;
            }

            if f_meta.is_dir() {
                folders.push(
                    child
                        .path()
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .to_string(),
                );
            } else if f_meta.is_file() && !path_obj.folders_only {
                if let Some(ext) = file_extension(&child.path()) {
                    if self.extensions.contains(&ext.to_string().to_lowercase()) {
                        files.push(child.path());
                    }
                }
            }
        }

        folders.path_sort(natural_lexical_cmp);
        files.path_sort(natural_lexical_cmp);
        let mut media_files = vec![];

        for file in files {
            match MediaProbe::new(file.to_string_lossy().as_ref()).await {
                Ok(probe) => {
                    let duration = probe.format.duration.unwrap_or_default();

                    let video = VideoFile {
                        name: file.file_name().unwrap().to_string_lossy().to_string(),
                        duration,
                    };
                    media_files.push(video);
                }
                Err(e) => error!("{e:?}"),
            };
        }

        obj.folders = Some(folders);
        obj.files = Some(media_files);

        Ok(obj)
    }

    async fn mkdir(&self, path_obj: &PathObject) -> Result<(), ServiceError> {
        let (path, _, _) = norm_abs_path(&self.root, &path_obj.source)?;

        if let Err(e) = fs::create_dir_all(&path).await {
            return Err(ServiceError::BadRequest(e.to_string()));
        }

        info!(
            "create folder: <b><magenta>{}</></b>",
            path.to_string_lossy()
        );

        Ok(())
    }

    async fn rename(&self, move_object: &MoveObject) -> Result<MoveObject, ServiceError> {
        let (source_path, _, _) = norm_abs_path(&self.root, &move_object.source)?;
        let (mut target_path, _, _) = norm_abs_path(&self.root, &move_object.target)?;

        if !source_path.exists() {
            return Err(ServiceError::BadRequest("Source file not exist!".into()));
        }

        if (source_path.is_dir() || source_path.is_file())
            && source_path.parent() == Some(&target_path)
        {
            return rename_only(&source_path, &target_path).await;
        }

        if target_path.is_dir() {
            target_path = target_path.join(source_path.file_name().unwrap());
        }

        if target_path.is_file() {
            return Err(ServiceError::BadRequest(
                "Target file already exists!".into(),
            ));
        }

        if source_path.is_file() && target_path.parent().is_some() {
            return rename_only(&source_path, &target_path).await;
        }

        Err(ServiceError::InternalServerError)
    }

    async fn remove(&self, source_path: &str, recursive: bool) -> Result<(), ServiceError> {
        let (source, _, _) = norm_abs_path(&self.root, source_path)?;

        if !source.exists() {
            return Err(ServiceError::BadRequest("Source does not exists!".into()));
        }

        if source.is_dir() {
            let res = if recursive {
                fs::remove_dir_all(source).await
            } else {
                fs::remove_dir(source).await
            };

            match res {
                Ok(..) => return Ok(()),
                Err(e) => {
                    error!("{e}");
                    return Err(ServiceError::BadRequest(
                        "Delete folder failed! (Folder must be empty)".into(),
                    ));
                }
            }
        }

        if source.is_file() {
            match fs::remove_file(source).await {
                Ok(_) => return Ok(()),
                Err(e) => {
                    error!("{e}");
                    return Err(ServiceError::BadRequest("Delete file failed!".into()));
                }
            };
        }

        Err(ServiceError::InternalServerError)
    }

    async fn upload(
        &self,
        mut data: Multipart,
        path: &Path,
        is_abs: bool,
    ) -> Result<(), ServiceError> {
        while let Some(mut field) = data.try_next().await? {
            let content_disposition = field.content_disposition().ok_or("No content")?;
            debug!("{content_disposition}");
            let rand_string: String = rand::rng()
                .sample_iter(&Alphanumeric)
                .take(20)
                .map(char::from)
                .collect();
            let filename = content_disposition
                .get_filename()
                .map_or_else(|| rand_string.to_string(), sanitize_filename::sanitize);

            let filepath = if is_abs {
                path.to_path_buf()
            } else {
                let (target_path, _, _) = norm_abs_path(&self.root, &path.to_string_lossy())?;

                target_path.join(filename)
            };

            // INFO: File exist check should be enough because file size and content length are different.
            // The error catching in the loop should normally prevent unfinished files from existing on disk.
            // If this is not enough, a second check can be implemented: is_close(file_size as i64, size as i64, 1000)
            if filepath.is_file() {
                return Err(ServiceError::Conflict("Target already exists!".into()));
            }

            let mut f = fs::File::create(&filepath).await?;

            loop {
                match field.try_next().await {
                    Ok(Some(chunk)) => {
                        f = f.write_all(&chunk).await.map(|_| f)?;
                    }

                    Ok(None) => break,

                    Err(e) => {
                        if e.to_string().contains("stream is incomplete") {
                            info!("Delete non finished file: {filepath:?}");

                            tokio::fs::remove_file(filepath).await?;
                        }

                        return Err(e.into());
                    }
                }
            }
        }
        Ok(())
    }

    async fn watchman(
        &mut self,
        config: PlayoutConfig,
        is_alive: Arc<AtomicBool>,
        sources: Arc<Mutex<Vec<Media>>>,
    ) {
        self.watch_handler = Arc::new(Mutex::new(Some(tokio::spawn(async {
            watch(config, is_alive, sources).await;
        }))));
    }

    async fn stop_watch(&mut self) {
        let mut watch_handler = self.watch_handler.lock().await;

        if let Some(handler) = watch_handler.as_mut() {
            handler.abort();
        }
    }
}

async fn rename_only(source: &PathBuf, target: &PathBuf) -> Result<MoveObject, ServiceError> {
    match fs::rename(source, target).await {
        Ok(_) => Ok(MoveObject {
            source: source
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            target: target
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
        }),
        Err(e) => {
            error!("{e}");
            copy_and_delete(source, target).await
        }
    }
}

async fn copy_and_delete(source: &PathBuf, target: &PathBuf) -> Result<MoveObject, ServiceError> {
    match fs::copy(&source, &target).await {
        Ok(_) => {
            if let Err(e) = fs::remove_file(source).await {
                error!("{e}");
                return Err(ServiceError::BadRequest(
                    "Removing File not possible!".into(),
                ));
            };

            Ok(MoveObject {
                source: source
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
                target: target
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
            })
        }
        Err(e) => {
            error!("{e}");
            Err(ServiceError::BadRequest("Error in file copy!".into()))
        }
    }
}
