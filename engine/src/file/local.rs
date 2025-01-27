use std::path::{Path, PathBuf};

use lexical_sort::{natural_lexical_cmp, PathSort};
use log::*;
use serde::{Deserialize, Serialize};
use tokio::{fs, io::AsyncWriteExt};

use crate::utils::errors::ServiceError;

use crate::file::{norm_abs_path, MoveObject, PathObject, Storage, VideoFile};
use crate::player::utils::{file_extension, probe::MediaProbe};

pub struct LocalStorage {
    pub root: PathBuf,
    pub extensions: Vec<String>,
}

impl LocalStorage {
    pub fn new(root: PathBuf, extensions: Vec<String>) -> Self {
        Self { root, extensions }
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

    async fn create_directory(&self, path_obj: &PathObject) -> Result<(), ServiceError> {
        Ok(())
    }
    async fn rename(&self, move_object: &MoveObject) -> Result<MoveObject, ServiceError> {
        Ok(MoveObject::default())
    }
    async fn remove(&self, source_path: &str, recursive: bool) -> Result<(), ServiceError> {
        Ok(())
    }
    async fn upload(&self, source_path: &str, recursive: bool) -> Result<(), ServiceError> {
        Ok(())
    }
}
