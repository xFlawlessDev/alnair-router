//! Admin CRUD: connections, aliases, combos, keys, and usage reporting.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;

use crate::db::repos::aliases::{CreateAlias, UpdateAlias};
use crate::db::repos::api_keys::{CreateApiKey, UpdateApiKey};
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
    state.invalidate_catalog().await;
    Ok((StatusCode::CREATED, Json(connection)))
}

pub async fn update_connection(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<UpdateConnection>,
) -> Result<impl IntoResponse> {
    let connection = state.connections().update(&id, input).await?;
    state.invalidate_catalog().await;
    Ok(Json(connection))
}

pub async fn delete_connection(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse> {
    if !state.connections().delete(&id).await? {
        return Err(Error::NotFound(format!("connection '{id}' not found")));
    }
    state.invalidate_catalog().await;
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
    if state
        .connections()
        .get(&input.connection_id)
        .await?
        .is_none()
    {
        return Err(Error::BadRequest(format!(
            "connection '{}' does not exist",
            input.connection_id
        )));
    }
    let alias = state.aliases().create(input).await?;
    state.invalidate_catalog().await;
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
    let alias = state.aliases().update(&id, input).await?;
    state.invalidate_catalog().await;
    Ok(Json(alias))
}

pub async fn delete_alias(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse> {
    if !state.aliases().delete(&id).await? {
        return Err(Error::NotFound(format!("alias '{id}' not found")));
    }
    state.invalidate_catalog().await;
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
    state.invalidate_catalog().await;
    Ok((StatusCode::CREATED, Json(combo)))
}

pub async fn update_combo(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<UpdateCombo>,
) -> Result<impl IntoResponse> {
    let combo = state.combos().update(&id, input).await?;
    state.invalidate_catalog().await;
    Ok(Json(combo))
}

pub async fn delete_combo(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse> {
    if !state.combos().delete(&id).await? {
        return Err(Error::NotFound(format!("combo '{id}' not found")));
    }
    state.invalidate_catalog().await;
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

/// Liveness probe: the process is up. Never touches the database, so a slow or
/// locked database cannot cause a restart loop.
pub async fn health() -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "service": "alnair-router",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// Readiness probe: checks the database, and optionally TCP reachability of
/// every enabled connection when `server.readiness_upstream_checks` is set.
///
/// Unreachable upstreams do not fail readiness — the router fails over between
/// tiers — they are reported so a human can see a sick connection.
pub async fn ready(State(state): State<AppState>) -> Response {
    if sqlx::query_scalar::<_, i64>("SELECT 1")
        .fetch_one(&state.pool)
        .await
        .is_err()
    {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "status": "not_ready", "database": "error" })),
        )
            .into_response();
    }

    let mut body = json!({ "status": "ready", "database": "ok" });
    if state.config.server.readiness_upstream_checks {
        let (reachable, unreachable) = check_upstreams(&state).await;
        body["upstreams"] = json!({ "reachable": reachable, "unreachable": unreachable });
    }

    (StatusCode::OK, Json(body)).into_response()
}

/// TCP-connects to each enabled connection, bounded to one second each.
async fn check_upstreams(state: &AppState) -> (usize, usize) {
    let Ok(snapshot) = state.catalog_snapshot().await else {
        return (0, 0);
    };

    let mut reachable = 0usize;
    let mut unreachable = 0usize;

    for connection in snapshot
        .catalog
        .connections
        .iter()
        .filter(|connection| connection.is_enabled())
    {
        let Some((host, port)) = upstream_endpoint(&connection.base_url) else {
            continue;
        };

        let attempt = tokio::net::TcpStream::connect((host.as_str(), port));
        match tokio::time::timeout(std::time::Duration::from_secs(1), attempt).await {
            Ok(Ok(_)) => reachable += 1,
            _ => unreachable += 1,
        }
    }

    (reachable, unreachable)
}

/// Extracts `host:port` from a connection's base URL.
fn upstream_endpoint(base_url: &str) -> Option<(String, u16)> {
    let url = url::Url::parse(base_url).ok()?;
    let host = url.host_str()?.to_string();
    let port = url.port_or_known_default()?;
    Some((host, port))
}

// ------------------------------------------------------------------- metrics

/// Prometheus text exposition of the router's counters.
pub async fn metrics(State(state): State<AppState>) -> impl IntoResponse {
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        state.metrics.render(),
    )
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
