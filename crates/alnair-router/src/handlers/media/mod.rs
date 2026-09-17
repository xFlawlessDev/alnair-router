//! Proxied endpoints: embeddings, images, audio, video, search, web fetch.
//!
//! Every handler resolves the request's `model` reference, enforces the key's
//! model allowlist, rewrites the reference to the upstream's real model id and
//! records a usage row. The plumbing lives in `proxy`, the multipart byte
//! helpers in `multipart`, and `/v1/web/fetch` in `fetch`.

mod fetch;
mod multipart;
mod proxy;

pub use fetch::web_fetch;

use axum::Extension;
use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::Response;

use crate::error::Result;
use crate::middleware::AuthenticatedKey;
use crate::state::AppState;
use crate::upstream::media::MediaProxy;

use multipart::{multipart_field, rewrite_multipart_field};
use proxy::{
    ProxyContext, ensure_model_allowed, forward_json, model_of, passthrough, proxy_raw,
    resolve_target, rewrite_model, status_label, target_for_body,
};

/// `?model=` for proxied endpoints whose wire format has no model field.
#[derive(Debug, Default, serde::Deserialize)]
pub struct ModelQuery {
    #[serde(default)]
    pub model: Option<String>,
}

/// `POST /v1/embeddings` — forwarded to the resolved connection's `/embeddings`.
pub async fn embeddings(
    State(state): State<AppState>,
    Extension(key): Extension<Option<AuthenticatedKey>>,
    Json(body): Json<serde_json::Value>,
) -> Result<Response> {
    let target = target_for_body(&state, &key, &body).await?;
    let model = model_of(&body);
    let body = rewrite_model(&body, &target);
    let context = ProxyContext::new(state, &key, &model);
    forward_json(context, &target, "/embeddings", body).await
}

/// `POST /v1/images/generations` — OpenAI-compatible image proxy.
pub async fn image_generations(
    State(state): State<AppState>,
    Extension(key): Extension<Option<AuthenticatedKey>>,
    Json(body): Json<serde_json::Value>,
) -> Result<Response> {
    let target = target_for_body(&state, &key, &body).await?;
    let model = model_of(&body);
    let body = rewrite_model(&body, &target);
    let context = ProxyContext::new(state, &key, &model);
    forward_json(context, &target, "/images/generations", body).await
}

