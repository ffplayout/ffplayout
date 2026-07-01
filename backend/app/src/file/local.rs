use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};

#[cfg(target_family = "unix")]
use std::os::unix::fs::MetadataExt;

use async_walkdir::WalkDir;
use axum::extract::Multipart;
use lexical_sort::{PathSort, natural_lexical_cmp};
use log::*;
use rand::seq::SliceRandom;
use tokio::{
    fs,
    io::{AsyncSeekExt, AsyncWriteExt, SeekFrom},
    sync::{Mutex, RwLock},
    task::JoinHandle,
};
use tokio_stream::StreamExt;

use crate::file::{
    MoveObject, PathObject, VideoFile, norm_abs_path,
    upload::{
        MAX_CHUNK_SIZE, MAX_UPLOAD_SIZE, Meta, UPLOADS, file_ranges, is_upload_complete,
        merge_ranges, validate_mime_type,
    },
    watcher::watch,
};
use crate::player::utils::{Media, file_extension, include_file_extension, probe::MediaProbe};
use crate::utils::{config::PlayoutConfig, errors::ServiceError, logging::Target};

#[derive(Clone, Debug)]
pub struct LocalStorage {
    pub root: Arc<RwLock<PathBuf>>,
    pub extensions: Arc<RwLock<Vec<String>>>,
    pub watch_handler: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl LocalStorage {
    pub async fn new(root: PathBuf, extensions: Vec<String>) -> Self {
        if !root.is_dir() {
            fs::create_dir_all(&root)
                .await
                .unwrap_or_else(|_| panic!("Can't create storage folder: {root:?}"));
        }

        Self {
            root: Arc::new(RwLock::new(root)),
            extensions: Arc::new(RwLock::new(extensions)),
            watch_handler: Arc::new(Mutex::new(None)),
        }
    }
}

impl LocalStorage {
    pub async fn browser(&self, path_obj: &PathObject) -> Result<PathObject, ServiceError> {
        let (path, parent, path_component) =
            norm_abs_path(&self.root.read().await, &path_obj.source)?;
        let mut parent_folders = vec![];

        let parent_path = if path_component.is_empty() {
            self.root.read().await.clone()
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
            } else if f_meta.is_file()
                && !path_obj.folders_only
                && let Some(ext) = file_extension(&child.path())
                && self
                    .extensions
                    .read()
                    .await
                    .contains(&ext.to_string().to_lowercase())
            {
                files.push(child.path());
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

    pub async fn mkdir(&self, path_obj: &PathObject) -> Result<(), ServiceError> {
        let (path, _, _) = norm_abs_path(&self.root.read().await, &path_obj.source)?;

        if let Err(e) = fs::create_dir_all(&path).await {
            return Err(ServiceError::BadRequest(e.to_string()));
        }

        info!(
            "create folder: <span class=\"log-addr\">{}</span>",
            path.to_string_lossy()
        );

        Ok(())
    }

    pub async fn rename(&self, move_object: &MoveObject) -> Result<MoveObject, ServiceError> {
        let root = self.root.read().await.clone();
        let (source_path, _, _) = norm_abs_path(&root, &move_object.source)?;
        let (mut target_path, _, _) = norm_abs_path(&root, &move_object.target)?;

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

    pub async fn remove(&self, source_path: &str, recursive: bool) -> Result<(), ServiceError> {
        let (source, _, _) = norm_abs_path(&self.root.read().await, source_path)?;

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

    pub async fn upload(
        &self,
        mut data: Multipart,
        path: &Path,
        is_abs: bool,
    ) -> Result<(), ServiceError> {
        let mut file_name: Option<String> = None;
        let mut start: Option<u64> = None;
        let mut end: Option<u64> = None;
        let mut size: u64 = 0;
        let mut chunk_data: Option<Vec<u8>> = None;
        let mut batch_id = String::new();

        while let Some(field) = data.next_field().await? {
            match field.name().unwrap_or_default() {
                "fileName" => file_name = Some(sanitize_filename::sanitize(&field.text().await?)),
                "start" => start = Some(field.text().await?.parse::<u64>().unwrap_or(0)),
                "end" => end = Some(field.text().await?.parse::<u64>().unwrap_or(0)),
                "size" => size = field.text().await?.parse::<u64>().unwrap_or(0),
                "chunk" => chunk_data = Some(field.bytes().await?.to_vec()),
                "batch_id" => batch_id = field.text().await?,
                _ => {}
            }
        }

        let file_name =
            file_name.ok_or_else(|| ServiceError::BadRequest("Missing filename".into()))?;

        validate_mime_type(&file_name)?;

        let start = start.ok_or_else(|| ServiceError::BadRequest("Missing start offset".into()))?;
        let end = end.ok_or_else(|| ServiceError::BadRequest("Missing end offset".into()))?;
        let chunk_data =
            chunk_data.ok_or_else(|| ServiceError::BadRequest("Missing chunk".into()))?;

        if size > MAX_UPLOAD_SIZE {
            return Err(ServiceError::BadRequest(format!(
                "File size exceeds maximum allowed size of {MAX_UPLOAD_SIZE} bytes"
            )));
        }

        if chunk_data.len() as u64 > MAX_CHUNK_SIZE {
            return Err(ServiceError::BadRequest(format!(
                "Chunk size exceeds maximum allowed chunk size of {MAX_CHUNK_SIZE} bytes"
            )));
        }

        if end <= start || chunk_data.len() as u64 != end - start || end > size {
            return Err(ServiceError::BadRequest("Invalid chunk range".into()));
        }

        let filepath = if is_abs {
            path.join(&file_name)
        } else {
            let (target_path, _, _) =
                norm_abs_path(&self.root.read().await, &path.to_string_lossy())?;
            target_path.join(&file_name)
        };

        let meta = Arc::new(Mutex::new(Meta::default()));
        let upload_value = file_ranges(start, size, &file_name, &filepath, &batch_id, meta).await?;

        let mut file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&filepath)
            .await?;
        file.seek(SeekFrom::Start(start)).await?;
        file.write_all(&chunk_data).await?;
        file.flush().await?;

        let mut ranges = upload_value.ranges.lock().await;
        ranges.push(start..end);
        merge_ranges(&mut ranges);

        if is_upload_complete(&ranges, size) {
            info!("Upload complete: {file_name}");
            UPLOADS
                .lock()
                .await
                .remove(&filepath.to_string_lossy().to_string());
        }

        Ok(())
    }

    pub async fn watchman(
        &self,
        config: PlayoutConfig,
        is_alive: Arc<AtomicBool>,
        sources: Arc<Mutex<Vec<Media>>>,
    ) {
        if let Some(old_handle) = self.watch_handler.lock().await.take() {
            old_handle.abort();
        }

        let handle = tokio::spawn(watch(config, is_alive, sources));

        *self.watch_handler.lock().await = Some(handle);
    }

    pub async fn stop_watch(&self) {
        if let Some(handle) = self.watch_handler.lock().await.take() {
            handle.abort();
        }
    }

    pub async fn fill_filler_list(
        &self,
        config: &PlayoutConfig,
        fillers: Option<Arc<Mutex<Vec<Media>>>>,
    ) -> Vec<Media> {
        let id = config.general.channel_id;
        let mut filler_list = vec![];
        let filler_path = &config.storage.filler_path;

        if filler_path.is_dir() {
            let config_clone = config.clone();
            let mut index = 0;
            let mut entries = WalkDir::new(&config_clone.storage.filler_path);

            while let Some(Ok(entry)) = entries.next().await {
                if entry.path().is_file() && include_file_extension(config, &entry.path()) {
                    let mut media = Media::new(index, &entry.path().to_string_lossy(), false).await;

                    if fillers.is_none()
                        && let Err(e) = media.add_probe(false).await
                    {
                        error!(target: Target::file_mail(), channel = id; "{e:?}");
                    };

                    filler_list.push(media);
                    index += 1;
                }
            }

            if config.storage.shuffle {
                let mut rng = rand::rng();

                filler_list.shuffle(&mut rng);
            } else {
                filler_list.sort_by(|d1, d2| natural_lexical_cmp(&d1.source, &d2.source));
            }

            for (index, item) in filler_list.iter_mut().enumerate() {
                item.index = Some(index);
            }

            if let Some(f) = fillers.as_ref() {
                f.lock().await.clone_from(&filler_list);
            }
        } else if filler_path.is_file() {
            let mut media =
                Media::new(0, &config.storage.filler_path.to_string_lossy(), false).await;

            if fillers.is_none()
                && let Err(e) = media.add_probe(false).await
            {
                error!(target: Target::file_mail(), channel = id; "{e:?}");
            };

            filler_list.push(media);

            if let Some(f) = fillers.as_ref() {
                f.lock().await.clone_from(&filler_list);
            }
        }

        filler_list
    }

    pub async fn copy_assets(&self) -> Result<(), std::io::Error> {
        let root = self.root.read().await.clone();
        if root.is_dir() {
            let target = root.join("00-assets");
            let mut dummy_source = Path::new("/usr/share/ffplayout/dummy.vtt");
            let mut font_source = Path::new("/usr/share/ffplayout/DejaVuSans.ttf");
            let mut logo_source = Path::new("/usr/share/ffplayout/logo.png");

            if !dummy_source.is_file() {
                dummy_source = Path::new("./assets/dummy.vtt");
            }
            if !font_source.is_file() {
                font_source = Path::new("./assets/DejaVuSans.ttf");
            }
            if !logo_source.is_file() {
                logo_source = Path::new("./assets/logo.png");
            }

            if !target.is_dir() {
                let dummy_target = target.join("dummy.vtt");
                let font_target = target.join("DejaVuSans.ttf");
                let logo_target = target.join("logo.png");

                fs::create_dir_all(&target).await?;
                fs::copy(&dummy_source, &dummy_target).await?;
                fs::copy(&font_source, &font_target).await?;
                fs::copy(&logo_source, &logo_target).await?;

                #[cfg(target_family = "unix")]
                {
                    let uid = nix::unistd::Uid::current();
                    let parent_owner = root.metadata().unwrap().uid();

                    if uid.is_root() && uid.to_string() != parent_owner.to_string() {
                        let user = nix::unistd::User::from_uid(parent_owner.into())
                            .unwrap_or_default()
                            .unwrap();

                        nix::unistd::chown(&target, Some(user.uid), Some(user.gid))?;

                        if dummy_target.is_file() {
                            nix::unistd::chown(&dummy_target, Some(user.uid), Some(user.gid))?;
                        }
                        if font_target.is_file() {
                            nix::unistd::chown(&font_target, Some(user.uid), Some(user.gid))?;
                        }
                        if logo_target.is_file() {
                            nix::unistd::chown(&logo_target, Some(user.uid), Some(user.gid))?;
                        }
                    }
                }
            }
        } else {
            error!("Storage path {:?} not exists!", self.root);
        }

        Ok(())
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
