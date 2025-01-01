use actix_multipart::Multipart;
use actix_web::web;
use futures_util::TryStreamExt as _;
use rand::{distributions::Alphanumeric, Rng};
use regex::Regex;
use std::io::Write;
use std::{
    io::{Error, ErrorKind},
    time::Duration,
};

use log::*;

use aws_sdk_s3::{
    presigning::PresigningConfig,
    types::{CompletedMultipartUpload, CompletedPart},
    Client,
};

use super::errors::ServiceError;

pub const S3_INDICATOR: &str = "s3://";

pub fn s3_parse_string(
    // to_do: maybe should change the snippet to get better understanding
    s3_str: &str,
) -> Result<(aws_sdk_s3::config::Credentials, String, String), std::io::Error> {
    // Define the regex pattern for /: delimiter
    // The pattern : s3://{bucket_name}/:{endpoint_url}/:{access_key}/:{secret_key}
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
pub async fn s3_get_object(
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
                .map_err(|_| ServiceError::InternalServerError)?,
        )
        .await
        .map_err(|_| ServiceError::InternalServerError)?;

    Ok(presigned_request.uri().to_string())
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

                        tokio::fs::remove_file(filepath).await?
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
