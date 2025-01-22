use actix_multipart::Multipart;
use actix_web::web;
use futures_util::TryStreamExt as _;
use lexical_sort::{natural_lexical_cmp, PathSort};
use rand::{distributions::Alphanumeric, Rng};
use regex::Regex;
use std::io::Write;
use std::path::PathBuf;
use std::{
    io::{Error, ErrorKind},
    time::Duration,
};

use log::*;

use super::files::MediaMap;
use super::{
    errors::ServiceError,
    files::{MoveObject, PathObject, VideoFile},
};
use crate::{db::models, player::utils::probe::MediaProbe, utils::config::PlayoutConfig};
use aws_config::Region;
use aws_sdk_s3::{
    presigning::PresigningConfig,
    types::{CompletedMultipartUpload, CompletedPart},
    Client,
};

pub const S3_INDICATOR: &str = "s3://";
pub const S3_DEFAULT_PRESIGNEDURL_EXP: f64 = 3600.0 * 24.0;

pub trait S3Ext {
    // to-do : check if its unessential!
    fn parse_is_s3(&self) -> bool;
}

impl S3Ext for String {
    fn parse_is_s3(&self) -> bool {
        self.starts_with(S3_INDICATOR)
    }
}
impl S3Ext for &str {
    fn parse_is_s3(&self) -> bool {
        self.starts_with(S3_INDICATOR)
    }
}
impl S3Ext for PathBuf {
    fn parse_is_s3(&self) -> bool {
        self.starts_with(S3_INDICATOR)
    }
}

/// **S3 Client Initializer**
///
/// Initializes an S3 client using credentials and endpoint details from a `Channel`.
///
/// ## Parameters
/// - **`channel: &models::Channel`**: Source of S3 configuration.
///
/// ## Returns
/// - **`aws_sdk_s3::Client`**: Configured S3 client.
pub async fn s3_initialize_client(channel: &models::Channel) -> Client {
    let shared_provider = aws_sdk_s3::config::SharedCredentialsProvider::new(
        channel.storage.get_s3_credentials().unwrap(),
    );
    let config = aws_config::from_env()
        .region(Region::new("us-east-1")) // Dummy default region, replace if needed!
        .credentials_provider(shared_provider)
        .load()
        .await;

    let s3_config = aws_sdk_s3::config::Builder::from(&config)
        .endpoint_url(channel.storage.get_s3_endpointurl().unwrap())
        .force_path_style(true)
        .build();

    aws_sdk_s3::Client::from_conf(s3_config)
}

