use axum::{
    body::Body,
    extract::{Path, State},
    http::{
        HeaderMap, StatusCode,
        header::{CONTENT_DISPOSITION, CONTENT_TYPE},
    },
    response::{IntoResponse, Response},
};
use path_clean::PathClean;
use tokio::fs;

use crate::{
    api::state::AppState,
    utils::{errors::ServiceError, public_path},
};

/// **Get Public**
///
/// Can be used for HLS Playlist and other static files in public folder
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/1/live/stream.m3u8
/// ```
pub async fn get_public(
    State(state): State<AppState>,
    Path((id, public, file_stem)): Path<(i32, String, String)>,
) -> Result<Response, ServiceError> {
    let absolute_path = if file_stem.ends_with(".ts")
        || file_stem.ends_with(".m3u8")
        || file_stem.ends_with(".vtt")
    {
        let manager = {
            let guard = state.controller.read().await;
            guard.get(id)
        }
        .ok_or_else(|| ServiceError::BadRequest(format!("Channel {id} not found!")))?;

        let config = manager.config.read().await;
        config.channel.public.join(public)
    } else {
        public_path()
    }
    .clean();

    let path = absolute_path.join(file_stem.as_str());
    let bytes = fs::read(path).await?;

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, "application/octet-stream".parse().unwrap());
    headers.insert(CONTENT_DISPOSITION, "attachment".parse().unwrap());

    Ok((StatusCode::OK, headers, Body::from(bytes)).into_response())
}
