use std::{
    borrow::BorrowMut,
    io::Write,
    path::{Path, PathBuf},
};

use actix_multipart::Multipart;
use actix_web::{web, HttpResponse};
use futures_util::{StreamExt, TryStreamExt as _};
use lexical_sort::{natural_lexical_cmp, PathSort};
use rand::{distributions::Alphanumeric, Rng};
use regex::Regex;
use relative_path::RelativePath;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::fs;

use log::*;

use crate::db::models::Channel;
use crate::player::utils::{file_extension, MediaProbe};
use crate::utils::{config::PlayoutConfig, errors::ServiceError, s3_utils};

use super::S3_INDICATOR;

const CHUNK_SIZE: u64 = 1024 * 1024 * 5;
const MAX_CHUNKS: u64 = 10000;

use aws_sdk_s3::{
    error::SdkError,
    operation::create_multipart_upload::CreateMultipartUploadOutput,
    presigning::PresigningConfig,
    primitives::ByteStream,
    types::{CompletedMultipartUpload, CompletedPart},
    Client,
};

#[derive(Debug, Deserialize, Serialize, Clone)]
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

    Ok((path.to_path_buf(), path_suffix, source_relative))
}

/// Prepares the raw input path for S3.
///
/// Ensures the path is valid for S3 configuration.
pub fn s3_path(input_path: &str) -> Result<(String, String), ServiceError> {
    fn s3_clean_path(input_path: &str) -> Result<String, ServiceError> {
        let re = Regex::new("//+").unwrap(); // Matches one or more '/'
        let none_redundant_path = re.replace_all(input_path, "/");
        let clean_path = if !none_redundant_path.is_empty() && none_redundant_path != "/" {
            if !input_path.ends_with("/") {
                format!("{}/", none_redundant_path.trim_start_matches("/"))
            } else {
                none_redundant_path.trim_start_matches("/").to_string()
            }
        } else {
            String::new()
        };
        Ok(clean_path)
    }
    let clean_path = s3_clean_path(input_path)?;
    let clean_parent_path = s3_clean_path(&format!(
        "{}/",
        clean_path
            .rsplit('/')
            .skip(2)
            .collect::<Vec<&str>>()
            .iter()
            .rev()
            .cloned()
            .collect::<Vec<&str>>()
            .join("/")
    ))?;

    Ok((clean_path, clean_parent_path))
}

/// Prepares the raw input path for S3.
///
/// Generates a presigned URL that provides temporary access to an S3 object.
async fn get_s3_object(
    client: &Client,
    bucket: &str,
    object: &str,
    expires_in: u64,
) -> Result<String, ServiceError> {
    let expires_in = Duration::from_secs(expires_in);
    let presigned_request = client
        .get_object()
        .bucket(bucket)
        .key(object)
        .presigned(
            PresigningConfig::expires_in(expires_in)
                .map_err(|_e| ServiceError::InternalServerError)?,
        )
        .await
        .map_err(|_e| ServiceError::InternalServerError)?;

    Ok(presigned_request.uri().to_string())
}

