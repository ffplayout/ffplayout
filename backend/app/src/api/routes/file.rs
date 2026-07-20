use std::{env, io};

use axum::{
    Json,
    extract::{Multipart, Path, Query, State},
    http::{HeaderMap, StatusCode, header::AUTHORIZATION},
    response::Response,
};
use protect_axum::authorities::AuthDetails;
use real::RealIp;
use serde::Deserialize;
use tokio::fs;

use crate::{
    api::{
        auth::decode_jwt,
        file_access::{FileAccessData, FileAccessResponse, check_file_access, prune_file_access},
        routes::{AuthUser, FileObj, ImportObj, ensure_any_authority, stream_file},
        state::AppState,
    },
    db::models::Role,
    file::{MoveObject, PathObject, UploadStatus, UploadStatusQuery, norm_abs_path},
    player::utils::import::import_file,
    utils::errors::ServiceError,
};

#[derive(Debug, Deserialize)]
pub struct FileQuery {
    #[serde(default)]
    access: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct FileAccessRequest {
    filename: String,
}

/// ### File Operations
///
/// **Get File/Folder List**
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/file/1/browse -H 'Content-Type: application/json'
/// -d '{ "source": "/" }' -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn file_browser(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
    Json(data): Json<PathObject>,
) -> Result<Json<PathObject>, ServiceError> {
    ensure_any_authority(
        &details,
        &[&Role::GlobalAdmin, &Role::ChannelAdmin, &Role::User],
    )?;
    user.ensure_channel_or_admin(id)?;

    let manager = {
        let guard = state.controller.read().await;
        guard.get(id)
    }
    .ok_or_else(|| ServiceError::BadRequest(format!("Channel {id} not found!")))?;

    match manager.storage.browser(&data).await {
        Ok(obj) => Ok(Json(obj)),
        Err(e) => Err(e),
    }
}

/// **Create Folder**
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/file/1/create-folder -H 'Content-Type: application/json'
/// -d '{"source": "<FOLDER PATH>"}' -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn add_dir(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
    Json(data): Json<PathObject>,
) -> Result<StatusCode, ServiceError> {
    ensure_any_authority(
        &details,
        &[&Role::GlobalAdmin, &Role::ChannelAdmin, &Role::User],
    )?;
    user.ensure_channel_or_admin(id)?;

    let manager = {
        let guard = state.controller.read().await;
        guard.get(id)
    }
    .ok_or_else(|| ServiceError::BadRequest(format!("Channel {id} not found!")))?;

    manager.storage.mkdir(&data).await?;

    Ok(StatusCode::OK)
}

/// **Rename File**
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/file/1/rename -H 'Content-Type: application/json'
/// -d '{"source": "<SOURCE>", "target": "<TARGET>"}' -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn move_rename(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
    Json(data): Json<MoveObject>,
) -> Result<Json<MoveObject>, ServiceError> {
    ensure_any_authority(
        &details,
        &[&Role::GlobalAdmin, &Role::ChannelAdmin, &Role::User],
    )?;
    user.ensure_channel_or_admin(id)?;

    let manager = {
        let guard = state.controller.read().await;
        guard.get(id)
    }
    .ok_or_else(|| ServiceError::BadRequest(format!("Channel {id} not found!")))?;

    match manager.storage.rename(&data).await {
        Ok(obj) => Ok(Json(obj)),
        Err(e) => Err(e),
    }
}

/// **Remove File/Folder**
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/file/1/remove -H 'Content-Type: application/json'
/// -d '{"source": "<SOURCE>"}' -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn remove(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
    Json(data): Json<PathObject>,
) -> Result<StatusCode, ServiceError> {
    ensure_any_authority(
        &details,
        &[&Role::GlobalAdmin, &Role::ChannelAdmin, &Role::User],
    )?;
    user.ensure_channel_or_admin(id)?;

    let manager = {
        let guard = state.controller.read().await;
        guard.get(id)
    }
    .ok_or_else(|| ServiceError::BadRequest(format!("Channel {id} not found!")))?;

    let recursive = data.recursive;

    match manager.storage.remove(&data.source, recursive).await {
        Ok(_) => Ok(StatusCode::OK),
        Err(e) => Err(e),
    }
}

/// **Upload File**
///
/// ```BASH
/// curl -X PUT 'http://127.0.0.1:8787/api/file/1/upload?path=folder' \
/// -H 'Authorization: Bearer <TOKEN>' -F 'fileName=file.mp4' -F 'start=0' \
/// -F 'end=1024' -F 'size=1024' -F 'batch_id=<ID>' \
/// -F 'chunk=@chunk.bin;type=application/octet-stream'
/// ```
pub async fn upload_status(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
    Query(query): Query<UploadStatusQuery>,
) -> Result<Json<UploadStatus>, ServiceError> {
    ensure_any_authority(
        &details,
        &[&Role::GlobalAdmin, &Role::ChannelAdmin, &Role::User],
    )?;
    user.ensure_channel_or_admin(id)?;

    let manager = {
        let guard = state.controller.read().await;
        guard.get(id)
    }
    .ok_or_else(|| ServiceError::BadRequest(format!("Channel {id} not found!")))?;

    Ok(Json(manager.storage.upload_status(&query, user.id).await?))
}

