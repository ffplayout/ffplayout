use std::{
    collections::HashMap,
    ops::Range,
    path::Path,
    sync::{Arc, LazyLock},
};

use log::{info, warn};
use tokio::{
    fs::{self},
    sync::Mutex,
};

use crate::utils::errors::ServiceError;

// Allowed MIME types for uploads
const ALLOWED_MIME_TYPES: &[&str] = &[
    "audio/aac",
    "audio/flac",
    "audio/mp4",
    "audio/mpeg",
    "audio/ogg",
    "audio/wav",
    "audio/webm",
    "image/avif",
    "image/gif",
    "image/heic",
    "image/heif",
    "image/jpeg",
    "image/jpg",
    "image/png",
    "image/svg+xml",
    "image/webp",
    "text/csv",
    "text/plain",
    "text/xml",
    "video/mp4",
    "video/ogg",
    "video/quicktime",
    "video/webm",
];
pub const MAX_UPLOAD_SIZE: u64 = 4096 * 1024 * 1024; // 4 GB
pub const MAX_CHUNK_SIZE: u64 = 10 * 1024 * 1024; // 10MB

/// Tracks byte ranges for resumable uploads
pub type UploadMap = HashMap<String, Upload>;

/// Global upload map, protected by a Mutex
pub static UPLOADS: LazyLock<Mutex<UploadMap>> = LazyLock::new(|| Mutex::new(HashMap::new()));

// Metadata for a single file upload
#[derive(Clone, Default)]
pub struct Meta {
    pub db_id: Option<i32>,
    pub mime_type: Option<String>,
}

#[derive(Clone)]
pub struct Upload {
    pub batch_id: String,
    pub ranges: Arc<Mutex<Vec<Range<u64>>>>,
    pub meta: Arc<Mutex<Meta>>,
}

impl Upload {
    pub fn new(batch_id: String, meta: Arc<Mutex<Meta>>) -> Self {
        Self {
            batch_id,
            ranges: Arc::new(Mutex::new(Vec::new())),
            meta,
        }
    }
}

pub fn validate_mime_type(filename: &str) -> Result<String, ServiceError> {
    let mime_type = mime_guess::from_path(filename)
        .first_or_octet_stream()
        .to_string();

    if ALLOWED_MIME_TYPES.contains(&mime_type.as_str()) {
        Ok(mime_type)
    } else {
        Err(ServiceError::BadRequest(format!(
            "File type '{}' is not allowed.",
            mime_type
        )))
    }
}

pub fn merge_ranges(ranges: &mut Vec<Range<u64>>) {
    if ranges.is_empty() {
        return;
    }

    ranges.sort_by_key(|r| r.start);
    let mut merged = vec![ranges[0].clone()];

    for r in ranges.iter().skip(1) {
        let last = merged.last_mut().unwrap();
        if last.end >= r.start {
            last.end = last.end.max(r.end); // merge overlapping or adjacent ranges
        } else {
            merged.push(r.clone());
        }
    }

    *ranges = merged;
}

/// Get or create UploadValue for a file
pub async fn file_ranges(
    start: u64,
    total_size: u64,
    file_name: &str,
    output_file: &Path,
    batch_id: &str,
    meta: Arc<Mutex<Meta>>,
) -> Result<Upload, ServiceError> {
    let upload_key = output_file.to_string_lossy().to_string();
    let mut uploads = UPLOADS.lock().await;

    // Prevent overwriting if file already exists and is not being tracked
    if total_size > 0
        && fs::metadata(&output_file)
            .await
            .is_ok_and(|f| f.len() == total_size)
        && !uploads.contains_key(&upload_key)
    {
        return Err(ServiceError::Conflict(format!(
            "File {file_name:?} is currently being uploaded!"
        )));
    }

    // Remove old tracking if start == 0 and file has no active ranges
    if start == 0 {
        if let Some(upload) = uploads.get(&upload_key) {
            let is_empty = {
                let guard = upload.ranges.lock().await;
                guard.is_empty()
            };

            if is_empty {
                uploads.remove(&upload_key);
                warn!("Removed old upload history for {file_name:?}");
            }
        }

        info!("Start uploading: {output_file:?}");
    }

    let upload_entry = uploads
        .entry(upload_key.clone())
        .or_insert_with(|| Upload::new(batch_id.to_string(), meta.clone()));

    let result = Upload {
        batch_id: upload_entry.batch_id.clone(),
        ranges: upload_entry.ranges.clone(),
        meta: upload_entry.meta.clone(),
    };

    drop(uploads);
    Ok(result)
}

/// Remove all uploads of a batch
pub async fn cleanup_uploads(batch_id: &str) {
    let mut uploads = UPLOADS.lock().await;
    uploads.retain(|_, upload| upload.batch_id != batch_id);
}

/// Check if upload is complete
pub fn is_upload_complete(ranges: &[Range<u64>], total_size: u64) -> bool {
    if ranges.is_empty() {
        return false;
    }

    let mut pos = 0;
    for r in ranges {
        if r.start != pos {
            return false; // gap detected
        }
        pos = r.end;
    }

    pos == total_size
}

/// Check if a batch of uploads is complete
pub fn is_batch_complete(upload_map: &UploadMap, batch_id: &str, batch_count: usize) -> bool {
    upload_map
        .values()
        .filter(|upload| upload.batch_id == batch_id)
        .count()
        == batch_count
}
