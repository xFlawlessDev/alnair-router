//! Proxied endpoints: embeddings, images, audio, video, search, web fetch.
//!
//! Every handler resolves the request's `model` reference, enforces the key's
//! model allowlist, rewrites the reference to the upstream's real model id and
//! records a usage row. The plumbing lives in `proxy`, the multipart byte
//! helpers in `multipart`, and `/v1/web/fetch` in `fetch`.

mod fetch;
mod multipart;
mod proxy;
mod search;

pub use fetch::web_fetch as local_web_fetch;

use axum::Extension;
use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::Response;

use crate::error::Result;
use crate::middleware::AuthenticatedKey;
use crate::model::ResolvedTarget;
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

/// Opt-in response transformations for `/v1/search`.
#[derive(Debug, Default, serde::Deserialize)]
pub struct SearchQuery {
    /// Map recognized provider responses onto the router's canonical result
    /// fields; false/absent preserves the upstream response verbatim.
    #[serde(default)]
    pub normalize: bool,
}

/// `POST /v1/search` — sends the request to the named provider, falling back to
/// search tiers when that provider is out of quota or rate limited.
///
/// Search-enabled connections (a `search_tier` or `search_path`) take precedence
/// over an unrelated chat default or a chat model echoed by a harness. With no
/// configured search tiers, a resolved primary remains usable for custom APIs.
/// Quota, rate-limit and transport failures try the next tier; a configured
/// search route returning 404 is surfaced so a wrong path remains visible.
///
/// Add `?normalize=true` to map recognized search bodies onto the router's
/// canonical shape. Normalization is opt-in to preserve the response contract
/// for clients already consuming provider-specific fields.
pub async fn search(
    State(state): State<AppState>,
    Extension(key): Extension<Option<AuthenticatedKey>>,
    axum::extract::Query(query): axum::extract::Query<SearchQuery>,
    Json(mut body): Json<serde_json::Value>,
) -> Result<Response> {
    let model = model_of(&body);
    ensure_model_allowed(&key, model.as_deref())?;

    if let Some(object) = body.as_object_mut() {
        object.remove("model");
    }

    // The primary is the named model's connection, or the default connection
    // when none is named. Search providers carry no model, so a request with
    // neither simply goes straight to the search tiers.
    let fallbacks = search_fallbacks(&state).await?;
    let mut candidates = Vec::new();
    match resolve_target(&state, model.as_deref()).await {
        Ok(primary) => {
            // A harness can echo a valid chat model here. Its connection is
            // not a search route unless it opted in through a path or tier.
            if fallbacks.is_empty()
                || primary.search_tier.is_some()
                || primary.search_path.is_some()
            {
                candidates.push(primary);
            }
        }
        // A malformed reference the caller actually named is their error;
        // surface it rather than silently answering from a different provider.
        Err(error) if model.is_some() && is_reference_error(&error) => return Err(error),
        // Anything else — an unknown name, or no model and no default
        // connection — leaves the search tiers as the route. A harness that
        // echoes its chat model here must still reach a configured search
        // provider instead of a 404.
        Err(_) => {}
    }
    let primary_id = candidates
        .first()
        .map(|target| target.connection_id.clone());
    candidates.extend(
        fallbacks
            .into_iter()
            .filter(|target| Some(&target.connection_id) != primary_id.as_ref()),
    );

    if candidates.is_empty() {
        return Err(crate::error::Error::NoRoute(
            "no search provider is available".to_string(),
        ));
    }

    let mut last = None;
    for target in candidates {
        let response = attempt_search(&state, &key, &model, &target, &body).await;
        match response {
            Ok(response) if !is_search_fallback_status(response.status()) => {
                return finish_search(response, query.normalize).await;
            }
            // Quota or rate limit: try the next candidate, keep this response
            // in case nothing else answers.
            Ok(response) => last = Some(Ok(response)),
            // Could not reach the provider at all: try the next candidate.
            Err(error) => last = Some(Err(error)),
        }
    }

    match last {
        Some(Ok(response)) => finish_search(response, query.normalize).await,
        Some(Err(error)) => Err(error),
        None => Err(crate::error::Error::NoRoute(
            "no search provider is available".to_string(),
        )),
    }
}

/// Statuses that mean a provider cannot serve more search right now, so the
/// next search tier should be tried. `404` is deliberately absent: it points to
/// a wrong path, which fallback would only hide.
fn is_search_fallback_status(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 402 | 429 | 432 | 433 | 434)
}

/// True when a resolve failure is about the *shape* of the model reference
/// rather than about it not existing.
///
/// `search` treats an unknown name as "no primary", because a harness may echo
/// its chat model into a search body and the search tiers are still a valid
/// route. A malformed reference is different: it is a caller bug, and silently
/// answering from a different provider would hide it.
fn is_reference_error(error: &crate::error::Error) -> bool {
    matches!(error, crate::error::Error::BadRequest(_))
}

