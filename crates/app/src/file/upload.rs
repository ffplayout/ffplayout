use std::{
    collections::HashMap,
    ops::Range,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
    time::{Duration, Instant},
};

use log::{error, info};
use serde::{Deserialize, Serialize};
use tokio::{
    fs,
    io::{AsyncSeekExt, AsyncWriteExt, SeekFrom},
    sync::Mutex,
};

use crate::utils::errors::ServiceError;

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

pub(crate) const MAX_UPLOAD_SIZE: u64 = 4096 * 1024 * 1024;
pub(crate) const MAX_CHUNK_SIZE: u64 = 10 * 1024 * 1024;
pub(crate) const MAX_UPLOAD_REQUEST_SIZE: usize = MAX_CHUNK_SIZE as usize + 64 * 1024;
const ACTIVE_UPLOAD_IDLE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const UPLOAD_PRUNE_INTERVAL: Duration = Duration::from_secs(60);
const MAX_BATCH_ID_LENGTH: usize = 128;

#[derive(Debug, Deserialize)]
pub struct UploadStatusQuery {
    #[serde(default)]
    pub path: PathBuf,
    pub file_name: String,
    pub size: u64,
    pub batch_id: String,
}

#[derive(Debug, Serialize)]
pub struct UploadStatus {
    pub received_ranges: Vec<(u64, u64)>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedUpload {
    user_id: i32,
    total_size: u64,
    ranges: Vec<(u64, u64)>,
}

#[derive(Debug)]
struct UploadState {
    batch_id: String,
    user_id: i32,
    total_size: u64,
    ranges: Vec<Range<u64>>,
    finalizing: bool,
    last_activity: Instant,
}

#[derive(Clone, Debug)]
pub(super) struct Upload {
    state: Arc<Mutex<UploadState>>,
    pub temp_file: PathBuf,
    metadata_file: PathBuf,
}

type UploadMap = HashMap<PathBuf, Upload>;

static UPLOADS: LazyLock<Mutex<UploadMap>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static UPLOAD_CREATION: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
static LAST_UPLOAD_PRUNE: LazyLock<Mutex<Instant>> = LazyLock::new(|| Mutex::new(Instant::now()));

pub(super) fn sanitize_upload_filename(filename: &str) -> Result<String, ServiceError> {
    let sanitized = sanitize_filename::sanitize(filename);
    if sanitized.is_empty() || sanitized == "." || sanitized == ".." {
        return Err(ServiceError::BadRequest("Invalid filename".to_string()));
    }

    validate_mime_type(&sanitized)?;
    Ok(sanitized)
}

fn validate_mime_type(filename: &str) -> Result<(), ServiceError> {
    let mime_type = mime_guess::from_path(filename)
        .first_or_octet_stream()
        .to_string();

    if ALLOWED_MIME_TYPES.contains(&mime_type.as_str()) {
        Ok(())
    } else {
        Err(ServiceError::BadRequest(format!(
            "File type '{mime_type}' is not allowed."
        )))
    }
}

pub(super) fn validate_upload_metadata(
    total_size: u64,
    batch_id: &str,
) -> Result<(), ServiceError> {
    if total_size == 0 {
        return Err(ServiceError::BadRequest(
            "Empty files cannot be uploaded".to_string(),
        ));
    }
    if total_size > MAX_UPLOAD_SIZE {
        return Err(ServiceError::BadRequest(format!(
            "File size exceeds maximum allowed size of {MAX_UPLOAD_SIZE} bytes"
        )));
    }
    if batch_id.trim().is_empty() || batch_id.len() > MAX_BATCH_ID_LENGTH {
        return Err(ServiceError::BadRequest("Invalid batch id".to_string()));
    }

    Ok(())
}

pub(super) fn validate_chunk(
    start: u64,
    end: u64,
    total_size: u64,
    chunk_size: usize,
) -> Result<(), ServiceError> {
    if chunk_size as u64 > MAX_CHUNK_SIZE {
        return Err(ServiceError::BadRequest(format!(
            "Chunk size exceeds maximum allowed chunk size of {MAX_CHUNK_SIZE} bytes"
        )));
    }
    if end <= start || chunk_size as u64 != end - start || end > total_size {
        return Err(ServiceError::BadRequest("Invalid chunk range".to_string()));
    }

    Ok(())
}

fn merge_ranges(ranges: &mut Vec<Range<u64>>) {
    if ranges.is_empty() {
        return;
    }

    ranges.sort_by_key(|range| range.start);
    let mut merged = Vec::with_capacity(ranges.len());
    merged.push(ranges[0].clone());

    for range in ranges.iter().skip(1) {
        let last = merged.last_mut().expect("merged ranges cannot be empty");
        if last.end >= range.start {
            last.end = last.end.max(range.end);
        } else {
            merged.push(range.clone());
        }
    }

    *ranges = merged;
}

fn is_upload_complete(ranges: &[Range<u64>], total_size: u64) -> bool {
    if ranges.is_empty() {
        return false;
    }

    let mut position = 0;
    for range in ranges {
        if range.start != position {
            return false;
        }
        position = range.end;
    }

    position == total_size
}

fn append_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

fn uploading_path(output_file: &Path) -> PathBuf {
    append_suffix(output_file, ".uploading")
}

fn metadata_path(temp_file: &Path) -> PathBuf {
    append_suffix(temp_file, ".json")
}

async fn persist_upload(upload: &Upload, state: &UploadState) -> Result<(), ServiceError> {
    let persisted = PersistedUpload {
        user_id: state.user_id,
        total_size: state.total_size,
        ranges: state
            .ranges
            .iter()
            .map(|range| (range.start, range.end))
            .collect(),
    };
    let data = serde_json::to_vec(&persisted)?;
    let temporary_metadata = append_suffix(&upload.metadata_file, ".tmp");

    fs::write(&temporary_metadata, data).await?;
    fs::rename(&temporary_metadata, &upload.metadata_file).await?;

    Ok(())
}

async fn active_upload(
    output_file: &Path,
    batch_id: &str,
    user_id: i32,
    total_size: u64,
) -> Result<Option<Upload>, ServiceError> {
    let upload = UPLOADS.lock().await.get(output_file).cloned();
    let Some(upload) = upload else {
        return Ok(None);
    };

    let mut state = upload.state.lock().await;
    if state.user_id != user_id || state.total_size != total_size {
        return Err(ServiceError::Conflict(
            "Upload metadata does not match the existing upload".to_string(),
        ));
    }
    if state.batch_id != batch_id {
        if state.ranges.is_empty() && !state.finalizing {
            state.batch_id = batch_id.to_string();
        } else {
            return Err(ServiceError::Conflict(
                "Another upload is already writing this file".to_string(),
            ));
        }
    }
    state.last_activity = Instant::now();
    drop(state);

    Ok(Some(upload))
}

async fn prune_inactive_uploads() {
    let now = Instant::now();
    {
        let mut last_prune = LAST_UPLOAD_PRUNE.lock().await;
        if now.saturating_duration_since(*last_prune) < UPLOAD_PRUNE_INTERVAL {
            return;
        }
        *last_prune = now;
    }

    let uploads = UPLOADS
        .lock()
        .await
        .iter()
        .map(|(path, upload)| (path.clone(), upload.clone()))
        .collect::<Vec<_>>();
    for (path, upload) in uploads {
        let is_inactive = {
            let state = upload.state.lock().await;
            !state.finalizing
                && now.saturating_duration_since(state.last_activity) >= ACTIVE_UPLOAD_IDLE_TIMEOUT
        };

        if is_inactive {
            remove_tracked_upload(&path, &upload).await;
        }
    }
}

fn restored_state(
    persisted: PersistedUpload,
    batch_id: &str,
    user_id: i32,
    total_size: u64,
) -> Result<UploadState, ServiceError> {
    if persisted.user_id != user_id || persisted.total_size != total_size {
        return Err(ServiceError::Conflict(
            "An incompatible incomplete upload already exists".to_string(),
        ));
    }

    let mut ranges = persisted
        .ranges
        .into_iter()
        .map(|(start, end)| start..end)
        .collect::<Vec<_>>();
    if ranges
        .iter()
        .any(|range| range.start >= range.end || range.end > total_size)
    {
        return Err(ServiceError::Conflict(
            "Stored upload ranges are invalid".to_string(),
        ));
    }
    merge_ranges(&mut ranges);

    Ok(UploadState {
        batch_id: batch_id.to_string(),
        user_id,
        total_size,
        ranges,
        finalizing: false,
        last_activity: Instant::now(),
    })
}

pub(super) async fn get_or_create_upload(
    total_size: u64,
    output_file: &Path,
    batch_id: &str,
    user_id: i32,
) -> Result<Upload, ServiceError> {
    prune_inactive_uploads().await;
    if let Some(upload) = active_upload(output_file, batch_id, user_id, total_size).await? {
        return Ok(upload);
    }

    let _creation_guard = UPLOAD_CREATION.lock().await;
    if let Some(upload) = active_upload(output_file, batch_id, user_id, total_size).await? {
        return Ok(upload);
    }
    if fs::try_exists(output_file).await? {
        return Err(ServiceError::Conflict(format!(
            "File '{}' already exists",
            output_file.display()
        )));
    }

    let temp_file = uploading_path(output_file);
    let metadata_file = metadata_path(&temp_file);
    let state = if fs::try_exists(&metadata_file).await? {
        let data = fs::read(&metadata_file).await?;
        let persisted = serde_json::from_slice::<PersistedUpload>(&data).map_err(|error| {
            ServiceError::Conflict(format!("Invalid upload resume metadata: {error}"))
        })?;
        let state = restored_state(persisted, batch_id, user_id, total_size)?;

        if let Some(last_end) = state.ranges.iter().map(|range| range.end).max() {
            let temp_size = fs::metadata(&temp_file)
                .await
                .map(|metadata| metadata.len());
            if temp_size.is_err() || temp_size.is_ok_and(|size| size < last_end) {
                return Err(ServiceError::Conflict(
                    "Incomplete upload data does not match its resume metadata".to_string(),
                ));
            }
        }

        state
    } else {
        if fs::try_exists(&temp_file).await? {
            return Err(ServiceError::Conflict(format!(
                "Incomplete upload '{}' has no resume metadata",
                temp_file.display()
            )));
        }

        UploadState {
            batch_id: batch_id.to_string(),
            user_id,
            total_size,
            ranges: Vec::new(),
            finalizing: false,
            last_activity: Instant::now(),
        }
    };

    let upload = Upload {
        state: Arc::new(Mutex::new(state)),
        temp_file,
        metadata_file,
    };
    UPLOADS
        .lock()
        .await
        .insert(output_file.to_path_buf(), upload.clone());
    info!("Start or resume uploading: {output_file:?}");

    Ok(upload)
}

pub(super) async fn write_upload_chunk(
    upload: &Upload,
    start: u64,
    end: u64,
    chunk_data: &[u8],
) -> Result<bool, ServiceError> {
    let mut state = upload.state.lock().await;
    state.last_activity = Instant::now();

    if state.finalizing {
        return Ok(false);
    }

    let already_written = state
        .ranges
        .iter()
        .any(|range| range.start <= start && range.end >= end);
    if !already_written {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&upload.temp_file)
            .await?;
        file.seek(SeekFrom::Start(start)).await?;
        file.write_all(chunk_data).await?;
        file.flush().await?;
        file.sync_data().await?;

        state.ranges.push(start..end);
        merge_ranges(&mut state.ranges);
        persist_upload(upload, &state).await?;
    }

