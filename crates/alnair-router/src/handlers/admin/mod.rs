//! Admin CRUD: connections, aliases, combos, keys, and usage reporting.

mod keys;

pub use keys::{create_key, delete_key, list_keys, reveal_key, rotate_key, update_key};

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;

use crate::db::repos::aliases::{CreateAlias, UpdateAlias};
use crate::db::repos::combos::{CreateCombo, UpdateCombo};
use crate::db::repos::connections::{CreateConnection, UpdateConnection};
use crate::db::repos::key_plans::{CreateKeyPlan, UpdateKeyPlan};
use crate::db::repos::usage::NewUsageRecord;
use crate::error::{Error, Result};
use crate::limits::BudgetWindow;
use crate::state::AppState;

use crate::upstream::chat_backend;

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

// ----------------------------------------------------------------- key plans

pub async fn list_plans(State(state): State<AppState>) -> Result<impl IntoResponse> {
    Ok(Json(state.key_plans().list().await?))
}

pub async fn create_plan(
    State(state): State<AppState>,
    Json(input): Json<CreateKeyPlan>,
) -> Result<impl IntoResponse> {
    let plan = state.key_plans().create(input).await?;
    Ok((StatusCode::CREATED, Json(plan)))
}

pub async fn update_plan(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<UpdateKeyPlan>,
) -> Result<impl IntoResponse> {
    Ok(Json(state.key_plans().update(&id, input).await?))
}

pub async fn delete_plan(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse> {
    if !state.key_plans().delete(&id).await? {
        return Err(Error::NotFound(format!("plan '{id}' not found")));
    }
    Ok(StatusCode::NO_CONTENT)
}

// -------------------------------------------------------------------- usage

/// Usage queries combine pagination with the dashboard's filters.
///
/// The pagination fields are repeated here instead of flattening [`Pagination`]:
/// `serde_urlencoded` cannot coerce `"100"` into `i64` through a flattened map.
#[derive(Debug, Deserialize)]
pub struct UsageQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
    /// ISO-8601 lower bound for usage queries.
    #[serde(default)]
    pub since: Option<chrono::DateTime<chrono::Utc>>,
    /// ISO-8601 upper bound, inclusive.
    #[serde(default)]
    pub until: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub api_key_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub connection: Option<String>,
    /// Sort key for the usage table; absent keeps newest first.
    #[serde(default)]
    pub sort: Option<String>,
    /// `asc` or `desc`; absent defaults to descending.
    #[serde(default)]
    pub order: Option<String>,
    /// `hour` or `day`; only read by the trend endpoint, defaults to `day`.
    #[serde(default)]
    pub bucket: Option<String>,
}

impl UsageQuery {
    fn filter(&self) -> crate::db::repos::usage::UsageFilter {
        let mut filter = crate::db::repos::usage::UsageFilter::new(
            self.api_key_id.clone(),
            self.model.clone(),
            self.provider.clone(),
            self.connection.clone(),
            self.since,
        );
        filter.until = self.until;
        filter
    }

    fn sort(&self) -> Result<crate::db::repos::usage::Sort> {
        crate::db::repos::usage::Sort::parse(self.sort.as_deref(), self.order.as_deref())
    }
}

pub async fn list_usage(
    State(state): State<AppState>,
    Query(query): Query<UsageQuery>,
) -> Result<impl IntoResponse> {
    let records = state
        .usage()
        .list(
            query.limit.clamp(1, 500),
            query.offset.max(0),
            &query.filter(),
            query.sort()?,
        )
        .await?;
    Ok(Json(records))
}

pub async fn usage_summary(
    State(state): State<AppState>,
    Query(query): Query<UsageQuery>,
) -> Result<impl IntoResponse> {
    let filter = query.filter();
    let summary = state.usage().summary(&filter).await?;
    let savings = state.usage().savings(&filter).await?;
    Ok(Json(summary.savings(savings)))
}

/// `POST /api/token-saver/headroom/test` — reachability check for the proxy.
///
/// The URL in the body wins so the button can test a value that has not been
/// saved yet; without one the configured URL is used.
pub async fn headroom_test(
    State(state): State<AppState>,
    body: Option<Json<HeadroomTestRequest>>,
) -> Result<impl IntoResponse> {
    let (url, timeout_ms) = {
        let config = state.config_snapshot();
        (
            config.token_saver.headroom_url.clone(),
            config.token_saver.headroom_timeout_ms,
        )
    };

    let url = body
        .and_then(|Json(request)| request.url)
        .map(|url| url.trim().to_string())
        .filter(|url| !url.is_empty())
        .unwrap_or(url);

    Ok(Json(
        crate::token_saver::probe_headroom(&url, timeout_ms).await,
    ))
}

