use std::{fs, io::Write, path::PathBuf};

use actix_multipart::Multipart;
use actix_web::{web, HttpResponse};
use futures_util::TryStreamExt as _;
use rand::{distributions::Alphanumeric, Rng};
use relative_path::RelativePath;
use serde::{Deserialize, Serialize};

use simplelog::*;

use crate::utils::{errors::ServiceError, playout_config};
use ffplayout_lib::utils::{file_extension, MediaProbe};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PathObject {
    pub source: String,
    parent: Option<String>,
    folders: Option<Vec<String>>,
    files: Option<Vec<VideoFile>>,
}

impl PathObject {
    fn new(source: String, parent: Option<String>) -> Self {
        Self {
            source,
            parent,
            folders: Some(vec![]),
            files: Some(vec![]),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MoveObject {
    source: String,
    target: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct VideoFile {
    name: String,
    duration: f64,
}

/// Normalize absolut path
///
/// This function takes care, that it is not possible to break out from root_path.
/// It also gives alway a relative path back.
fn norm_abs_path(root_path: &str, input_path: &str) -> (PathBuf, String, String) {
    let mut path = PathBuf::from(root_path);
    let path_relative = RelativePath::new(root_path)
        .normalize()
        .to_string()
        .replace("../", "");
    let mut source_relative = RelativePath::new(input_path)
        .normalize()
        .to_string()
        .replace("../", "");
    let path_suffix = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    if input_path.starts_with(root_path) || source_relative.starts_with(&path_relative) {
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

    path = path.join(&source_relative);

    (path, path_suffix, source_relative)
}

/// File Browser
///
/// Take input path and give file and folder list from it back.
/// Input should be a relative path segment, but when it is a absolut path, the norm_abs_path function
/// will take care, that user can not break out from given storage path in config.
pub async fn browser(id: i32, path_obj: &PathObject) -> Result<PathObject, ServiceError> {
    let (config, _) = playout_config(&id).await?;
    let extensions = config.storage.extensions;
    let (path, parent, path_component) = norm_abs_path(&config.storage.path, &path_obj.source);
    let mut obj = PathObject::new(path_component, Some(parent));

    let mut paths: Vec<_> = match fs::read_dir(path) {
        Ok(p) => p.filter_map(|r| r.ok()).collect(),
        Err(e) => {
            error!("{e} in {}", path_obj.source);
            return Err(ServiceError::NoContent(e.to_string()));
        }
    };

    paths.sort_by_key(|dir| dir.path().display().to_string().to_lowercase());
    let mut files = vec![];
    let mut folders = vec![];

    for path in paths {
        let file_path = path.path().to_owned();
        let path = file_path.clone();

        // ignore hidden files/folders on unix
        if path.display().to_string().contains("/.") {
            continue;
        }

        if file_path.is_dir() {
            folders.push(path.file_name().unwrap().to_string_lossy().to_string());
        } else if file_path.is_file() {
            if let Some(ext) = file_extension(&file_path) {
                if extensions.contains(&ext.to_string().to_lowercase()) {
                    let media = MediaProbe::new(&path.display().to_string());
                    let mut duration = 0.0;

                    if let Some(dur) = media.format.and_then(|f| f.duration) {
                        duration = dur.parse().unwrap_or(0.0)
                    }

                    let video = VideoFile {
                        name: path.file_name().unwrap().to_string_lossy().to_string(),
                        duration,
                    };
                    files.push(video);
                }
            }
        }
    }

    obj.folders = Some(folders);
    obj.files = Some(files);

    Ok(obj)
}

pub async fn create_directory(
    id: i32,
    path_obj: &PathObject,
) -> Result<HttpResponse, ServiceError> {
    let (config, _) = playout_config(&id).await?;
    let (path, _, _) = norm_abs_path(&config.storage.path, &path_obj.source);

    if let Err(e) = fs::create_dir_all(&path) {
        return Err(ServiceError::BadRequest(e.to_string()));
    }

    info!("create folder: <b><magenta>{}</></b>", path.display());

    Ok(HttpResponse::Ok().into())
}

// fn copy_and_delete(source: &PathBuf, target: &PathBuf) -> Result<PathObject, ServiceError> {
//     match fs::copy(&source, &target) {
//         Ok(_) => {
//             if let Err(e) = fs::remove_file(source) {
//                 error!("{e}");
//                 return Err(ServiceError::BadRequest(
//                     "Removing File not possible!".into(),
//                 ));
//             };

//             return Ok(PathObject::new(target.display().to_string()));
//         }
//         Err(e) => {
//             error!("{e}");
//             Err(ServiceError::BadRequest("Error in file copy!".into()))
//         }
//     }
// }

fn rename(source: &PathBuf, target: &PathBuf) -> Result<MoveObject, ServiceError> {
    match fs::rename(source, target) {
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
            Err(ServiceError::BadRequest("Rename failed!".into()))
        }
    }
}

pub async fn rename_file(id: i32, move_object: &MoveObject) -> Result<MoveObject, ServiceError> {
    let (config, _) = playout_config(&id).await?;
    let (source_path, _, _) = norm_abs_path(&config.storage.path, &move_object.source);
    let (mut target_path, _, _) = norm_abs_path(&config.storage.path, &move_object.target);

    if !source_path.exists() {
        return Err(ServiceError::BadRequest("Source file not exist!".into()));
    }

    if (source_path.is_dir() || source_path.is_file()) && source_path.parent() == Some(&target_path)
    {
        return rename(&source_path, &target_path);
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
        return rename(&source_path, &target_path);
    }

    Err(ServiceError::InternalServerError)
}

pub async fn remove_file_or_folder(id: i32, source_path: &str) -> Result<(), ServiceError> {
    let (config, _) = playout_config(&id).await?;
    let (source, _, _) = norm_abs_path(&config.storage.path, source_path);

    if !source.exists() {
        return Err(ServiceError::BadRequest("Source does not exists!".into()));
    }

    if source.is_dir() {
        match fs::remove_dir(source) {
            Ok(_) => return Ok(()),
            Err(e) => {
                error!("{e}");
                return Err(ServiceError::BadRequest(
                    "Delete folder failed! (Folder must be empty)".into(),
                ));
            }
        };
    }

    if source.is_file() {
        match fs::remove_file(source) {
            Ok(_) => return Ok(()),
            Err(e) => {
                error!("{e}");
                return Err(ServiceError::BadRequest("Delete file failed!".into()));
            }
        };
    }

    Err(ServiceError::InternalServerError)
}

async fn valid_path(id: i32, path: &str) -> Result<PathBuf, ServiceError> {
    let (config, _) = playout_config(&id).await?;
    let (test_path, _, _) = norm_abs_path(&config.storage.path, path);

    if !test_path.is_dir() {
        return Err(ServiceError::BadRequest("Target folder not exists!".into()));
    }

    Ok(test_path)
}

pub async fn upload(
    id: i32,
    mut payload: Multipart,
    path: &str,
    abs_path: bool,
) -> Result<HttpResponse, ServiceError> {
    while let Some(mut field) = payload.try_next().await? {
        let content_disposition = field.content_disposition();
        debug!("{content_disposition}");
        let rand_string: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(20)
            .map(char::from)
            .collect();
        let filename = content_disposition
            .get_filename()
            .map_or_else(|| rand_string.to_string(), sanitize_filename::sanitize);

        let filepath;

        if abs_path {
            filepath = PathBuf::from(path);
        } else {
            let target_path = valid_path(id, path).await?;
            filepath = target_path.join(filename);
        }

        if filepath.is_file() {
            return Err(ServiceError::BadRequest("Target already exists!".into()));
        }

        let mut f = web::block(|| std::fs::File::create(filepath)).await??;

        while let Some(chunk) = field.try_next().await? {
            f = web::block(move || f.write_all(&chunk).map(|_| f)).await??;
        }
    }

    Ok(HttpResponse::Ok().into())
}
