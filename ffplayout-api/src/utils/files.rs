use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use actix_multipart::Multipart;
use actix_web::{web, HttpResponse};
use futures_util::TryStreamExt as _;
use rand::{distributions::Alphanumeric, Rng};
use relative_path::RelativePath;
use serde::{Deserialize, Serialize};

use simplelog::*;

use crate::utils::{errors::ServiceError, playout_config};
use ffplayout_lib::utils::file_extension;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PathObject {
    pub source: String,
    folders: Option<Vec<String>>,
    files: Option<Vec<String>>,
}

impl PathObject {
    fn new(source: String) -> Self {
        Self {
            source,
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

pub async fn browser(id: i64, path_obj: &PathObject) -> Result<PathObject, ServiceError> {
    let (config, _) = playout_config(&id).await?;
    let path = PathBuf::from(config.storage.path);
    let extensions = config.storage.extensions;
    let path_component = RelativePath::new(&path_obj.source)
        .normalize()
        .to_string()
        .replace("../", "");
    let path = path.join(path_component.clone());
    let mut obj = PathObject::new(path_component.clone());

    let mut paths: Vec<_> = match fs::read_dir(path) {
        Ok(p) => p.filter_map(|r| r.ok()).collect(),
        Err(e) => {
            error!("{e} in {path_component}");
            return Err(ServiceError::InternalServerError);
        }
    };

    paths.sort_by_key(|dir| dir.path());

    for path in paths {
        let file_path = path.path().to_owned();
        let path = file_path.clone();

        // ignore hidden files/folders on unix
        if path.display().to_string().contains("/.") {
            continue;
        }

        if file_path.is_dir() {
            if let Some(ref mut folders) = obj.folders {
                folders.push(path.file_name().unwrap().to_string_lossy().to_string());
            }
        } else if file_path.is_file() {
            if let Some(ext) = file_extension(&file_path) {
                if extensions.contains(&ext.to_string().to_lowercase()) {
                    if let Some(ref mut files) = obj.files {
                        files.push(path.file_name().unwrap().to_string_lossy().to_string());
                    }
                }
            }
        }
    }

    Ok(obj)
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
    match fs::rename(&source, &target) {
        Ok(_) => Ok(MoveObject {
            source: source.display().to_string(),
            target: target.display().to_string(),
        }),
        Err(e) => {
            error!("{e}");
            Err(ServiceError::BadRequest("Rename failed!".into()))
        }
    }
}

pub async fn rename_file(id: i64, move_object: &MoveObject) -> Result<MoveObject, ServiceError> {
    let (config, _) = playout_config(&id).await?;
    let path = PathBuf::from(&config.storage.path);
    let source = RelativePath::new(&move_object.source)
        .normalize()
        .to_string()
        .replace("../", "");
    let target = RelativePath::new(&move_object.target)
        .normalize()
        .to_string()
        .replace("../", "");

    let mut source_path = PathBuf::from(source.clone());
    let mut target_path = PathBuf::from(target.clone());

    let relativ_path = RelativePath::new(&config.storage.path)
        .normalize()
        .to_string();

    source_path = match source_path.starts_with(&relativ_path) {
        true => path.join(source_path.strip_prefix(&relativ_path).unwrap()),
        false => path.join(source),
    };

    target_path = match target_path.starts_with(&relativ_path) {
        true => path.join(target_path.strip_prefix(relativ_path).unwrap()),
        false => path.join(target),
    };

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

pub async fn remove_file_or_folder(id: i64, source_path: &str) -> Result<(), ServiceError> {
    let (config, _) = playout_config(&id).await?;
    let source = PathBuf::from(source_path);

    let test_source = RelativePath::new(&source_path)
        .normalize()
        .to_string()
        .replace("../", "");

    let test_path = RelativePath::new(&config.storage.path)
        .normalize()
        .to_string();

    if !test_source.starts_with(&test_path) {
        return Err(ServiceError::BadRequest(
            "Source file is not in storage!".into(),
        ));
    }

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

async fn valid_path(id: i64, path: &str) -> Result<(), ServiceError> {
    let (config, _) = playout_config(&id).await?;

    let test_target = RelativePath::new(&path)
        .normalize()
        .to_string()
        .replace("../", "");

    let test_path = RelativePath::new(&config.storage.path)
        .normalize()
        .to_string();

    if !test_target.starts_with(&test_path) {
        return Err(ServiceError::BadRequest(
            "Target folder is not in storage!".into(),
        ));
    }

    if !Path::new(path).is_dir() {
        return Err(ServiceError::BadRequest("Target folder not exists!".into()));
    }

    Ok(())
}

pub async fn upload(id: i64, mut payload: Multipart) -> Result<HttpResponse, ServiceError> {
    while let Some(mut field) = payload.try_next().await? {
        let content_disposition = field.content_disposition();
        debug!("{content_disposition}");
        let rand_string: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(20)
            .map(char::from)
            .collect();
        let path_name = content_disposition.get_name().unwrap_or(&rand_string);
        let filename = content_disposition
            .get_filename()
            .map_or_else(|| rand_string.to_string(), sanitize_filename::sanitize);

        if let Err(e) = valid_path(id, path_name).await {
            return Err(e);
        }

        let filepath = PathBuf::from(path_name).join(filename);

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