/// Body of the Headroom test request; both fields are optional.
#[derive(Debug, Deserialize)]
pub struct HeadroomTestRequest {
    #[serde(default)]
    pub url: Option<String>,
}

/// `GET /api/usage/facets` — distinct models and providers for the filter bar.
pub async fn usage_facets(State(state): State<AppState>) -> Result<impl IntoResponse> {
    Ok(Json(state.usage().facets().await?))
}

/// `GET /api/usage/models` — per-model rollup under the same filters as the
/// summary, so the dashboard can break spend down without the self-service key.
pub async fn usage_models(
    State(state): State<AppState>,
    Query(query): Query<UsageQuery>,
) -> Result<impl IntoResponse> {
    Ok(Json(state.usage().models(&query.filter()).await?))
}

/// `GET /api/usage/timeseries` — the trend chart behind the usage table.
pub async fn usage_timeseries(
    State(state): State<AppState>,
    Query(query): Query<UsageQuery>,
) -> Result<impl IntoResponse> {
    let bucket = crate::db::repos::usage::Bucket::parse(query.bucket.as_deref())?;
    Ok(Json(
        state.usage().timeseries(&query.filter(), bucket).await?,
    ))
}

/// Spend per key for the dashboard's budget monitor.
pub async fn usage_by_key(State(state): State<AppState>) -> Result<impl IntoResponse> {
    let now = chrono::Utc::now();
    let rows = state
        .usage()
        .spend_by_key(
            BudgetWindow::Daily.start(now),
            BudgetWindow::Weekly.start(now),
            BudgetWindow::Monthly.start(now),
        )
        .await?;
    Ok(Json(rows))
}

// ------------------------------------------------------------------ pricing

pub async fn list_pricing(State(state): State<AppState>) -> Result<impl IntoResponse> {
    Ok(Json(state.pricing().list().await?))
}

#[derive(Debug, Deserialize)]
pub struct PricingOverrides {
    #[serde(default)]
    pub prices: Vec<crate::pricing::PriceInput>,
}

/// Upserts dashboard overrides; synced rows for the same model stay shadowed.
pub async fn upsert_pricing(
    State(state): State<AppState>,
    Json(input): Json<PricingOverrides>,
) -> Result<impl IntoResponse> {
    let updated = state.pricing().upsert_overrides(&input.prices).await?;
    state.pricing_cache.invalidate().await;
    Ok(Json(json!({ "updated": updated })))
}

#[derive(Debug, Deserialize)]
pub struct PricingDeleteQuery {
    /// Delete only this model's override; absent clears all overrides.
    #[serde(default)]
    pub model: Option<String>,
}

pub async fn delete_pricing(
    State(state): State<AppState>,
    Query(query): Query<PricingDeleteQuery>,
) -> Result<impl IntoResponse> {
    let deleted = state
        .pricing()
        .delete_overrides(query.model.as_deref())
        .await?;
    state.pricing_cache.invalidate().await;
    Ok(Json(json!({ "deleted": deleted })))
}

/// `POST /api/pricing/sync` — crawls the configured catalog now.
pub async fn sync_pricing(State(state): State<AppState>) -> Result<impl IntoResponse> {
    let source_url = state
        .config
        .read()
        .expect("config lock poisoned")
        .pricing
        .source_url
        .clone();
    let status = crate::pricing::sync_from_source(&state.pricing_cache, &source_url).await?;
    Ok(Json(status))
}

pub async fn pricing_sync_status(State(state): State<AppState>) -> Result<impl IntoResponse> {
    Ok(Json(state.pricing().sync_status().await?))
}

#[derive(Debug, Deserialize)]
pub struct PricingMatchQuery {
    pub model: String,
}

/// `GET /api/pricing/match?model=…` — shows which catalog key answers an id.
pub async fn match_pricing(
    State(state): State<AppState>,
    Query(query): Query<PricingMatchQuery>,
) -> Result<impl IntoResponse> {
    let model = query.model.trim();
    if model.is_empty() {
        return Err(Error::BadRequest("model is required".to_string()));
    }

    match state.pricing_cache.match_for(model).await {
        Some(found) => Ok(Json(json!(found))),
        None => Ok(Json(json!({ "model": model, "matched": null }))),
    }
}

// ------------------------------------------------------------ upstream probes