/// **S3 String Parser**
///
/// ## Purpose
/// Parses S3 configuration details from a provided string, extracting the bucket name, endpoint URL, and AWS credentials.
///
/// ## Input Format
/// The input string must follow this format:
/// `s3://{bucket_name}/:{endpoint_url}/:{access_key}/:{secret_key}`
///
/// - **`bucket_name`**: The name of the S3 bucket.
/// - **`endpoint_url`**: The URL of the S3 endpoint. If missing `http://` or `https://`, `http://` is automatically added.
/// - **`access_key`**: The AWS or S3 access key.
/// - **`secret_key`**: The AWS or S3 secret key.
///
/// ## Example
/// ```rust
/// let s3_string = "s3://my_bucket/:http://example.com/:my_access_key/:my_secret_key";
/// match s3_parse_string(s3_string) {
///     Ok((credentials, bucket_name, endpoint_url)) => {
///         assert_eq!(bucket_name, "my_bucket");
///         assert_eq!(endpoint_url, "http://example.com");
///         assert_eq!(credentials.access_key_id(), "my_access_key");
///     }
///     Err(e) => eprintln!("Error: {}", e),
/// }
/// ```
///
/// ## Returns
/// A `Result` containing:
/// - **`aws_sdk_s3::config::Credentials`**: AWS credentials.
/// - **`String`**: The S3 bucket name.
/// - **`String`**: The endpoint URL.
///
/// Returns an error if the string does not match the expected format.
///
/// ## Errors
/// - Returns `std::io::Error` if the input is invalid.
///
/// ## Use Case
/// Use this function to parse S3 configurations from strings, such as environment variables or config files.
pub fn s3_parse_string(
    // to_do: maybe should change the snippet to get better understanding
    s3_str: &str,
) -> Result<(aws_sdk_s3::config::Credentials, String, String), std::io::Error> {
    let pattern = format!(r"{}([^/]+)/:(.*?)/:([^/]+)/:([^/]+)", S3_INDICATOR);

    let re = Regex::new(&pattern)
        .map_err(|_| Error::new(ErrorKind::InvalidInput, "Failed to compile regex"))?;

    // Match the input string against the regex
    if let Some(captures) = re.captures(s3_str) {
        let access_key = captures[3].to_string();
        let secret_key = captures[4].to_string();
        let mut endpoint = captures[2].to_string();

        if !endpoint.starts_with("http://") && !endpoint.starts_with("https://") {
            endpoint = format!("http://{}", endpoint);
        }

        Ok((
            aws_sdk_s3::config::Credentials::new(access_key, secret_key, None, None, "None"), // Credential
            captures[1].to_string(), // bucket-name
            endpoint,                // endpoint-url
        ))
    } else {
        Err(Error::new(
            ErrorKind::InvalidInput,
            format!(
                "Input S3 string does not match the expected format: {}",
                s3_str
            ),
        ))
    }
}