/// `POST /v1/images/edits` — multipart image edit pass-through.
pub async fn image_edits(
    State(state): State<AppState>,
    Extension(key): Extension<Option<AuthenticatedKey>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response> {
    forward_multipart(state, key, headers, body, "/images/edits").await
}

/// `POST /v1/images/variations` — multipart image variation pass-through.
pub async fn image_variations(
    State(state): State<AppState>,
    Extension(key): Extension<Option<AuthenticatedKey>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response> {
    forward_multipart(state, key, headers, body, "/images/variations").await
}

/// `POST /v1/audio/speech` — text-to-speech proxy.
pub async fn audio_speech(
    State(state): State<AppState>,
    Extension(key): Extension<Option<AuthenticatedKey>>,
    Json(body): Json<serde_json::Value>,
) -> Result<Response> {
    let target = target_for_body(&state, &key, &body).await?;
    let model = model_of(&body);
    let body = rewrite_model(&body, &target);
    let context = ProxyContext::new(state, &key, &model);
    forward_json(context, &target, "/audio/speech", body).await
}

/// `POST /v1/audio/transcriptions` — multipart pass-through.
pub async fn audio_transcriptions(
    State(state): State<AppState>,
    Extension(key): Extension<Option<AuthenticatedKey>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response> {
    forward_multipart(state, key, headers, body, "/audio/transcriptions").await
}

/// `POST /v1/audio/translations` — multipart pass-through to the same shape.
pub async fn audio_translations(
    State(state): State<AppState>,
    Extension(key): Extension<Option<AuthenticatedKey>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response> {
    forward_multipart(state, key, headers, body, "/audio/translations").await
}

/// `POST /v1/moderations` — JSON pass-through.
pub async fn moderations(
    State(state): State<AppState>,
    Extension(key): Extension<Option<AuthenticatedKey>>,
    Json(body): Json<serde_json::Value>,
) -> Result<Response> {
    let target = target_for_body(&state, &key, &body).await?;
    let model = model_of(&body);
    let body = rewrite_model(&body, &target);
    let context = ProxyContext::new(state, &key, &model);
    forward_json(context, &target, "/moderations", body).await
}

/// `POST /v1/videos/generations` — async video job creation.
pub async fn video_generations(
    State(state): State<AppState>,
    Extension(key): Extension<Option<AuthenticatedKey>>,
    Json(body): Json<serde_json::Value>,
) -> Result<Response> {
    let target = target_for_body(&state, &key, &body).await?;
    let model = model_of(&body);
    let body = rewrite_model(&body, &target);
    let context = ProxyContext::new(state, &key, &model);
    forward_json(context, &target, "/videos/generations", body).await
}

/// `GET /v1/videos/{id}` — async video job polling pass-through.
///
/// A polling URL carries no model, so the caller names the connection with
/// `?model=`; without it the default connection is polled.
pub async fn video_status(
    State(state): State<AppState>,
    Extension(key): Extension<Option<AuthenticatedKey>>,
    axum::extract::Path(id): axum::extract::Path<String>,
    axum::extract::Query(query): axum::extract::Query<ModelQuery>,
) -> Result<Response> {
    ensure_model_allowed(&key, query.model.as_deref())?;
    let target = resolve_target(&state, query.model.as_deref()).await?;
    let context = ProxyContext::new(state, &key, &query.model);

    let _permit = context.state.limiter.acquire(&target.connection_id).await?;
    let started = std::time::Instant::now();
    let proxy = MediaProxy::new();
    let response = proxy.get(&target, &format!("/videos/{id}")).await;
    let latency_ms = started.elapsed().as_millis() as u64;

    let response = match response {
        Ok(response) => response,
        Err(error) => {
            context.record(&target, "error", latency_ms).await;
            return Err(error);
        }
    };

    let status = response.status();
    context
        .record(&target, status_label(status), latency_ms)
        .await;
    passthrough(response).await
}

/// `POST /v1/search` — SearXNG-style search passthrough.
///
/// The upstream takes no model, so an optional `model` in the body only picks
/// the connection; it is stripped before the request is forwarded.
pub async fn search(
    State(state): State<AppState>,
    Extension(key): Extension<Option<AuthenticatedKey>>,
    Json(mut body): Json<serde_json::Value>,
) -> Result<Response> {
    let model = model_of(&body);
    ensure_model_allowed(&key, model.as_deref())?;

    if let Some(object) = body.as_object_mut() {
        object.remove("model");
    }

    let target = resolve_target(&state, model.as_deref()).await?;
    let context = ProxyContext::new(state, &key, &model);
    let _permit = context.state.limiter.acquire(&target.connection_id).await?;
    let started = std::time::Instant::now();
    let proxy = MediaProxy::new();
    let response = proxy.post_json(&target, "/search", &body).await;
    let latency_ms = started.elapsed().as_millis() as u64;

    let response = match response {
        Ok(response) => response,
        Err(error) => {
            context.record(&target, "error", latency_ms).await;
            return Err(error);
        }
    };

    let status = response.status();
    context
        .record(&target, status_label(status), latency_ms)
        .await;
    passthrough(response).await
}

/// Forwards a multipart body upstream, rewriting its `model` field.
///
/// The field selects the connection, and the router's prefix must not reach the
/// upstream, so the value is replaced in place. Editing the field leaves every
/// other part — including file bytes — untouched, which re-encoding would not.
async fn forward_multipart(
    state: AppState,
    key: Option<AuthenticatedKey>,
    headers: HeaderMap,
    body: Bytes,
    path: &str,
) -> Result<Response> {
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok());

    let model = multipart_field(&body, content_type, "model");
    ensure_model_allowed(&key, model.as_deref())?;

    let target = resolve_target(&state, model.as_deref()).await?;
    let body = match model.as_deref() {
        Some(requested) if requested != target.model => {
            rewrite_multipart_field(&body, content_type, "model", &target.model)
        }
        _ => body,
    };

    let context = ProxyContext::new(state, &key, &model);
    proxy_raw(context, &target, path, content_type, body).await
}

#[cfg(test)]
mod tests;
