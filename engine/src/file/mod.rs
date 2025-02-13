use std::{
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
};

use actix_multipart::Multipart;
use relative_path::RelativePath;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

mod local;
mod s3;
mod watcher;

use crate::player::utils::Media;
use crate::utils::{config::PlayoutConfig, errors::ServiceError};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct PathObject {
    pub source: String,
    parent: Option<String>,
    parent_folders: Option<Vec<String>>,
    folders: Option<Vec<String>>,
    files: Option<Vec<VideoFile>>,
    #[serde(default)]
    pub folders_only: bool,
    #[serde(default)]
    pub recursive: bool,
}

impl PathObject {
    fn new(source: String, parent: Option<String>) -> Self {
        Self {
            source,
            parent,
            parent_folders: Some(vec![]),
            folders: Some(vec![]),
            files: Some(vec![]),
            folders_only: false,
            recursive: false,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MoveObject {
    source: String,
    target: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct VideoFile {
    name: String,
    duration: f64,
}

#[derive(Clone, Debug)]
pub enum StorageType {
    Local,
    S3,
}

#[derive(Clone, Debug)]
pub enum StorageBackend {
    Local(local::LocalStorage),
    S3(s3::S3Storage),
}

impl StorageBackend {
    pub async fn browser(&self, path_obj: &PathObject) -> Result<PathObject, ServiceError> {
        match self {
            StorageBackend::Local(storage) => storage.browser(path_obj).await,
            StorageBackend::S3(storage) => storage.browser(path_obj).await,
        }
    }

    pub async fn mkdir(&self, path_obj: &PathObject) -> Result<(), ServiceError> {
        match self {
            StorageBackend::Local(storage) => storage.mkdir(path_obj).await,
            StorageBackend::S3(storage) => storage.mkdir(path_obj).await,
        }
    }

    pub async fn rename(&self, move_object: &MoveObject) -> Result<MoveObject, ServiceError> {
        match self {
            StorageBackend::Local(storage) => storage.rename(move_object).await,
            StorageBackend::S3(storage) => storage.rename(move_object).await,
        }
    }

    pub async fn remove(&self, source_path: &str, recursive: bool) -> Result<(), ServiceError> {
        match self {
            StorageBackend::Local(storage) => storage.remove(source_path, recursive).await,
            StorageBackend::S3(storage) => storage.remove(source_path, recursive).await,
        }
    }

    pub async fn upload(
        &self,
        payload: Multipart,
        path: &Path,
        is_abs: bool,
    ) -> Result<(), ServiceError> {
        match self {
            StorageBackend::Local(storage) => storage.upload(payload, path, is_abs).await,
            StorageBackend::S3(storage) => storage.upload(payload, path, is_abs).await,
        }
    }

    pub async fn watchman(
        &mut self,
        config: PlayoutConfig,
        is_alive: Arc<AtomicBool>,
        sources: Arc<Mutex<Vec<Media>>>,
    ) {
        match self {
            StorageBackend::Local(storage) => storage.watchman(config, is_alive, sources).await,
            StorageBackend::S3(storage) => storage.watchman(config, is_alive, sources).await,
        }
    }

    pub async fn stop_watch(&mut self) {
        match self {
            StorageBackend::Local(storage) => storage.stop_watch().await,
            StorageBackend::S3(storage) => storage.stop_watch().await,
        }
    }

    pub async fn fill_filler_list(
        &mut self,
        config: &PlayoutConfig,
        fillers: Option<Arc<Mutex<Vec<Media>>>>,
    ) -> Vec<Media> {
        match self {
            StorageBackend::Local(storage) => storage.fill_filler_list(config, fillers).await,
            StorageBackend::S3(storage) => storage.fill_filler_list(config, fillers).await,
        }
    }

    pub async fn copy_assets(&self) -> Result<(), std::io::Error> {
        match self {
            StorageBackend::Local(storage) => storage.copy_assets().await,
            StorageBackend::S3(storage) => storage.copy_assets().await,
        }
    }

    pub fn is_dir<P: AsRef<Path>>(&self, input: P) -> bool {
        match self {
            StorageBackend::Local(storage) => storage.is_dir(input),
            StorageBackend::S3(storage) => storage.is_dir(input),
        }
    }

    pub fn is_file<P: AsRef<Path>>(&self, input: P) -> bool {
        match self {
            StorageBackend::Local(storage) => storage.is_file(input),
            StorageBackend::S3(storage) => storage.is_file(input),
        }
    }
}

trait Storage {
    async fn browser(&self, path_obj: &PathObject) -> Result<PathObject, ServiceError>;
    async fn mkdir(&self, path_obj: &PathObject) -> Result<(), ServiceError>;
    async fn rename(&self, move_object: &MoveObject) -> Result<MoveObject, ServiceError>;
    async fn remove(&self, source_path: &str, recursive: bool) -> Result<(), ServiceError>;
    async fn upload(&self, data: Multipart, path: &Path, is_abs: bool) -> Result<(), ServiceError>;
    async fn watchman(
        &mut self,
        config: PlayoutConfig,
        is_alive: Arc<AtomicBool>,
        sources: Arc<Mutex<Vec<Media>>>,
    );
    async fn stop_watch(&mut self);
    async fn fill_filler_list(
        &mut self,
        config: &PlayoutConfig,
        fillers: Option<Arc<Mutex<Vec<Media>>>>,
    ) -> Vec<Media>;
    async fn copy_assets(&self) -> Result<(), std::io::Error>;
    fn is_dir<P: AsRef<Path>>(&self, input: P) -> bool;
    fn is_file<P: AsRef<Path>>(&self, input: P) -> bool;
}

pub fn select_storage_type<S: AsRef<std::ffi::OsStr>>(path: S) -> StorageType {
    let path_str = path.as_ref().to_string_lossy().to_lowercase();

    if path_str.starts_with("s3://") {
        return StorageType::S3;
    }

    StorageType::Local
}

pub async fn init_storage(
    storage_type: StorageType,
    root: PathBuf,
    extensions: Vec<String>,
) -> StorageBackend {
    match storage_type {
        StorageType::Local => {
            StorageBackend::Local(local::LocalStorage::new(root, extensions).await)
        }
        StorageType::S3 => StorageBackend::S3(s3::S3Storage::new(root, extensions)),
    }
}

/// Normalize absolut path
///
/// This function takes care, that it is not possible to break out from root_path.
pub fn norm_abs_path(
    root_path: &Path,
    input_path: &str,
) -> Result<(PathBuf, String, String), ServiceError> {
    let path_relative = RelativePath::new(&root_path.to_string_lossy())
        .normalize()
        .to_string()
        .replace("../", "");
    let path_suffix = root_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let mut source_relative = RelativePath::new(input_path)
        .normalize()
        .to_string()
        .replace("../", "");

    if input_path.starts_with(&*root_path.to_string_lossy())
        || source_relative.starts_with(&path_relative)
    {
        source_relative = source_relative
            .strip_prefix(&path_relative)
            .and_then(|s| s.strip_prefix('/'))
            .unwrap_or_default()
            .to_string();
    } else {
        source_relative = source_relative
            .strip_prefix(&path_suffix)
            .and_then(|s| s.strip_prefix('/'))
            .unwrap_or(&source_relative)
            .to_string();
    }

    let path = &root_path.join(&source_relative);

    Ok((path.clone(), path_suffix, source_relative))
}
