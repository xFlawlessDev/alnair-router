//! Database backup download and restore upload.
//!
//! Both routes live under the admin-token guard. Downloads stream a fresh
//! SQLite snapshot written to a temp file that is deleted when the response
//! ends; uploads stream to a temp file with a size cap and are removed however
//! the request finishes.

use std::path::PathBuf;

use axum::Json;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::{IntoResponse, Response};
use futures::StreamExt;
use tokio::io::AsyncWriteExt;

use crate::backup;
use crate::error::{Error, Result};
use crate::state::AppState;

/// Uploads are streamed to disk; this cap keeps a runaway upload from filling
/// the temp volume.
const MAX_RESTORE_BYTES: u64 = 512 * 1024 * 1024;

/// Deletes the staged file however the request ends.
struct TempFile(PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn unique_path(prefix: &str) -> PathBuf {
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S-%3f");
    std::env::temp_dir().join(format!("alnair-router-{prefix}-{stamp}.sqlite"))
}

/// `GET /api/backup` — streams a consistent SQLite snapshot as a download.
pub async fn download_backup(State(state): State<AppState>) -> Result<Response> {
    let path = unique_path("backup");
    let guard = TempFile(path.clone());

    backup::snapshot(&state.pool, &path).await?;

    let file = tokio::fs::File::open(&path)
        .await
        .map_err(|error| Error::Internal(format!("cannot open the backup: {error}")))?;
    let stream = tokio_util::io::ReaderStream::new(file).map(move |chunk| {
        let _ = &guard; // keep the staged file until the download ends
        chunk
    });

    let filename = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "alnair-router-backup.sqlite".to_string());

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .map_err(|error| Error::Internal(error.to_string()))?,
    );

    Ok((headers, Body::from_stream(stream)).into_response())
}

/// `POST /api/restore` — replaces every data table with the uploaded backup.
pub async fn restore_backup(
    State(state): State<AppState>,
    request: Request,
) -> Result<impl IntoResponse> {
    let path = unique_path("restore");
    let _guard = TempFile(path.clone());

    let mut body = request.into_body().into_data_stream();
    let mut file = tokio::fs::File::create(&path)
        .await
        .map_err(|error| Error::Internal(format!("cannot stage the upload: {error}")))?;

    let mut written: u64 = 0;
    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(|error| Error::BadRequest(format!("upload failed: {error}")))?;
        written += chunk.len() as u64;
        if written > MAX_RESTORE_BYTES {
            return Err(Error::BadRequest(format!(
                "the backup exceeds the {} MiB upload limit",
                MAX_RESTORE_BYTES / (1024 * 1024)
            )));
        }
        file.write_all(&chunk)
            .await
            .map_err(|error| Error::Internal(format!("cannot stage the upload: {error}")))?;
    }
    file.flush()
        .await
        .map_err(|error| Error::Internal(format!("cannot stage the upload: {error}")))?;
    drop(file);

    if written == 0 {
        return Err(Error::BadRequest(
            "the uploaded backup is empty".to_string(),
        ));
    }

    let summary = backup::restore(&state.pool, &path, &state.cipher).await?;

    // Restored rows must not be shadowed by stale caches.
    state.invalidate_catalog().await;
    state.pricing_cache.invalidate().await;

    Ok(Json(summary))
}