pub async fn upload_file(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Query(obj): Query<FileObj>,
    user: AuthUser,
    details: AuthDetails<Role>,
    payload: Multipart,
) -> Result<StatusCode, ServiceError> {
    ensure_any_authority(
        &details,
        &[&Role::GlobalAdmin, &Role::ChannelAdmin, &Role::User],
    )?;
    user.ensure_channel_or_admin(id)?;

    let manager = {
        let guard = state.controller.read().await;
        guard.get(id)
    }
    .ok_or_else(|| ServiceError::BadRequest(format!("Channel {id} not found!")))?;

    manager.storage.upload(payload, &obj.path, user.id).await?;

    Ok(StatusCode::OK)
}

pub async fn create_file_access_token(
    real_ip: RealIp,
    State(state): State<AppState>,
    Path(id): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
    Json(request): Json<FileAccessRequest>,
) -> Result<Json<FileAccessResponse>, ServiceError> {
    ensure_any_authority(
        &details,
        &[&Role::GlobalAdmin, &Role::ChannelAdmin, &Role::User],
    )?;
    user.ensure_channel_or_admin(id)?;

    let manager = {
        let guard = state.controller.read().await;
        guard.get(id)
    }
    .ok_or_else(|| ServiceError::BadRequest(format!("Channel {id} not found!")))?;

    let config = manager.config.read().await;
    let storage = config.channel.storage.clone();
    let (path, _, _) = norm_abs_path(&storage, &request.filename)?;
    drop(config);

    let metadata = match fs::metadata(&path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(ServiceError::NotFound("File not found".to_string()));
        }
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_file() {
        return Err(ServiceError::NotFound("File not found".to_string()));
    }

    let access_data = FileAccessData::new(real_ip.ip().to_string(), user.id, id, &path);
    let response = FileAccessResponse::from(&access_data);
    let mut tokens = state.file_access.tokens.lock().await;
    prune_file_access(&mut tokens);
    tokens.insert(access_data);

    Ok(Json(response))
}

/// **Get File**
///
/// Can be used for preview video files
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/file/1/path/to/file.mp4
/// ```
pub async fn get_file(
    real_ip: RealIp,
    State(state): State<AppState>,
    Path((id, filename)): Path<(i32, String)>,
    Query(query): Query<FileQuery>,
    headers: HeaderMap,
) -> Result<Response, ServiceError> {
    let manager = {
        let guard = state.controller.read().await;
        guard.get(id)
    }
    .ok_or_else(|| ServiceError::BadRequest(format!("Channel {id} not found!")))?;

    let config = manager.config.read().await;
    let storage = config.channel.storage.clone();
    let (path, _, _) = norm_abs_path(&storage, &filename)?;
    drop(config);

    if let Some(token) = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
    {
        let claims = decode_jwt(token).await?;
        let auth = AuthUser {
            id: claims.id,
            channels: claims.channels,
            role: claims.role,
        };
        auth.ensure_channel_or_admin(id)?;
    } else if let Some(access) = query.access.as_deref() {
        let mut tokens = state.file_access.tokens.lock().await;
        check_file_access(&mut tokens, access, &real_ip.ip().to_string(), id, &path)?;
    } else {
        return Err(ServiceError::Unauthorized("Missing token".to_string()));
    }

    stream_file(&path, &headers).await
}

/// **Import playlist**
///
/// Import text/m3u file and convert it to a playlist
/// lines with leading "#" will be ignore
///
/// ```BASH
/// curl -X PUT http://127.0.0.1:8787/api/file/1/import -H 'Authorization: Bearer <TOKEN>'
/// -F "file=@list.m3u"
/// ```
pub async fn import_playlist(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Query(obj): Query<ImportObj>,
    user: AuthUser,
    details: AuthDetails<Role>,
    mut payload: Multipart,
) -> Result<Json<String>, ServiceError> {
    ensure_any_authority(
        &details,
        &[&Role::GlobalAdmin, &Role::ChannelAdmin, &Role::User],
    )?;
    user.ensure_channel_or_admin(id)?;

    let manager = {
        let guard = state.controller.read().await;
        guard.get(id)
    }
    .ok_or_else(|| ServiceError::BadRequest(format!("Channel {id} not found!")))?;

    let channel_name = manager.channel.lock().await.name.clone();
    let playlists = manager.config.read().await.channel.playlists.clone();
    let file = obj.file.file_name().unwrap_or_default();
    let path = env::temp_dir().join(file);
    let path_clone = path.clone();

    if let Some(field) = payload.next_field().await? {
        let bytes = field.bytes().await?;
        fs::write(&path, &bytes).await?;
    } else {
        return Err(ServiceError::BadRequest("No file provided".into()));
    }

    let response = import_file(&playlists, &obj.date, Some(channel_name), &path_clone).await?;

    fs::remove_file(path).await?;

    Ok(Json(response))
}
