use std::{
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
};

use actix_multipart::Multipart;
use tokio::sync::Mutex;

use crate::file::{MoveObject, PathObject, Storage};
use crate::player::utils::Media;
use crate::utils::{config::PlayoutConfig, errors::ServiceError};

#[derive(Clone, Debug)]
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
    async fn browser(&self, _path_obj: &PathObject) -> Result<PathObject, ServiceError> {
        Ok(PathObject::default())
    }

    async fn mkdir(&self, _path_obj: &PathObject) -> Result<(), ServiceError> {
        Ok(())
    }
    async fn rename(&self, _move_object: &MoveObject) -> Result<MoveObject, ServiceError> {
        Ok(MoveObject::default())
    }
    async fn remove(&self, _source_path: &str, _recursive: bool) -> Result<(), ServiceError> {
        Ok(())
    }
    async fn upload(
        &self,
        _data: Multipart,
        _path: &Path,
        _is_abs: bool,
    ) -> Result<(), ServiceError> {
        Ok(())
    }
    async fn watchman(
        &mut self,
        _config: PlayoutConfig,
        _is_alive: Arc<AtomicBool>,
        _sources: Arc<Mutex<Vec<Media>>>,
    ) {
    }

    async fn stop_watch(&mut self) {}
}
