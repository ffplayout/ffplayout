use std::path::{Path, PathBuf};

use lexical_sort::{natural_lexical_cmp, PathSort};
use log::*;
use serde::{Deserialize, Serialize};
use tokio::{fs, io::AsyncWriteExt};

use crate::utils::errors::ServiceError;

use crate::file::{norm_abs_path, MoveObject, PathObject, Storage, VideoFile};
use crate::player::utils::{file_extension, probe::MediaProbe};

pub struct S3Storage {
    pub root: PathBuf,
    pub extensions: Vec<String>,
}

impl S3Storage {
    pub fn new(root: PathBuf, extensions: Vec<String>) -> Self {
        Self { root, extensions }
    }
}

impl Storage for S3Storage {
    async fn browser(&self, path_obj: &PathObject) -> Result<PathObject, ServiceError> {
        Ok(PathObject::default())
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