    if is_upload_complete(&state.ranges, state.total_size) {
        state.finalizing = true;
        return Ok(true);
    }

    Ok(false)
}

pub(super) async fn received_ranges(upload: &Upload) -> Vec<(u64, u64)> {
    let mut state = upload.state.lock().await;
    state.last_activity = Instant::now();

    if is_upload_complete(&state.ranges, state.total_size) && !state.finalizing {
        return Vec::new();
    }

    state
        .ranges
        .iter()
        .map(|range| (range.start, range.end))
        .collect()
}

async fn remove_tracked_upload(output_file: &Path, upload: &Upload) {
    let mut uploads = UPLOADS.lock().await;
    if uploads
        .get(output_file)
        .is_some_and(|current| Arc::ptr_eq(&current.state, &upload.state))
    {
        uploads.remove(output_file);
    }
}

pub(super) async fn finalize_upload(
    output_file: &Path,
    upload: &Upload,
) -> Result<(), ServiceError> {
    if fs::try_exists(output_file).await? {
        let mut state = upload.state.lock().await;
        state.finalizing = false;
        return Err(ServiceError::Conflict(format!(
            "File '{}' already exists",
            output_file.display()
        )));
    }
    if let Err(error) = fs::rename(&upload.temp_file, output_file).await {
        let mut state = upload.state.lock().await;
        state.finalizing = false;
        return Err(error.into());
    }

    remove_tracked_upload(output_file, upload).await;
    if let Err(error) = fs::remove_file(&upload.metadata_file).await
        && error.kind() != std::io::ErrorKind::NotFound
    {
        error!("Failed to remove upload metadata: {error}");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_directory() -> PathBuf {
        std::env::temp_dir().join(format!(
            "ffplayout-upload-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ))
    }

    #[test]
    fn merges_overlapping_adjacent_and_contained_ranges() {
        let mut ranges = vec![8..10, 0..4, 3..6, 6..8, 2..3, 12..14];

        merge_ranges(&mut ranges);

        assert_eq!(ranges, vec![0..10, 12..14]);
        assert!(!is_upload_complete(&ranges, 14));
        assert!(is_upload_complete(&[0..7, 7..14], 14));
    }

    #[test]
    fn rejects_invalid_upload_metadata_and_ranges() {
        assert!(validate_upload_metadata(0, "batch").is_err());
        assert!(validate_upload_metadata(1, "").is_err());
        assert!(validate_chunk(3, 3, 6, 0).is_err());
        assert!(validate_chunk(0, 4, 6, 3).is_err());
        assert!(validate_chunk(4, 7, 6, 3).is_err());
        assert!(sanitize_upload_filename("/").is_err());
        assert!(sanitize_upload_filename("video.exe").is_err());
    }

    #[test]
    fn creates_sidecar_paths_without_replacing_the_extension() {
        let output = Path::new("/media/example.mp4");
        let temp = uploading_path(output);

        assert_eq!(temp, Path::new("/media/example.mp4.uploading"));
        assert_eq!(
            metadata_path(&temp),
            Path::new("/media/example.mp4.uploading.json")
        );
    }

    #[tokio::test]
    async fn restores_ranges_and_atomically_finalizes_upload() {
        let directory = test_directory();
        fs::create_dir_all(&directory).await.unwrap();
        let output = directory.join("video.mp4");

        let upload = get_or_create_upload(6, &output, "batch-a", 7)
            .await
            .unwrap();
        assert!(!write_upload_chunk(&upload, 3, 6, b"def").await.unwrap());
        assert!(!fs::try_exists(&output).await.unwrap());
        assert!(fs::try_exists(&upload.temp_file).await.unwrap());

        remove_tracked_upload(&output, &upload).await;
        let restored = get_or_create_upload(6, &output, "batch-b", 7)
            .await
            .unwrap();
        assert_eq!(received_ranges(&restored).await, vec![(3, 6)]);
        assert!(write_upload_chunk(&restored, 0, 3, b"abc").await.unwrap());

        finalize_upload(&output, &restored).await.unwrap();
        assert_eq!(fs::read(&output).await.unwrap(), b"abcdef");
        assert!(!fs::try_exists(&restored.metadata_file).await.unwrap());

        fs::remove_dir_all(directory).await.unwrap();
    }

    #[tokio::test]
    async fn status_only_upload_does_not_create_sidecar_files() {
        let directory = test_directory();
        fs::create_dir_all(&directory).await.unwrap();
        let output = directory.join("video.mp4");

        let upload = get_or_create_upload(6, &output, "batch-a", 7)
            .await
            .unwrap();
        assert!(!fs::try_exists(&upload.temp_file).await.unwrap());
        assert!(!fs::try_exists(&upload.metadata_file).await.unwrap());

        remove_tracked_upload(&output, &upload).await;
        fs::remove_dir_all(directory).await.unwrap();
    }

    #[tokio::test]
    async fn finalizing_does_not_replace_an_existing_file() {
        let directory = test_directory();
        fs::create_dir_all(&directory).await.unwrap();
        let output = directory.join("video.mp4");

        let upload = get_or_create_upload(3, &output, "batch-a", 7)
            .await
            .unwrap();
        assert!(write_upload_chunk(&upload, 0, 3, b"new").await.unwrap());
        fs::write(&output, b"existing").await.unwrap();

        assert!(matches!(
            finalize_upload(&output, &upload).await,
            Err(ServiceError::Conflict(_))
        ));
        assert_eq!(fs::read(&output).await.unwrap(), b"existing");
        assert_eq!(fs::read(&upload.temp_file).await.unwrap(), b"new");

        remove_tracked_upload(&output, &upload).await;
        fs::remove_dir_all(directory).await.unwrap();
    }

    #[tokio::test]
    async fn rejects_competing_or_incompatible_uploads() {
        let directory = test_directory();
        fs::create_dir_all(&directory).await.unwrap();
        let output = directory.join("video.mp4");

        let upload = get_or_create_upload(6, &output, "batch-a", 7)
            .await
            .unwrap();
        write_upload_chunk(&upload, 0, 3, b"abc").await.unwrap();
        assert!(matches!(
            get_or_create_upload(6, &output, "batch-b", 7).await,
            Err(ServiceError::Conflict(_))
        ));

        remove_tracked_upload(&output, &upload).await;
        assert!(matches!(
            get_or_create_upload(6, &output, "batch-b", 8).await,
            Err(ServiceError::Conflict(_))
        ));

        fs::remove_file(&upload.temp_file).await.unwrap();
        fs::remove_file(&upload.metadata_file).await.unwrap();
        fs::remove_dir_all(directory).await.unwrap();
    }
}
