use std::path::{Path, PathBuf};

use path_clean::PathClean;
use relative_path::RelativePath;
use serde::{Deserialize, Serialize};

pub mod local;
mod upload;
mod watcher;

use crate::utils::errors::ServiceError;
use local::LocalStorage;
pub(crate) use upload::MAX_UPLOAD_REQUEST_SIZE;
pub use upload::{UploadStatus, UploadStatusQuery};

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

pub async fn init_storage(
    root: PathBuf,
    extensions: Vec<String>,
) -> Result<LocalStorage, ServiceError> {
    LocalStorage::new(root, extensions).await
}

/// Normalize absolut path
///
/// This function takes care, that it is not possible to break out from root_path.
pub fn norm_abs_path(
    root_path: &Path,
    input_path: &str,
) -> Result<(PathBuf, String, String), ServiceError> {
    let path_relative = strip_parent_segments(
        RelativePath::new(&root_path.to_string_lossy())
            .normalize()
            .as_str(),
    );
    let path_suffix = root_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let mut source_relative =
        strip_parent_segments(RelativePath::new(input_path).normalize().as_str());

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

    let path = root_path.join(&source_relative);

    // Defensive containment check: the cleaned absolute path must never leave
    // the storage root, regardless of the normalization above.
    let cleaned = path.clean();
    let cleaned_root = root_path.clean();
    if !cleaned.starts_with(&cleaned_root) {
        return Err(ServiceError::Forbidden("Access denied".to_string()));
    }

    Ok((path, path_suffix, source_relative))
}

/// Removes every `../` traversal segment, repeating until the string is stable
/// so a single non-recursive pass cannot leave a reconstructed `../` behind.
fn strip_parent_segments(value: &str) -> String {
    let mut result = value.to_string();
    loop {
        let stripped = result.replace("../", "");
        if stripped == result {
            return stripped;
        }
        result = stripped;
    }
}
