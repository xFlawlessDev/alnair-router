//! Router-issued API key handlers.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Serialize;

use crate::db::repos::api_keys::{CreateApiKey, UpdateApiKey};
use crate::error::{Error, Result};
use crate::state::AppState;

pub async fn list_keys(State(state): State<AppState>) -> Result<impl IntoResponse> {
    Ok(Json(state.api_keys().list().await?))
}

pub async fn create_key(
    State(state): State<AppState>,
    Json(input): Json<CreateApiKey>,
) -> Result<impl IntoResponse> {
    let created = state.api_keys().create(input).await?;
    Ok((StatusCode::CREATED, Json(created)))
}

pub async fn update_key(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<UpdateApiKey>,
) -> Result<impl IntoResponse> {
    Ok(Json(state.api_keys().update(&id, input).await?))
}

pub async fn delete_key(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse> {
    if !state.api_keys().delete(&id).await? {
        return Err(Error::NotFound(format!("api key '{id}' not found")));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
struct RevealedKey {
    secret: String,
}

/// Returns a key's plaintext secret to the admin, for copying it back out.
/// 404s when no reversible copy was kept, which covers both keys minted before
/// secrets were stored and keys created while `server.store_key_secrets` is off.
pub async fn reveal_key(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse> {
    let secret = state.api_keys().reveal_secret(&id).await?.ok_or_else(|| {
        Error::NotFound(format!(
            "api key '{id}' has no stored secret; rotate it to get a new one"
        ))
    })?;
    Ok(Json(RevealedKey { secret }))
}

/// Mints a replacement secret for a key, invalidating the previous one.
pub async fn rotate_key(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse> {
    let created = state.api_keys().rotate(&id).await?;
    Ok((StatusCode::CREATED, Json(created)))
}