/// **S3 Path Preparer**
///
/// Cleans and validates an input path for S3 compatibility, ensuring proper formatting.
///
/// ## Parameters
/// - **`input_path: &str`**: The raw input path to be processed.
///
/// ## Returns
/// - **`Result<(String, String), ServiceError>`**:
///   - **`clean_path`**: The sanitized path.
///   - **`clean_parent_path`**: The sanitized parent path.
///
/// ## Notes
/// - Removes redundant slashes and ensures the path ends with a `/` where appropriate.
/// - Returns an empty string for invalid or empty paths.
pub fn s3_path(input_path: &str) -> Result<(String, String), ServiceError> {
    fn s3_clean_path(input_path: &str) -> Result<String, ServiceError> {
        let re = Regex::new("//+").unwrap(); // Matches one or more '/'
        let none_redundant_path = re.replace_all(input_path, "/");
        let clean_path = if !none_redundant_path.is_empty() && none_redundant_path != "/" {
            if input_path.ends_with("/") {
                none_redundant_path.trim_start_matches("/").to_string()
            } else {
                format!("{}/", none_redundant_path.trim_start_matches("/"))
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

/// **Generate Presigned URL for S3 Object**
///
/// Creates a presigned URL that provides temporary access to an S3 object.
///
/// ## Returns
/// - **`Result<String, ServiceError>`**: The presigned URL if successful, or an error.
///
/// ## Notes
/// - The URL will be valid for the specified duration.
pub async fn s3_get_object(
    client: &Client,
    bucket: &str,
    object_key: &str,
    expires_in: u64,
) -> Result<String, ServiceError> {
    let expires_in = Duration::from_secs(expires_in);
    let presigned_request = client
        .get_object()
        .bucket(bucket)
        .key(object_key)
        .presigned(
            PresigningConfig::expires_in(expires_in)
                .map_err(|_| ServiceError::InternalServerError)?,
        )
        .await
        .map_err(|e| ServiceError::BadRequest(format!("Invalid S3 config!: {}", e)))?;

    Ok(presigned_request.uri().to_string())
}

pub async fn s3_browser(
    config: &PlayoutConfig,
    path_obj: &PathObject,
    extensions: Vec<String>,
    duration: web::Data<MediaMap>,
) -> Result<PathObject, ServiceError> {
    let mut parent_folders = vec![];
    // let mut s3_obj_dur = duration
    //     .lock()
    //     .map_err(|e| ServiceError::Conflict(format!("Invalid S3 config!: {}", e)))?;
    let s3_obj_dur = duration;
    let bucket = &config.channel.s3_storage.as_ref().unwrap().bucket;
    let path = path_obj.source.clone();
    let delimiter = '/'; // should be a single character
    let (prefix, parent_path) = s3_path(&path_obj.source)?;
    let s3_client = &config.channel.s3_storage.as_ref().unwrap().client;
    let mut obj = PathObject::new(path.clone(), Some(bucket.clone()));
    obj.folders_only = path_obj.folders_only;

    if (prefix != parent_path && !path_obj.folders_only)
        || (!prefix.is_empty() && (parent_path.is_empty()))
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
                let child = prefix.split(delimiter).nth_back(1).unwrap_or("");
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
        .map_err(|_| ServiceError::InternalServerError)?;

    let mut folders: Vec<String> = vec![];
    let mut files: Vec<String> = vec![];

    for prefix in list_resp.common_prefixes() {
        if let Some(prefix) = prefix.prefix() {
            let fldrs = prefix.split(delimiter).nth_back(1).unwrap_or(prefix);
            folders.push(fldrs.to_string());
        }
    }

    for objs in list_resp.contents() {
        if let Some(obj) = objs.key() {
            if s3_obj_extension_checker(obj, &extensions) {
                let fls = obj.strip_prefix(bucket).unwrap_or(obj); // to-do: maybe no needed!
                files.push(fls.to_string());
            }
        }
    }

    files.path_sort(natural_lexical_cmp);
    folders.path_sort(natural_lexical_cmp);

    let mut media_files = vec![];

    for file in files {
        let s3file_presigned_url = s3_get_object(
            s3_client,
            bucket,
            &file,
            S3_DEFAULT_PRESIGNEDURL_EXP as u64, // to-do: may need extract from playlist-secs
        )
        .await?;
        let name = file.strip_prefix(&prefix).unwrap_or(&file).to_string();
        if let Some(stored_dur) = s3_obj_dur.get_obj(&file) {
            let video = VideoFile {
                name,
                duration: stored_dur,
            };
            media_files.push(video);
        } else {
            match MediaProbe::new(&s3file_presigned_url).await {
                Ok(probe) => {
                    let duration = probe.format.duration.unwrap_or_default();
                    s3_obj_dur.add_obj(file, duration)?; // Store file address(key) and file duration(value) in a hashmap

                    let video = VideoFile { name, duration };
                    media_files.push(video);
                }
                Err(e) => error!("{e:?}"),
            };
        }
    }

    obj.folders = Some(folders);
    obj.files = Some(media_files);
    Ok(obj)
}

pub async fn s3_upload_multipart(
    mut payload: Multipart,
    bucket: &str,
    path: &str,
    s3_client: Client,
) -> Result<(), ServiceError> {
    let mut upload_id: Option<String> = None;
    let mut key: Option<String> = None;
    let mut completed_parts: Vec<CompletedPart> = Vec::new();
    let mut part_number = 1;
    let mut s3_upload_permit = false;

    while let Some(mut field) = payload.try_next().await.map_err(|e| e.to_string())? {
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
            .map_or_else(|| rand_string, sanitize_filename::sanitize);

        let filepath = format!("{path}{filename}");

        if upload_id.is_none() {
            let create_multipart_upload_output = s3_client
                .create_multipart_upload()
                .bucket(bucket)
                .key(&filepath)
                .send()
                .await
                .map_err(|e| format!("Failed to initiate multipart upload: {}", e))?;

            upload_id = create_multipart_upload_output
                .upload_id()
                .map(|id| id.to_string());
            key = Some(filepath.clone());
        }
        let mut f = web::block(|| std::io::Cursor::new(Vec::new()))
            .await
            .map_err(|e| e.to_string())?;
        loop {
            match field.try_next().await {
                Ok(Some(chunk)) => {
                    f = web::block(move || f.write_all(&chunk).map(|_| f))
                        .await
                        .map_err(|e| e.to_string())??;
                    s3_upload_permit = true;
                }
                Ok(None) => break,
                Err(e) => {
                    if e.to_string().contains("stream is incomplete") {
                        info!("Incomplete stream for part, continuing multipart upload: {e}");

                        tokio::fs::remove_file(filepath).await?;
                    }

                    return Err(e.into());
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
            .await
            .map_err(|e| format!("Failed to upload part: {}", e))?;

        completed_parts.push(
            CompletedPart::builder()
                .e_tag(upload_part_output.e_tag().unwrap())
                .part_number(part_number)
                .build(),
        );
        part_number += 1;
    }
    if s3_upload_permit {
        let completed_multipart_upload = CompletedMultipartUpload::builder()
            .set_parts(Some(completed_parts))
            .build();

        s3_client
            .complete_multipart_upload()
            .bucket(bucket)
            .key(key.as_ref().unwrap())
            .upload_id(upload_id.as_ref().unwrap())
            .multipart_upload(completed_multipart_upload)
            .send()
            .await
            .map_err(|e| format!("Failed to complete multipart upload: {}", e))?;
    }

    Ok(())
}

pub async fn s3_delete_prefix(
    source_path: &str,
    bucket: &str,
    s3_client: &Client,
    recursive: bool,
) -> Result<(), ServiceError> {
    let (clean_path, parent_path) = s3_path(source_path)?;
    let delimiter = '/';
    let parent_list_resp = s3_client // list of objects and prefix in parent path
        .list_objects_v2()
        .bucket(bucket)
        .prefix(&parent_path)
        .delimiter(delimiter)
        .send()
        .await
        .map_err(|e| ServiceError::BadRequest(format!("Invalid S3 config!: {}", e)))?;
    for prefix in parent_list_resp.common_prefixes() {
        // detele prefix
        if let Some(prefix) = prefix.prefix() {
            if prefix == source_path {
                if recursive {
                    // recursive deleting
                    let target_fld_list_resp = s3_client
                        .list_objects_v2()
                        .bucket(bucket)
                        .prefix(&clean_path)
                        .send()
                        .await
                        .map_err(|_| ServiceError::InternalServerError)?;
                    for objs in target_fld_list_resp.contents() {
                        if let Some(obj) = objs.key() {
                            s3_client
                                .delete_object()
                                .bucket(bucket)
                                .key(obj)
                                .send()
                                .await
                                .map_err(|_| {
                                    ServiceError::BadRequest("Source does not exists!".into())
                                })?;
                        }
                    }
                } else {
                    // non-recursive deleting
                    let target_fld_list_resp = s3_client
                        .list_objects_v2()
                        .bucket(bucket)
                        .prefix(&clean_path)
                        .delimiter(delimiter)
                        .send()
                        .await
                        .map_err(|_e| ServiceError::InternalServerError)?;
                    for objs in target_fld_list_resp.contents() {
                        if let Some(obj) = objs.key() {
                            s3_client
                                .delete_object()
                                .bucket(bucket)
                                .key(obj)
                                .send()
                                .await
                                .map_err(|_| {
                                    ServiceError::BadRequest("Source does not exists!".into())
                                })?;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

pub async fn s3_delete_object(
    source_path: &str,
    bucket: &str,
    s3_client: &Client,
    duration: web::Data<MediaMap>,
) -> Result<(), ServiceError> {
    let (clean_path, _) = s3_path(source_path)?;
    let obj_path = clean_path.rsplit_once('/').unwrap_or((&clean_path, "")).0;
    s3_client
        .delete_object()
        .bucket(bucket)
        .key(obj_path)
        .send()
        .await
        .map_err(|e| ServiceError::Conflict(format!("Failed to remove object!: {}", e)))?;

    duration.remove_obj(obj_path)?;
    Ok(())
}

pub async fn s3_copy_object(
    source_object: &str,
    destination_object: &str,
    bucket: &str,
    client: &aws_sdk_s3::Client,
) -> Result<(), ServiceError> {
    let source_key = format!("{bucket}/{source_object}");
    client
        .copy_object()
        .copy_source(&source_key)
        .bucket(bucket)
        .key(destination_object)
        .send()
        .await
        .map_err(|e| ServiceError::Conflict(format!("Failed to copy object!: {}", e)))?;
    Ok(())
}
pub async fn s3_rename_object(
    source_object: &str,
    destination_object: &str,
    bucket: &str,
    client: &aws_sdk_s3::Client,
    duration: web::Data<MediaMap>,
) -> Result<(), ServiceError> {
    s3_copy_object(source_object, destination_object, bucket, client).await?;
    s3_delete_object(source_object, bucket, client, duration).await?;
    Ok(())
}

/// **Check if Path is an S3 Prefix**
///
/// Checks if the given path corresponds to a prefix in the specified S3 bucket.
///
/// ## Returns
/// - **`Result<bool, ServiceError>`**: `true` if the path is a prefix, `false` otherwise, or an error.
///
/// ## Notes
/// - Uses S3's `list_objects_v2` to check for common prefixes matching the path.
pub async fn s3_is_prefix(
    path: &str,
    bucket: &str,
    s3_client: &Client,
) -> Result<bool, ServiceError> {
    let mut is_prefix = false;
    let (clean_path, parent_path) = s3_path(path)?;
    let delimiter = '/';
    let parent_list_resp = s3_client // list of objects and prefix in parent path
        .list_objects_v2()
        .bucket(bucket)
        .prefix(&parent_path)
        .delimiter(delimiter)
        .send()
        .await
        .map_err(|e| ServiceError::BadRequest(format!("Invalid S3 config!: {}", e)))?;
    for prefix in parent_list_resp.common_prefixes() {
        if let Some(prefix) = prefix.prefix() {
            if prefix == clean_path {
                is_prefix = true;
            }
        }
    }
    Ok(is_prefix)
}

/// **Check if Path is an S3 Object**
///
/// Verifies whether the given path corresponds to an object in the specified S3 bucket.
///
/// ## Returns
/// - **`Result<bool, ServiceError>`**: `true` if the path is an object, `false` otherwise, or an error.
///
/// ## Notes
/// - Uses S3's `head_object` to check for the existence of the object at the specified path.
/// - Returns an error if the bucket or S3 client is misconfigured, or if an unexpected error occurs.
pub async fn s3_is_object(
    path: &str,
    bucket: &str,
    s3_client: &Client,
) -> Result<bool, ServiceError> {
    let mut is_object = false;
    let (clean_path, parent_path) = s3_path(path)?;
    let delimiter = '/';
    let parent_list_resp = s3_client // list of objects and prefix in parent path
        .list_objects_v2()
        .bucket(bucket)
        .prefix(&parent_path)
        .delimiter(delimiter)
        .send()
        .await
        .map_err(|e| ServiceError::BadRequest(format!("Invalid S3 config!: {}", e)))?;
    for object in parent_list_resp.contents() {
        if let Some(prefix) = object.key() {
            if prefix == clean_path {
                is_object = true;
            }
        }
    }
    Ok(is_object)
}

pub fn s3_rename(
    source_path: &str,
    target_path: &str,
    duration: web::Data<MediaMap>,
) -> Result<MoveObject, ServiceError> {
    let source_name = source_path.rsplit('/').next().unwrap_or(source_path);
    let target_name = target_path.rsplit('/').next().unwrap_or(target_path);
    duration.update_obj(source_path, target_path)?;

    Ok(MoveObject {
        source: source_name.to_string(),
        target: target_name.to_string(),
    })
}

fn s3_obj_extension_checker(obj_name: &str, extensions: &[String]) -> bool {
    extensions.iter().any(|ext| obj_name.ends_with(ext))
}


