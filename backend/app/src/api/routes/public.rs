use axum::{
    extract::{Path, State},
    http::HeaderMap,
    response::Response,
};
use path_clean::PathClean;

use crate::{
    api::{routes::stream_file, state::AppState},
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
    headers: HeaderMap,
) -> Result<Response, ServiceError> {
    let base_path = if file_stem.ends_with(".ts")
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

    // Join the untrusted wildcard segment, then re-clean and confirm the
    // result stays inside the base directory. Without this an attacker can
    // escape with `..` segments (e.g. percent-encoded) and read arbitrary
    // files, since `PathBuf::join` does not resolve `..` on its own.
    let path = base_path.join(file_stem.as_str()).clean();
    if !path.starts_with(&base_path) {
        return Err(ServiceError::Forbidden("Access denied".to_string()));
    }

    stream_file(&path, &headers).await
}
