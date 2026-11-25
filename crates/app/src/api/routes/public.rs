use axum::{
    extract::{Path, State},
    http::{
        HeaderMap, HeaderValue,
        header::{CONTENT_DISPOSITION, CONTENT_TYPE},
    },
    response::Response,
};
use path_clean::PathClean;

use crate::{
    api::{routes::stream_file, state::AppState},
    file::ensure_path_within_root,
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
    let hls_type = hls_content_type(&file_stem);
    let base_path = if hls_type.is_some() {
        let manager = {
            let guard = state.controller.read().await;
            guard.get(id)
        }
        .ok_or_else(|| ServiceError::BadRequest(format!("Channel {id} not found!")))?;

        let config = manager.config.read().await;
        let base = config.channel.public.join(public).clean();
        ensure_path_within_root(&config.channel.public, &base)?;

        base
    } else {
        public_path()
    }
    .clean();

    // Join the untrusted wildcard segment, then re-clean and confirm the
    // result stays inside the base directory. Without this an attacker can
    // escape with `..` segments (e.g. percent-encoded) and read arbitrary
    // files, since `PathBuf::join` does not resolve `..` on its own.
    let path = base_path.join(file_stem.as_str()).clean();
    ensure_path_within_root(&base_path, &path)?;

    let mut response = stream_file(&path, &headers).await?;

    if let Some(content_type) = hls_type {
        response
            .headers_mut()
            .insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
        response.headers_mut().remove(CONTENT_DISPOSITION);
    }

    Ok(response)
}

fn hls_content_type(name: &str) -> Option<&'static str> {
    match std::path::Path::new(name).extension()?.to_str()? {
        "m3u8" => Some("application/vnd.apple.mpegurl"),
        "ts" => Some("video/mp2t"),
        "vtt" => Some("text/vtt"),
        "m4s" => Some("video/mp4"),
        "mp4" if name.ends_with(".init.mp4") => Some("video/mp4"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::hls_content_type;

    #[test]
    fn routes_fmp4_and_its_initialization_as_channel_hls_resources() {
        assert_eq!(hls_content_type("high_12.m4s"), Some("video/mp4"));
        assert_eq!(hls_content_type("high_12.init.mp4"), Some("video/mp4"));
        assert_eq!(hls_content_type("ordinary-video.mp4"), None);
    }
}