/// `GET /api/connections/{id}/models` — lists the models the upstream offers.
pub async fn connection_models(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse> {
    let connection = state
        .connections()
        .get(&id)
        .await?
        .ok_or_else(|| Error::NotFound(format!("connection '{id}' not found")))?;
    let outcome = crate::upstream::probe::fetch_models(&connection).await?;

    Ok(Json(json!({
        "connection_id": connection.id,
        "provider_type": connection.provider_type,
        "base_url": connection.base_url,
        "latency_ms": outcome.latency_ms,
        "models": outcome.models,
        "enumerable": outcome.enumerable,
    })))
}

/// `POST /api/connections/{id}/test` — connectivity check via the models probe.
pub async fn connection_test(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse> {
    let connection = state
        .connections()
        .get(&id)
        .await?
        .ok_or_else(|| Error::NotFound(format!("connection '{id}' not found")))?;
    let outcome = crate::upstream::probe::fetch_models(&connection).await?;
    let count = outcome.models.len();

    let message = if outcome.enumerable {
        format!("Connected — {count} models available")
    } else {
        "Connected — model list not available (this provider does not publish /models)".to_string()
    };

    Ok(Json(json!({
        "ok": true,
        "models_count": count,
        "enumerable": outcome.enumerable,
        "latency_ms": outcome.latency_ms,
        "message": message,
    })))
}

/// `POST /api/aliases/{id}/test` — alias state plus upstream model check.
pub async fn alias_test(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse> {
    let alias = state
        .aliases()
        .get(&id)
        .await?
        .ok_or_else(|| Error::NotFound(format!("alias '{id}' not found")))?;
    let connection = state
        .connections()
        .get(&alias.connection_id)
        .await?
        .ok_or_else(|| {
            Error::NotFound(format!("connection '{}' not found", alias.connection_id))
        })?;

    if !alias.is_enabled() {
        return Ok(Json(test_failure(
            "alias is disabled",
            alias.model_override.clone(),
            None,
        )));
    }
    if !connection.is_enabled() {
        return Ok(Json(test_failure(
            &format!("connection '{}' is disabled", connection.name),
            alias.model_override.clone(),
            None,
        )));
    }

    let outcome = crate::upstream::probe::fetch_models(&connection).await?;
    let model = alias.model_override.clone();
    let available = model
        .as_deref()
        .filter(|_| outcome.enumerable)
        .map(|model| crate::upstream::probe::model_available(&outcome.models, model));

    let (ok, message) = match (model.as_deref(), available, outcome.enumerable) {
        (_, _, false) => (
            true,
            format!(
                "Connected — model list not available (this provider does not publish /models); cannot verify '{}'",
                model.as_deref().unwrap_or("")
            ),
        ),
        (Some(model), Some(true), _) => (
            true,
            format!(
                "Connected — '{model}' is available on '{}'",
                connection.name
            ),
        ),
        (Some(model), _, _) => (
            false,
            format!(
                "Connected, but '{model}' is not among the {} models offered by '{}'",
                outcome.models.len(),
                connection.name
            ),
        ),
        (None, _, _) => (
            true,
            format!(
                "Connected — prefix '{}' routes any model to '{}'",
                alias.prefix, connection.name
            ),
        ),
    };

    Ok(Json(json!({
        "ok": ok,
        "message": message,
        "model": model,
        "model_available": available,
        "models_count": outcome.models.len(),
        "enumerable": outcome.enumerable,
        "latency_ms": outcome.latency_ms,
    })))
}

fn test_failure(
    message: &str,
    model: Option<String>,
    available: Option<bool>,
) -> serde_json::Value {
    json!({
        "ok": false,
        "message": message,
        "model": model,
        "model_available": available,
        "models_count": 0,
        "enumerable": false,
        "latency_ms": 0,
    })
}

/// Request body for the alias chat probe.
#[derive(Debug, Default, Deserialize)]
pub struct AliasChatTestRequest {
    /// Prompt to send; defaults to a tiny ping.
    #[serde(default)]
    pub prompt: Option<String>,
    /// Model segment to use when the alias has no override.
    #[serde(default)]
    pub model: Option<String>,
}

/// `POST /api/aliases/{id}/test-chat` — one real non-streaming completion.
///
/// Runs through the normal resolver + executor, so it verifies the same path a
/// client would take. Results come back as `{ ok, message, ... }` with HTTP 200
/// (except for an unknown alias) so the UI can show diagnostics inline.
pub async fn alias_chat_test(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<AliasChatTestRequest>,
) -> Result<impl IntoResponse> {
    let alias = state
        .aliases()
        .get(&id)
        .await?
        .ok_or_else(|| Error::NotFound(format!("alias '{id}' not found")))?;

    if !alias.is_enabled() {
        return Ok(Json(json!({ "ok": false, "message": "alias is disabled" })));
    }

    let Some(model) = input.model.clone().or_else(|| alias.model_override.clone()) else {
        return Ok(Json(json!({
            "ok": false,
            "message": "alias has no model override; pass { \"model\": \"...\" } to test a model",
        })));
    };

    let reference = format!("{}/{}", alias.prefix, model);
    let resolver = state.resolver().await?;
    let targets = match resolver.resolve(&reference) {
        Ok(targets) => targets,
        Err(error) => return Ok(Json(json!({ "ok": false, "message": error.to_string() }))),
    };

    let prompt = input
        .prompt
        .map(|prompt| prompt.trim().to_string())
        .filter(|prompt| !prompt.is_empty())
        .unwrap_or_else(|| "Reply with the single word: pong".to_string());
    let messages = vec![chat_backend::message_text("user", prompt)];
    let options = chat_backend::GenerationOptions {
        max_tokens: Some(64),
        ..Default::default()
    };

    let started = std::time::Instant::now();
    let executed = match state
        .executor
        .stream(&targets, messages, None, Some(&options), false)
        .await
    {
        Ok(executed) => executed,
        Err(error) => return Ok(Json(json!({ "ok": false, "message": error.to_string() }))),
    };

    let target = executed.target.clone();
    let attempts = executed.attempts.len();
    let completion = match chat_backend::collect(executed.stream).await {
        Ok(completion) => completion,
        Err(error) => {
            return Ok(Json(json!({
                "ok": false,
                "message": error.to_string(),
                "model": target.model,
                "source": target.source,
            })));
        }
    };

    let usage = completion.usage.unwrap_or_default();
    let latency_ms = started.elapsed().as_millis() as u64;

    if let Err(error) = state
        .usage()
        .record(NewUsageRecord {
            api_key_id: None,
            requested_model: reference,
            resolved_provider: Some(target.provider_type.clone()),
            resolved_model: Some(target.model.clone()),
            connection_name: Some(target.connection_name.clone()),
            attempt: attempts,
            status: "ok".to_string(),
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            cached_tokens: usage.cached_tokens,
            reasoning_tokens: usage.reasoning_tokens,
            cost_usd: usage.cost_usd,
            cost_input_usd: usage.cost_input_usd,
            cost_output_usd: usage.cost_output_usd,
            cost_reasoning_usd: usage.cost_reasoning_usd,
            latency_ms,
            ..Default::default()
        })
        .await
    {
        tracing::warn!(error = %error, "failed to record alias test usage");
    }

    Ok(Json(json!({
        "ok": true,
        "message": format!("Completion from '{}' via {}", target.model, target.source),
        "content": completion.content,
        "finish_reason": completion.finish_reason,
        "model": target.model,
        "source": target.source,
        "provider_type": target.provider_type,
        "attempts": attempts,
        "prompt_tokens": usage.prompt_tokens,
        "completion_tokens": usage.completion_tokens,
        "cost_usd": usage.cost_usd,
        "latency_ms": latency_ms,
    })))
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
    let readiness_checks = state
        .config
        .read()
        .expect("config lock poisoned")
        .server
        .readiness_upstream_checks;
    if readiness_checks {
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

// ------------------------------------------------------------------- activity

/// `GET /api/activity` — live attempts, per-connection counters, recent events.
///
/// Configured connections are merged in so the topology shows every provider,
/// idle or busy, not just the ones that already took traffic.
pub async fn activity(
    State(state): State<AppState>,
    Query(pagination): Query<Pagination>,
) -> Result<impl IntoResponse> {
    let events = pagination.limit.clamp(1, 500) as usize;
    let mut snapshot = state.telemetry.snapshot(events);

    if let Ok(catalog) = state.catalog_snapshot().await {
        let known: std::collections::HashSet<String> = snapshot
            .connections
            .iter()
            .map(|stats| stats.id.clone())
            .collect();

        for connection in catalog
            .catalog
            .connections
            .iter()
            .filter(|connection| connection.is_enabled())
        {
            if !known.contains(&connection.id) {
                snapshot
                    .connections
                    .push(crate::telemetry::ConnectionActivity {
                        id: connection.id.clone(),
                        name: connection.name.clone(),
                        ..Default::default()
                    });
            }
        }
        snapshot.connections.sort_by(|a, b| a.name.cmp(&b.name));
    }

    Ok(Json(snapshot))
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
    let require_api_key = state
        .config
        .read()
        .expect("config lock poisoned")
        .server
        .require_api_key;

    Ok(Json(json!({
        "initialized": enabled > 0,
        "connections": connections.len(),
        "enabled_connections": enabled,
        "require_api_key": require_api_key,
    })))
}
