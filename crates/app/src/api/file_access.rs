use std::{
    collections::HashSet,
    path::Path,
    sync::Arc,
    time::{Duration, SystemTime},
};

use serde::Serialize;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::utils::errors::ServiceError;

const FILE_ACCESS_TTL: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Clone, Eq, Hash, PartialEq)]
pub struct FileAccessData {
    uuid: Uuid,
    expiration: SystemTime,
    ip_address: String,
    user_id: i32,
    channel_id: i32,
    path: String,
}

impl FileAccessData {
    pub fn new(ip_address: String, user_id: i32, channel_id: i32, path: &Path) -> Self {
        Self {
            uuid: Uuid::new_v4(),
            expiration: SystemTime::now() + FILE_ACCESS_TTL,
            ip_address,
            user_id,
            channel_id,
            path: normalize_path(path),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct FileAccessState {
    pub tokens: Arc<Mutex<HashSet<FileAccessData>>>,
}

#[derive(Debug, Serialize)]
pub struct FileAccessResponse {
    pub access: String,
    pub expires_in_seconds: u64,
}

impl From<&FileAccessData> for FileAccessResponse {
    fn from(data: &FileAccessData) -> Self {
        Self {
            access: data.uuid.to_string(),
            expires_in_seconds: FILE_ACCESS_TTL.as_secs(),
        }
    }
}

pub fn prune_file_access(tokens: &mut HashSet<FileAccessData>) {
    tokens.retain(|entry| entry.expiration > SystemTime::now());
}

pub fn check_file_access(
    tokens: &mut HashSet<FileAccessData>,
    access: &str,
    ip_address: &str,
    channel_id: i32,
    path: &Path,
) -> Result<(), ServiceError> {
    let access_uuid = Uuid::parse_str(access)
        .map_err(|_| ServiceError::Forbidden("Invalid file access token".to_string()))?;
    let requested_path = normalize_path(path);

    prune_file_access(tokens);

    match tokens.iter().find(|entry| entry.uuid == access_uuid) {
        Some(entry) => {
            if entry.ip_address != ip_address {
                return Err(ServiceError::Forbidden(
                    "File access token IP address mismatch".to_string(),
                ));
            }

            if entry.channel_id != channel_id || entry.path != requested_path {
                return Err(ServiceError::Forbidden(
                    "File access token does not match requested file".to_string(),
                ));
            }

            Ok(())
        }
        None => Err(ServiceError::Forbidden(
            "Invalid or expired file access token".to_string(),
        )),
    }
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, path::Path, time::Duration};

    use super::{FileAccessData, check_file_access};

    #[test]
    fn file_access_accepts_matching_request() {
        let token = FileAccessData::new("127.0.0.1".to_string(), 1, 1, Path::new("/tmp/a.mp4"));
        let access = token.uuid.to_string();
        let mut tokens = HashSet::from([token]);

        assert!(
            check_file_access(
                &mut tokens,
                &access,
                "127.0.0.1",
                1,
                Path::new("/tmp/a.mp4")
            )
            .is_ok()
        );
    }

    #[test]
    fn file_access_rejects_different_file() {
        let token = FileAccessData::new("127.0.0.1".to_string(), 1, 1, Path::new("/tmp/a.mp4"));
        let access = token.uuid.to_string();
        let mut tokens = HashSet::from([token]);

        assert!(
            check_file_access(
                &mut tokens,
                &access,
                "127.0.0.1",
                1,
                Path::new("/tmp/b.mp4")
            )
            .is_err()
        );
    }

    #[test]
    fn file_access_rejects_different_ip() {
        let token = FileAccessData::new("127.0.0.1".to_string(), 1, 1, Path::new("/tmp/a.mp4"));
        let access = token.uuid.to_string();
        let mut tokens = HashSet::from([token]);

        assert!(
            check_file_access(
                &mut tokens,
                &access,
                "127.0.0.2",
                1,
                Path::new("/tmp/a.mp4")
            )
            .is_err()
        );
    }

    #[test]
    fn file_access_rejects_expired_token() {
        let mut token = FileAccessData::new("127.0.0.1".to_string(), 1, 1, Path::new("/tmp/a.mp4"));
        token.expiration = std::time::SystemTime::now() - Duration::from_secs(1);
        let access = token.uuid.to_string();
        let mut tokens = HashSet::from([token]);

        assert!(
            check_file_access(
                &mut tokens,
                &access,
                "127.0.0.1",
                1,
                Path::new("/tmp/a.mp4")
            )
            .is_err()
        );
        assert!(tokens.is_empty());
    }
}
