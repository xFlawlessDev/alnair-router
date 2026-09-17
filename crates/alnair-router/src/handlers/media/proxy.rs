//! Shared plumbing for the proxied media endpoints: model resolution, the
//! key allowlist check, request forwarding and usage attribution.

use axum::body::Bytes;
use axum::http::StatusCode;
use axum::response::Response;
use serde_json::json;

use crate::db::repos::usage::NewUsageRecord;
use crate::error::{Error, Result};
use crate::middleware::AuthenticatedKey;
use crate::model::{ResolvedTarget, Resolver};
use crate::state::AppState;
use crate::upstream::media::MediaProxy;

/// Rejects requests whose model is outside the key's allowlist.
///
/// Every proxied endpoint funnels through here, so a restricted key cannot
/// reach an upstream through an endpoint that carries no JSON body.
pub(super) fn ensure_model_allowed(
    key: &Option<AuthenticatedKey>,
    model: Option<&str>,
) -> Result<()> {
    match (key.as_ref(), model) {
        (Some(auth), Some(model)) if !model.trim().is_empty() => auth.policy.ensure_model(model),
        _ => Ok(()),
    }
}

/// Resolves a single target for a proxied call, preferring the request's model
/// when it names one and falling back to the router's default connection.
///
/// A prefixed `alias/model` reference and a bare model name resolve through the
/// same path; only an absent model falls back to the default connection.
pub(super) async fn resolve_target(
    state: &AppState,
    model: Option<&str>,
) -> Result<ResolvedTarget> {
    let reference = match model {
        Some(model) if !model.trim().is_empty() => model.to_string(),
        _ => state
            .config
            .read()
            .expect("config lock poisoned")
            .router
            .default_connection
            .clone()
            .ok_or_else(|| {
                Error::BadRequest(
                    "no model given and no default_connection is configured".to_string(),
                )
            })?,
    };

    let resolver = state.resolver().await?;
    first_target(&resolver, &reference)
}

/// Resolves a model reference, failing when nothing resolves.
fn first_target(resolver: &Resolver, reference: &str) -> Result<ResolvedTarget> {
    resolver
        .resolve(reference)?
        .into_iter()
        .next()
        .ok_or_else(|| Error::NoRoute(format!("no target for '{reference}'")))
}

/// Reads a request body's `model` field, if it names one.
pub(super) fn model_of(body: &serde_json::Value) -> Option<String> {
    body.get("model")
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

/// Picks the target named by the body's `model`, else the default connection.
pub(super) async fn target_for_body(
    state: &AppState,
    key: &Option<AuthenticatedKey>,
    body: &serde_json::Value,
) -> Result<ResolvedTarget> {
    let model = model_of(body);
    ensure_model_allowed(key, model.as_deref())?;

    resolve_target(state, model.as_deref()).await
}

/// Replaces a router-prefixed model id with the upstream's real model id.
///
/// The upstream must see the *real* model id, not the router's prefix.
pub(super) fn rewrite_model(
    body: &serde_json::Value,
    target: &ResolvedTarget,
) -> serde_json::Value {
    let mut body = body.clone();
    if let Some(object) = body.as_object_mut() {
        object.insert("model".to_string(), json!(target.model));
    }
    body
}

/// Carries what a proxied call needs to attribute its outcome.
///
/// Media calls carry no token counts, so the row exists to attribute the spend
/// and the failure to a key and a connection rather than to meter it.
#[derive(Clone)]
pub(super) struct ProxyContext {
    pub(super) state: AppState,
    api_key_id: Option<String>,
    requested_model: String,
}

impl ProxyContext {
    pub(super) fn new(
        state: AppState,
        key: &Option<AuthenticatedKey>,
        model: &Option<String>,
    ) -> Self {
        Self {
            requested_model: model.clone().unwrap_or_default(),
            api_key_id: key.as_ref().map(|auth| auth.key.id.clone()),
            state,
        }
    }

    /// Records one proxied attempt, whether it succeeded or not.
    pub(super) async fn record(&self, target: &ResolvedTarget, status: &str, latency_ms: u64) {
        let record = NewUsageRecord {
            api_key_id: self.api_key_id.clone(),
            requested_model: self.requested_model.clone(),
            resolved_provider: Some(target.provider_type.clone()),
            resolved_model: Some(target.model.clone()),
            connection_name: Some(target.connection_name.clone()),
            attempt: 1,
            status: status.to_string(),
            latency_ms,
            ..Default::default()
        };

        if let Err(error) = self.state.usage().record(record).await {
            tracing::warn!(error = %error, "failed to record proxied usage");
        }
    }
}

/// Forwards a JSON body upstream and records the attempt's outcome.
pub(super) async fn forward_json(
    context: ProxyContext,
    target: &ResolvedTarget,
    path: &str,
    body: serde_json::Value,
) -> Result<Response> {
    let _permit = context.state.limiter.acquire(&target.connection_id).await?;
    let started = std::time::Instant::now();
    let proxy = MediaProxy::new();
    let response = proxy.post_json(target, path, &body).await;
    let latency_ms = started.elapsed().as_millis() as u64;

    let response = match response {
        Ok(response) => response,
        Err(error) => {
            context.record(target, "error", latency_ms).await;
            return Err(error);
        }
    };

    let status = response.status();
    context
        .record(target, status_label(status), latency_ms)
        .await;
    passthrough(response).await
}

/// Forwards a raw body upstream and records the attempt's outcome.
pub(super) async fn proxy_raw(
    context: ProxyContext,
    target: &ResolvedTarget,
    path: &str,
    content_type: Option<&str>,
    body: Bytes,
) -> Result<Response> {
    let _permit = context.state.limiter.acquire(&target.connection_id).await?;
    let started = std::time::Instant::now();
    let proxy = MediaProxy::new();
    let response = proxy
        .post_raw(target, path, content_type, body.to_vec(), None)
        .await;
    let latency_ms = started.elapsed().as_millis() as u64;

    let response = match response {
        Ok(response) => response,
        Err(error) => {
            context.record(target, "error", latency_ms).await;
            return Err(error);
        }
    };

    let status = response.status();
    context
        .record(target, status_label(status), latency_ms)
        .await;
    passthrough(response).await
}

/// Maps an upstream status onto the usage row's `ok`/`error` label.
pub(super) fn status_label(status: reqwest::StatusCode) -> &'static str {
    if status.is_success() { "ok" } else { "error" }
}

/// Streams an upstream response back to the client unchanged.
pub(super) async fn passthrough(response: reqwest::Response) -> Result<Response> {
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);

    let bytes = response
        .bytes()
        .await
        .map_err(|error| Error::Upstream(format!("failed to read upstream body: {error}")))?;

    let mut builder = Response::builder().status(status);
    if let Some(content_type) = content_type {
        builder = builder.header(axum::http::header::CONTENT_TYPE, content_type);
    }

    builder
        .body(axum::body::Body::from(bytes))
        .map_err(|error| Error::Internal(error.to_string()))
}