/// Search connections in the order to try after the primary: tiers ascending,
/// each tier rotated round-robin.
async fn search_fallbacks(state: &AppState) -> Result<Vec<ResolvedTarget>> {
    let resolver = state.resolver().await?;
    let mut tiered: std::collections::BTreeMap<i64, Vec<ResolvedTarget>> =
        std::collections::BTreeMap::new();

    for target in resolver.search_targets() {
        if let Some(tier) = target.search_tier {
            tiered.entry(tier).or_default().push(target);
        }
    }

    let mut ordered = Vec::new();
    for (tier, members) in tiered {
        ordered.extend(state.search_rotation.order(tier, &members));
    }
    Ok(ordered)
}

/// Sends one attempt to a target, recording its usage row.
async fn attempt_search(
    state: &AppState,
    key: &Option<AuthenticatedKey>,
    model: &Option<String>,
    target: &ResolvedTarget,
    body: &serde_json::Value,
) -> Result<reqwest::Response> {
    let context = ProxyContext::new(state.clone(), key, model);
    let _permit = context.state.limiter.acquire(&target.connection_id).await?;
    let started = std::time::Instant::now();
    let proxy = MediaProxy::new();
    let response = proxy
        .post_json(
            target,
            target.search_path.as_deref().unwrap_or("/search"),
            body,
        )
        .await;
    let latency_ms = started.elapsed().as_millis() as u64;

    match &response {
        Ok(response) => {
            context
                .record(target, status_label(response.status()), latency_ms)
                .await
        }
        Err(_) => context.record(target, "error", latency_ms).await,
    }
    response
}

/// Hands the chosen upstream response back, normalizing it when asked.
async fn finish_search(response: reqwest::Response, normalize: bool) -> Result<Response> {
    if !response.status().is_success() {
        return passthrough(response).await;
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|error| crate::error::Error::Upstream(format!("search body: {error}")))?;
    let parsed: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(parsed) => parsed,
        // Not JSON at all: hand the caller exactly what the upstream said.
        Err(_) => return raw_json_response(bytes),
    };

    if !normalize {
        return raw_json_response(bytes);
    }

    match search::normalize_search_response(&parsed, None) {
        Some(normalized) => raw_json_response(
            serde_json::to_vec(&normalized)
                .map_err(|error| crate::error::Error::Internal(error.to_string()))?,
        ),
        None => raw_json_response(bytes),
    }
}

/// `POST /v1/web/fetch` — fetches a URL server-side.
///
/// Without a `model` the router fetches the URL itself, behind the SSRF guard.
/// A `model` naming a connection with `web_fetch_mode = provider` forwards the
/// URL to that connection's extract endpoint instead, which is how pages that
/// need JavaScript rendering are reached.
pub async fn web_fetch(
    State(state): State<AppState>,
    Extension(key): Extension<Option<AuthenticatedKey>>,
    Json(body): Json<serde_json::Value>,
) -> Result<Response> {
    let url = body
        .get("url")
        .and_then(|value| value.as_str())
        .ok_or_else(|| crate::error::Error::BadRequest("url is required".to_string()))?
        .to_string();

    let model = model_of(&body);
    let Some(model) = model.filter(|model| !model.trim().is_empty()) else {
        // No connection named: the router's own fetch is the default.
        return local_web_fetch(Json(body)).await;
    };

    // `model` is a hint, not a requirement. Before provider fetches existed this
    // endpoint ignored the field entirely, so a reference that does not resolve
    // must keep fetching locally rather than turning a working request into a
    // 404. The allowlist still applies: a key may not reach a model it was not
    // granted, even by accident.
    ensure_model_allowed(&key, Some(&model))?;
    let Ok(target) = resolve_target(&state, Some(&model)).await else {
        return local_web_fetch(Json(body)).await;
    };

    let mode = state
        .connections()
        .get(&target.connection_id)
        .await?
        .map(|connection| {
            (
                connection.web_fetch_mode,
                connection.web_fetch_path,
                connection.web_fetch_shape,
            )
        })
        .unwrap_or_default();

    let (mode, path, shape) = mode;
    if mode != "provider" {
        // The connection exists but fetches locally; behave exactly as before.
        return local_web_fetch(Json(body)).await;
    }

    let path = path.ok_or_else(|| {
        crate::error::Error::BadRequest(format!(
            "connection `{}` is set to fetch through its provider but has no web_fetch_path",
            target.connection_name
        ))
    })?;

    let format = body
        .get("format")
        .and_then(|value| value.as_str())
        .unwrap_or("markdown");

    let context = ProxyContext::new(state, &key, &Some(model));
    let _permit = context.state.limiter.acquire(&target.connection_id).await?;
    let started = std::time::Instant::now();

    let result = fetch::provider_fetch(fetch::ProviderFetch {
        target: &target,
        path: &path,
        shape: shape.as_deref(),
        url: &url,
        format,
    })
    .await;

    let latency_ms = started.elapsed().as_millis() as u64;
    let label = match &result {
        Ok(_) => "ok",
        Err(_) => "error",
    };
    context.record(&target, label, latency_ms).await;

    result
}

/// Returns a JSON body with a 200 status.
fn raw_json_response(bytes: impl Into<axum::body::Bytes>) -> Result<Response> {
    Response::builder()
        .status(axum::http::StatusCode::OK)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(bytes.into()))
        .map_err(|error| crate::error::Error::Internal(error.to_string()))
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
