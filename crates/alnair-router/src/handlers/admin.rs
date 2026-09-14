//! Admin CRUD: connections, aliases, combos, keys, and usage reporting.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::json;

use crate::db::repos::aliases::{CreateAlias, UpdateAlias};
use crate::db::repos::api_keys::CreateApiKey;
use crate::db::repos::combos::{CreateCombo, UpdateCombo};
use crate::db::repos::connections::{CreateConnection, UpdateConnection};
use crate::error::{Error, Result};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct Pagination {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
    /// ISO-8601 lower bound for usage queries.
    #[serde(default)]
    pub since: Option<chrono::DateTime<chrono::Utc>>,
}

fn default_limit() -> i64 {
    100
}

// ---------------------------------------------------------------- connections

pub async fn list_connections(State(state): State<AppState>) -> Result<impl IntoResponse> {
    Ok(Json(state.connections().list().await?))
}

pub async fn create_connection(
    State(state): State<AppState>,
    Json(input): Json<CreateConnection>,
) -> Result<impl IntoResponse> {
    let connection = state.connections().create(input).await?;
    Ok((StatusCode::CREATED, Json(connection)))
}

pub async fn update_connection(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<UpdateConnection>,
) -> Result<impl IntoResponse> {
    Ok(Json(state.connections().update(&id, input).await?))
}

pub async fn delete_connection(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse> {
    if !state.connections().delete(&id).await? {
        return Err(Error::NotFound(format!("connection '{id}' not found")));
    }
    Ok(StatusCode::NO_CONTENT)
}

// -------------------------------------------------------------------- aliases

pub async fn list_aliases(State(state): State<AppState>) -> Result<impl IntoResponse> {
    Ok(Json(state.aliases().list().await?))
}

pub async fn create_alias(
    State(state): State<AppState>,
    Json(input): Json<CreateAlias>,
) -> Result<impl IntoResponse> {
    // An alias that points at a missing connection would fail at request time;
    // reject it up front instead.
    if state.connections().get(&input.connection_id).await?.is_none() {
        return Err(Error::BadRequest(format!(
            "connection '{}' does not exist",
            input.connection_id
        )));
    }
    let alias = state.aliases().create(input).await?;
    Ok((StatusCode::CREATED, Json(alias)))
}

pub async fn update_alias(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<UpdateAlias>,
) -> Result<impl IntoResponse> {
    if let Some(connection_id) = &input.connection_id
        && state.connections().get(connection_id).await?.is_none()
    {
        return Err(Error::BadRequest(format!(
            "connection '{connection_id}' does not exist"
        )));
    }
    Ok(Json(state.aliases().update(&id, input).await?))
}

pub async fn delete_alias(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse> {
    if !state.aliases().delete(&id).await? {
        return Err(Error::NotFound(format!("alias '{id}' not found")));
    }
    Ok(StatusCode::NO_CONTENT)
}

// --------------------------------------------------------------------- combos

pub async fn list_combos(State(state): State<AppState>) -> Result<impl IntoResponse> {
    let combos = state.combos().list().await?;
    let mut out = Vec::with_capacity(combos.len());
    for combo in combos {
        let entries = state.combos().entries(&combo.id).await?;
        out.push(json!({ "combo": combo, "entries": entries }));
    }
    Ok(Json(out))
}

pub async fn create_combo(
    State(state): State<AppState>,
    Json(input): Json<CreateCombo>,
) -> Result<impl IntoResponse> {
    let combo = state.combos().create(input).await?;
    Ok((StatusCode::CREATED, Json(combo)))
}

pub async fn update_combo(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<UpdateCombo>,
) -> Result<impl IntoResponse> {
    Ok(Json(state.combos().update(&id, input).await?))
}

pub async fn delete_combo(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse> {
    if !state.combos().delete(&id).await? {
        return Err(Error::NotFound(format!("combo '{id}' not found")));
    }
    Ok(StatusCode::NO_CONTENT)
}

// ----------------------------------------------------------------- api keys

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

pub async fn delete_key(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse> {
    if !state.api_keys().delete(&id).await? {
        return Err(Error::NotFound(format!("api key '{id}' not found")));
    }
    Ok(StatusCode::NO_CONTENT)
}

// -------------------------------------------------------------------- usage

pub async fn list_usage(
    State(state): State<AppState>,
    Query(pagination): Query<Pagination>,
) -> Result<impl IntoResponse> {
    let records = state
        .usage()
        .list(pagination.limit.clamp(1, 500), pagination.offset.max(0))
        .await?;
    Ok(Json(records))
}

pub async fn usage_summary(
    State(state): State<AppState>,
    Query(pagination): Query<Pagination>,
) -> Result<impl IntoResponse> {
    Ok(Json(state.usage().summary(pagination.since).await?))
}

// -------------------------------------------------------------------- health

pub async fn health(State(state): State<AppState>) -> Result<impl IntoResponse> {
    // A trivial query proves the pool is live.
    sqlx::query_scalar::<_, i64>("SELECT 1")
        .fetch_one(&state.pool)
        .await?;

    Ok(Json(json!({
        "status": "ok",
        "service": "alnair-router",
        "version": env!("CARGO_PKG_VERSION"),
    })))
}

pub async fn version() -> impl IntoResponse {
    Json(json!({
        "name": env!("CARGO_PKG_NAME"),
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// Reports whether the router has been configured with at least one connection.
pub async fn init_state(State(state): State<AppState>) -> Result<impl IntoResponse> {
    let connections = state.connections().list().await?;
    let enabled = connections.iter().filter(|c| c.is_enabled()).count();

    Ok(Json(json!({
        "initialized": enabled > 0,
        "connections": connections.len(),
        "enabled_connections": enabled,
        "require_api_key": state.config.server.require_api_key,
    })))
}
