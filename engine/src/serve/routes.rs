#[cfg(not(debug_assertions))]
use {
    axum::{
        Router,
        body::Body,
        extract::Path,
        http::{StatusCode, header},
        response::{IntoResponse, Response},
        routing::get,
    },
    chrono::Duration,
    include_dir::{Dir, File, include_dir},
    mime_guess::{Mime, mime},
};

#[cfg(not(debug_assertions))]
static FRONTEND_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../frontend/dist");

#[cfg(not(debug_assertions))]
const DEFAULT_FILE: &str = "index.html";

#[cfg(not(debug_assertions))]
const ROOT: &str = "";

#[cfg(not(debug_assertions))]
fn serve_file(
    file: &File,
    mime: Option<Mime>,
    cache: Duration,
    status: Option<StatusCode>,
) -> Response<Body> {
    let mime = mime.unwrap_or(mime::TEXT_HTML);
    let status = status.unwrap_or(StatusCode::OK);
    let cache_header = format!("max-age={}", cache.as_seconds_f32());

    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, mime.to_string())
        .header(header::CACHE_CONTROL, cache_header)
        .body(Body::from(file.contents().to_owned()))
        .unwrap()
}

#[cfg(not(debug_assertions))]
fn serve_index() -> Response<Body> {
    if let Some(file) = FRONTEND_DIR.get_file(DEFAULT_FILE) {
        serve_file(file, None, Duration::seconds(0), None)
    } else {
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("index.html not found"))
            .unwrap()
    }
}

#[cfg(not(debug_assertions))]
async fn serve_asset(path: Option<Path<String>>) -> impl IntoResponse {
    let Some(Path(path)) = path else {
        return serve_index();
    };

    if let Some(file) = FRONTEND_DIR.get_file(&path) {
        let mime = mime_guess::from_path(&path).first_or_octet_stream();
        let cache = if mime == mime::TEXT_HTML {
            Duration::seconds(0)
        } else {
            Duration::days(365)
        };
        return serve_file(file, Some(mime), cache, None);
    }

    if let Some(dir) = FRONTEND_DIR.get_dir(&path)
        && let Some(file) = dir.get_file(DEFAULT_FILE)
    {
        return serve_file(file, None, Duration::seconds(0), None);
    }

    // If nothing was found → return index.html (SPA fallback)
    serve_index()
}

#[cfg(not(debug_assertions))]
pub fn admin_ui_routes() -> Router {
    Router::new()
        .route(
            "/",
            get(|| async { serve_asset(Some(Path(String::from(ROOT)))).await }),
        )
        .route(
            "/{*path}",
            get(|path| async { serve_asset(Some(path)).await }),
        )
}
