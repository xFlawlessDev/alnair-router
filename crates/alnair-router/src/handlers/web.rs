//! Serves the embedded dashboard assets (`apps/web/dist`).

use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

const INDEX: &str = "index.html";

#[derive(RustEmbed)]
#[folder = "../../apps/web/dist"]
struct WebAssets;

/// Serves a static asset, falling back to `index.html` for SPA routes.
///
/// Paths that look like files (they carry an extension) get a real 404 so a
/// broken asset URL is visible instead of silently returning the shell.
pub async fn serve_asset(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { INDEX } else { path };

    if let Some(asset) = WebAssets::get(path) {
        return asset_response(path, asset.data.into_owned());
    }

    let last_segment = path.rsplit('/').next().unwrap_or_default();
    let is_route = !last_segment.contains('.');
    if is_route && let Some(index) = WebAssets::get(INDEX) {
        return asset_response(INDEX, index.data.into_owned());
    }

    (StatusCode::NOT_FOUND, "not found").into_response()
}

fn asset_response(path: &str, bytes: Vec<u8>) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let cache_control = if path.ends_with(".html") {
        "no-cache"
    } else {
        "public, max-age=31536000, immutable"
    };

    (
        [
            (header::CONTENT_TYPE, mime.as_ref().to_string()),
            (header::CACHE_CONTROL, cache_control.to_string()),
        ],
        bytes,
    )
        .into_response()
}