/// File Browser
///
/// Take input path and give file and folder list from it back.
/// Input should be a relative path segment, but when it is a absolut path, the norm_abs_path function
/// will take care, that user can not break out from given storage path in config.
pub async fn browser(
    config: &PlayoutConfig,
    channel: &Channel,
    path_obj: &PathObject,
) -> Result<PathObject, ServiceError> {
    let mut channel_extensions = channel
        .extra_extensions
        .split(',')
        .map(|e| e.to_string())
        .collect::<Vec<String>>();
    let mut parent_folders = vec![];
    let mut extensions = config.storage.extensions.clone();
    extensions.append(&mut channel_extensions);

    if channel.storage.starts_with(S3_INDICATOR) {
        // S3 Storage Browser
        let bucket: &str = config.channel.s3_storage.as_ref().unwrap().bucket.as_str();
        let path = path_obj.source.clone();

        let delimiter = '/'; // should be a single character
        let (prefix, parent_path) = s3_path(&path_obj.source)?;

        let s3_client = config.channel.s3_storage.as_ref().unwrap().client.clone();

        let mut obj = PathObject::new(path.clone(), Some(bucket.to_string()));
        obj.folders_only = path_obj.folders_only;

        if (path != parent_path && !path_obj.folders_only)
            || (!path.is_empty() && (parent_path.is_empty() || parent_path == "/"))
        // to-do: fix! this cause a bug that occur when you click back on root path after searching in sub folders
        {
            let childs_resp = s3_client
                .list_objects_v2()
                .bucket(bucket)
                .prefix(&parent_path)
                .delimiter(delimiter)
                .send()
                .await
                .map_err(|_e| ServiceError::InternalServerError)?;

            for prefix in childs_resp.common_prefixes() {
                if let Some(prefix) = prefix.prefix() {
                    let child = prefix.split(delimiter).nth_back(1).unwrap_or(prefix);
                    parent_folders.push(child.to_string());
                }
            }
            parent_folders.path_sort(natural_lexical_cmp);

            obj.parent_folders = Some(parent_folders);
        }

        let list_resp = s3_client
            .list_objects_v2()
            .bucket(bucket)
            .prefix(&prefix)
            .delimiter(delimiter)
            .send()
            .await
            .map_err(|_e| ServiceError::InternalServerError)?;

        let mut folders: Vec<String> = vec![];
        let mut files: Vec<String> = vec![];

        for prefix in list_resp.common_prefixes() {
            if let Some(prefix) = prefix.prefix() {
                let fldrs = prefix.split(delimiter).nth_back(1).unwrap_or(prefix);
                folders.push(fldrs.to_string());
            }
        }

        for objs in list_resp.contents() {
            if let Some(objs) = objs.key() {
                let fls = objs.strip_prefix(&prefix).unwrap_or(objs);
                files.push(fls.to_string());
            }
        }

        files.path_sort(natural_lexical_cmp);
        folders.path_sort(natural_lexical_cmp);

        let mut media_files = vec![];

        for file in files {
            let s3file_presigned_url = get_s3_object(
                &s3_client,
                bucket,
                &file,
                config.playlist.length_sec.unwrap_or(3600.0 * 24.0) as u64,
            )
            .await?;
            match MediaProbe::new(s3file_presigned_url.as_ref()) {
                Ok(probe) => {
                    let mut duration = 0.0;

                    if let Some(dur) = probe.format.duration {
                        duration = dur.parse().unwrap_or_default()
                    }

                    let video = VideoFile {
                        name: file.to_string(),
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
    } else {
        // Local Storage Browser
        let (path, parent, path_component) =
            norm_abs_path(&config.channel.storage, &path_obj.source)?;
        let parent_path = if !path_component.is_empty() {
            path.parent().unwrap()
        } else {
            &config.channel.storage
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
                    if extensions.contains(&ext.to_string().to_lowercase()) {
                        files.push(child.path())
                    }
                }
            }
        }

        folders.path_sort(natural_lexical_cmp);
        files.path_sort(natural_lexical_cmp);
        let mut media_files = vec![];

        for file in files {
            match MediaProbe::new(file.to_string_lossy().as_ref()) {
                Ok(probe) => {
                    let mut duration = 0.0;

                    if let Some(dur) = probe.format.duration {
                        duration = dur.parse().unwrap_or_default()
                    }

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
}

pub async fn create_directory(
    config: &PlayoutConfig,
    channel: &Channel,
    path_obj: &PathObject,
) -> Result<HttpResponse, ServiceError> {
    if channel.storage.starts_with(S3_INDICATOR) {
        // S3 Storage
        let bucket: &str = config.channel.s3_storage.as_ref().unwrap().bucket.as_str();

        let (folder_name, _) = s3_path(&path_obj.source)?;
        let txt_file_name = format!("{}null.txt", folder_name); // it should be made to validate the new folder's existence
        let s3_client = config.channel.s3_storage.as_ref().unwrap().client.clone();

        let body = aws_sdk_s3::primitives::ByteStream::from(Vec::new()); // to not consume bytes!
        s3_client
            .put_object()
            .bucket(bucket)
            .key(&txt_file_name)
            .body(body)
            .send()
            .await
            .map_err(|_e| ServiceError::InternalServerError)?;
    } else {
        // local storage
        let (path, _, _) = norm_abs_path(&config.channel.storage, &path_obj.source)?;

        if let Err(e) = fs::create_dir_all(&path).await {
            return Err(ServiceError::BadRequest(e.to_string()));
        }

        info!(
            "create folder: <b><magenta>{}</></b>",
            path.to_string_lossy()
        );
    }

    Ok(HttpResponse::Ok().into())
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

            return Ok(MoveObject {
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
            });
        }
        Err(e) => {
            error!("{e}");
            Err(ServiceError::BadRequest("Error in file copy!".into()))
        }
    }
}

async fn rename(source: &PathBuf, target: &PathBuf) -> Result<MoveObject, ServiceError> {
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

pub async fn rename_file(
    config: &PlayoutConfig,
    move_object: &MoveObject,
) -> Result<MoveObject, ServiceError> {
    let (source_path, _, _) = norm_abs_path(&config.channel.storage, &move_object.source)?;
    let (mut target_path, _, _) = norm_abs_path(&config.channel.storage, &move_object.target)?;

    if !source_path.exists() {
        return Err(ServiceError::BadRequest("Source file not exist!".into()));
    }

    if (source_path.is_dir() || source_path.is_file()) && source_path.parent() == Some(&target_path)
    {
        return rename(&source_path, &target_path).await;
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
        return rename(&source_path, &target_path).await;
    }

    Err(ServiceError::InternalServerError)
}

pub async fn remove_file_or_folder(
    config: &PlayoutConfig,
    source_path: &str,
    recursive: bool,
) -> Result<(), ServiceError> {
    let (source, _, _) = norm_abs_path(&config.channel.storage, source_path)?;

    if !source.exists() {
        return Err(ServiceError::BadRequest("Source does not exists!".into()));
    }

    if source.is_dir() {
        if recursive {
            match fs::remove_dir_all(source).await {
                Ok(_) => return Ok(()),
                Err(e) => {
                    error!("{e}");
                    return Err(ServiceError::BadRequest(
                        "Delete folder and its content failed!".into(),
                    ));
                }
            };
        } else {
            match fs::remove_dir(source).await {
                Ok(_) => return Ok(()),
                Err(e) => {
                    error!("{e}");
                    return Err(ServiceError::BadRequest(
                        "Delete folder failed! (Folder must be empty)".into(),
                    ));
                }
            };
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

async fn valid_path(config: &PlayoutConfig, path: &str) -> Result<PathBuf, ServiceError> {
    let (test_path, _, _) = norm_abs_path(&config.channel.storage, path)?;

    if !test_path.is_dir() {
        return Err(ServiceError::BadRequest("Target folder not exists!".into()));
    }

    Ok(test_path)
}

pub async fn upload(
    config: &PlayoutConfig,
    channel: Option<&Channel>,
    _size: u64,
    mut payload: Multipart,
    path: &Path,
    abs_path: bool,
) -> Result<HttpResponse, ServiceError> {
    if channel.unwrap().storage.starts_with(S3_INDICATOR) {
        // S3 multipart-upload
        let mut upload_id: Option<String> = None;
        let mut key: Option<String> = None;
        let bucket: &str = config.channel.s3_storage.as_ref().unwrap().bucket.as_str();
        let (path, _) = s3_path(&path.to_string_lossy())?;
        let s3_client = config.channel.s3_storage.as_ref().unwrap().client.clone();
        let mut completed_parts: Vec<CompletedPart> = Vec::new();
        let mut part_number = 1;
        while let Some(mut field) = payload.try_next().await? {
            let content_disposition = field
                .content_disposition()
                .ok_or("No content disposition")?;
            debug!("{content_disposition}");

            let rand_string: String = rand::thread_rng()
                .sample_iter(&Alphanumeric)
                .take(20)
                .map(char::from)
                .collect();

            let filename = content_disposition
                .get_filename()
                .map_or_else(|| rand_string.to_string(), sanitize_filename::sanitize);

            let filepath = format!("{path}{filename}");

            if upload_id.is_none() {
                let create_multipart_upload_output = s3_client
                    .create_multipart_upload()
                    .bucket(bucket)
                    .key(&filepath)
                    .send()
                    .await;

                match create_multipart_upload_output {
                    Ok(output) => {
                        upload_id = output.upload_id().map(|id| id.to_string());
                        key = Some(filepath);
                    }
                    Err(_e) => return Err(ServiceError::InternalServerError),
                }
            }

            let mut f = web::block(|| std::io::Cursor::new(Vec::new())).await?; // In-memory "file"

            loop {
                match field.try_next().await {
                    Ok(Some(chunk)) => {
                        f = web::block(move || f.write_all(&chunk).map(|_| f)).await??;
                    }
                    Ok(None) => break,
                    Err(e) => {
                        if e.to_string().contains("stream is incomplete") {
                            info!("Incomplete stream for part, continuing multipart upload: {e}");
                        }
                        return Err(e.into()); // Propagate error
                    }
                }
            }
            let body_bytes = actix_web::web::Bytes::from(f.into_inner());

            let upload_part_output = s3_client
                .upload_part()
                .bucket(bucket)
                .key(key.as_ref().unwrap())
                .upload_id(upload_id.as_ref().unwrap())
                .part_number(part_number)
                .body(body_bytes.into())
                .send()
                .await;

            match upload_part_output {
                Ok(output) => {
                    completed_parts.push(
                        CompletedPart::builder()
                            .e_tag(output.e_tag().unwrap())
                            .part_number(part_number)
                            .build(),
                    );
                    part_number += 1;
                }
                Err(_e) => return Err(ServiceError::InternalServerError),
            }
        }

        // Complete multipart upload (same as before)
        let completed_multipart_upload = CompletedMultipartUpload::builder()
            .set_parts(Some(completed_parts))
            .build();

        s3_client // final part
            .complete_multipart_upload()
            .bucket(bucket)
            .key(key.as_ref().unwrap())
            .upload_id(upload_id.as_ref().unwrap())
            .multipart_upload(completed_multipart_upload)
            .send()
            .await
            .map_err(|e| ServiceError::Conflict(e.to_string()))?;
    } else {
        // local storage upload
        while let Some(mut field) = payload.try_next().await? {
            let content_disposition = field.content_disposition().ok_or("No content")?;
            debug!("{content_disposition}");
            let rand_string: String = rand::thread_rng()
                .sample_iter(&Alphanumeric)
                .take(20)
                .map(char::from)
                .collect();
            let filename = content_disposition
                .get_filename()
                .map_or_else(|| rand_string.to_string(), sanitize_filename::sanitize);
            let filepath = if abs_path {
                path.to_path_buf()
            } else {
                valid_path(config, &path.to_string_lossy())
                    .await?
                    .join(filename)
            };
            let filepath_clone = filepath.clone();

            let _file_size = match filepath.metadata() {
                Ok(metadata) => metadata.len(),
                Err(_) => 0,
            };

            // INFO: File exist check should be enough because file size and content length are different.
            // The error catching in the loop should normally prevent unfinished files from existing on disk.
            // If this is not enough, a second check can be implemented: is_close(file_size as i64, size as i64, 1000)
            if filepath.is_file() {
                return Err(ServiceError::Conflict("Target already exists!".into()));
            }

            let mut f = web::block(|| std::fs::File::create(filepath_clone)).await??;

            loop {
                match field.try_next().await {
                    Ok(Some(chunk)) => {
                        f = web::block(move || f.write_all(&chunk).map(|_| f)).await??;
                    }

                    Ok(None) => break,

                    Err(e) => {
                        if e.to_string().contains("stream is incomplete") {
                            info!("Delete non finished file: {filepath:?}");

                            tokio::fs::remove_file(filepath).await?
                        }

                        return Err(e.into());
                    }
                }
            }
        }
    }

    Ok(HttpResponse::Ok().into())
}
