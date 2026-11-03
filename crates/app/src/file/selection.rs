use std::{
    io::ErrorKind,
    path::{Component, Path, PathBuf},
};

use lexical_sort::natural_lexical_cmp;
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::utils::errors::ServiceError;

use super::{ensure_path_within_root, local::LocalStorage};

#[derive(Deserialize)]
pub struct BrowseRequest {
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub initial: bool,
}

#[derive(Serialize)]
pub struct SelectionMediaInfo {
    /// Keep the existing default duration for playlist clip selection.
    pub duration: f64,
    pub source_duration: Option<f64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
}

#[derive(Serialize)]
pub struct Entry {
    pub name: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

#[derive(Serialize)]
pub struct Listing {
    pub path: String,
    pub breadcrumbs: Vec<Entry>,
    pub folders: Vec<Entry>,
    pub files: Vec<Entry>,
    pub selected_path: Option<String>,
    pub fallback: bool,
}

fn selection_path(root: &Path, source: &str) -> Result<PathBuf, ServiceError> {
    let source = Path::new(source);

    if source
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return Err(ServiceError::Forbidden("Access denied".into()));
    }

    let path = if source.as_os_str().is_empty() {
        root.to_path_buf()
    } else if source.is_absolute() {
        source.to_path_buf()
    } else {
        root.join(source)
    };
    ensure_path_within_root(root, &path)?;

    Ok(path)
}

impl LocalStorage {
    pub async fn selection_media_info(
        &self,
        source: &str,
    ) -> Result<SelectionMediaInfo, ServiceError> {
        let root = self.root.read().await.clone();
        let path = selection_path(&root, source)?;

        if !fs::metadata(&path).await?.is_file() {
            return Err(ServiceError::BadRequest("Not a file".into()));
        }

        tokio::task::spawn_blocking(move || {
            let probe = ff_engine::probe_media(&path.to_string_lossy()).map_err(|error| {
                ServiceError::BadRequest(format!("Cannot inspect media: {error}"))
            })?;

            let video = probe.video.first();
            let source_duration = probe
                .format
                .duration
                .or_else(|| video.and_then(|stream| stream.duration))
                .filter(|duration| duration.is_finite() && *duration > 0.0);

            Ok(SelectionMediaInfo {
                duration: source_duration.unwrap_or(10.0),
                source_duration,
                width: video
                    .and_then(|stream| stream.width)
                    .filter(|value| *value > 0),
                height: video
                    .and_then(|stream| stream.height)
                    .filter(|value| *value > 0),
            })
        })
        .await
        .map_err(|error| ServiceError::BadRequest(format!("Media inspection failed: {error}")))?
    }

