use std::path::{Path, PathBuf};

use relative_path::RelativePath;
use serde::{Deserialize, Serialize};

mod local;
mod s3;

use crate::utils::{errors::ServiceError, system::Load};

enum StorageType {
    Local,
    S3,
}
enum StorageBackend {
    Local(local::LocalStorage),
    S3(s3::S3Storage),
}

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

trait Storage {
    async fn browser(&self, path_obj: &PathObject) -> Result<PathObject, ServiceError>;
    async fn create_directory(&self, path_obj: &PathObject) -> Result<(), ServiceError>;
    async fn rename(&self, move_object: &MoveObject) -> Result<MoveObject, ServiceError>;
    async fn remove(&self, source_path: &str, recursive: bool) -> Result<(), ServiceError>;
    async fn upload(&self, source_path: &str, recursive: bool) -> Result<(), ServiceError>;
}

fn create_storage(
    storage_type: StorageType,
    root: PathBuf,
    extensions: Vec<String>,
) -> StorageBackend {
    match storage_type {
        StorageType::Local => StorageBackend::Local(local::LocalStorage::new(root, extensions)),
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