    /// List selectable paths without media probing or the media-extension filter.
    pub async fn browse_selection(&self, request: &BrowseRequest) -> Result<Listing, ServiceError> {
        let root = self.root.read().await.clone();
        let mut path = selection_path(&root, &request.source)?;

        let mut selected_path = None;
        let mut fallback = false;

        loop {
            match fs::metadata(&path).await {
                Ok(metadata) if metadata.is_dir() => break,
                Ok(metadata) if request.initial && metadata.is_file() => {
                    selected_path = Some(path.to_string_lossy().into_owned());
                    path.pop();
                    break;
                }
                Err(error) if request.initial && error.kind() == ErrorKind::NotFound => {
                    fallback = true;
                    if path == root || !path.pop() {
                        return Err(error.into());
                    }
                }
                Err(error) => return Err(error.into()),
                _ => return Err(ServiceError::BadRequest("Not a directory".into())),
            }
        }

        let mut folders = Vec::new();
        let mut files = Vec::new();
        let mut directory = fs::read_dir(&path).await?;

        while let Some(child) = directory.next_entry().await? {
            let child_path = child.path();
            let name = child.file_name().to_string_lossy().into_owned();

            if name.starts_with('.') || ensure_path_within_root(&root, &child_path).is_err() {
                continue;
            }

            let metadata = match child.metadata().await {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            let entry = Entry {
                name,
                path: child_path.to_string_lossy().into_owned(),
                size: metadata.is_file().then_some(metadata.len()),
            };

            if metadata.is_dir() {
                folders.push(entry);
            } else if metadata.is_file() {
                files.push(entry);
            }
        }
        folders.sort_by(|a, b| natural_lexical_cmp(&a.name, &b.name));
        files.sort_by(|a, b| natural_lexical_cmp(&a.name, &b.name));

        let mut current = root.clone();
        let mut breadcrumbs = vec![Entry {
            name: root
                .file_name()
                .unwrap_or(root.as_os_str())
                .to_string_lossy()
                .into_owned(),
            path: root.to_string_lossy().into_owned(),
            size: None,
        }];

        for component in path
            .strip_prefix(&root)
            .unwrap_or(Path::new(""))
            .components()
        {
            current.push(component);
            breadcrumbs.push(Entry {
                name: component.as_os_str().to_string_lossy().into_owned(),
                path: current.to_string_lossy().into_owned(),
                size: None,
            });
        }

        Ok(Listing {
            path: path.to_string_lossy().into_owned(),
            breadcrumbs,
            folders,
            files,
            selected_path,
            fallback,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn selection_lists_nonmedia_files_and_restores_file_and_missing_paths() {
        let root = std::env::temp_dir().join(format!("selection-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("nested")).await.unwrap();
        fs::write(root.join("nested/captions.vtt"), b"not media")
            .await
            .unwrap();
        fs::write(root.join("nested/task.sh"), b"not media")
            .await
            .unwrap();
        let storage = LocalStorage::new(root.clone(), vec!["mp4".into()])
            .await
            .unwrap();
        let listing = storage
            .browse_selection(&BrowseRequest {
                source: root
                    .join("nested/captions.vtt")
                    .to_string_lossy()
                    .into_owned(),
                initial: true,
            })
            .await
            .unwrap();
        assert_eq!(listing.files.len(), 2);
        assert!(listing.files.iter().all(|file| file.size == Some(9)));
        assert!(listing.breadcrumbs.iter().all(|entry| entry.size.is_none()));
        assert!(listing.selected_path.unwrap().ends_with("captions.vtt"));
        assert_eq!(listing.breadcrumbs.len(), 2);
        assert!(!listing.fallback);

        let listing = storage
            .browse_selection(&BrowseRequest {
                source: root
                    .join("nested/missing/filler.mp4")
                    .to_string_lossy()
                    .into_owned(),
                initial: true,
            })
            .await
            .unwrap();
        assert_eq!(Path::new(&listing.path), root.join("nested"));
        assert!(listing.fallback);
        assert!(listing.selected_path.is_none());
        assert!(
            storage
                .browse_selection(&BrowseRequest {
                    source: root.join("../outside").to_string_lossy().into_owned(),
                    initial: true,
                })
                .await
                .is_err()
        );
        fs::remove_dir_all(root).await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn selection_rejects_external_symlinks_and_hides_them_in_listings() {
        let directory = std::env::temp_dir().join(format!("selection-{}", uuid::Uuid::new_v4()));
        let root = directory.join("storage");
        fs::create_dir_all(&root).await.unwrap();
        fs::write(directory.join("secret"), b"secret")
            .await
            .unwrap();
        std::os::unix::fs::symlink(directory.join("secret"), root.join("link")).unwrap();
        let storage = LocalStorage::new(root, vec![]).await.unwrap();
        assert!(
            storage
                .browse_selection(&BrowseRequest {
                    source: "link".into(),
                    initial: true
                })
                .await
                .is_err()
        );
        let listing = storage
            .browse_selection(&BrowseRequest {
                source: String::new(),
                initial: false,
            })
            .await
            .unwrap();
        assert!(listing.files.is_empty());
        assert!(listing.folders.is_empty());
        fs::remove_dir_all(directory).await.unwrap();
    }
}
