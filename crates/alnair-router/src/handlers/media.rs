//! Proxied endpoints: embeddings, images, audio, video, search, web fetch.

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::Json;
use serde_json::json;

use crate::error::{Error, Result};
use crate::model::{ResolvedTarget, Resolver};
use crate::state::AppState;
use crate::upstream::media::MediaProxy;

/// Resolves a single target for a proxied call, preferring the request's model
/// when it names one and falling back to the router's default connection.
async fn resolve_target(state: &AppState, model: Option<&str>) -> Result<ResolvedTarget> {
    let reference = match model {
        Some(model) if !model.trim().is_empty() => model.to_string(),
        _ => state
            .config
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

/// Resolves the reference named by `model`, used when it carries a prefix.
async fn resolve_model_reference(state: &AppState, model: &str) -> Result<ResolvedTarget> {
    let resolver = state.resolver().await?;
    first_target(&resolver, model)
}

/// `POST /v1/embeddings` — forwarded to the resolved connection's `/embeddings`.
pub async fn embeddings(
    State(state): State<AppState>,
    Json(mut body): Json<serde_json::Value>,
) -> Result<Response> {
    let model = body
        .get("model")
        .and_then(|value| value.as_str())
        .map(str::to_string);

    let target = match &model {
        Some(model) if model.contains('/') => resolve_model_reference(&state, model).await?,
        _ => resolve_target(&state, model.as_deref()).await?,
    };

    // The upstream must see the *real* model id, not the router's prefix.
    if let Some(object) = body.as_object_mut() {
        object.insert("model".to_string(), json!(target.model));
    }

    forward_json(state, &target, "/embeddings", body).await
}

/// `POST /v1/images/generations` — OpenAI-compatible image proxy.
pub async fn image_generations(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Response> {
    let target = target_for_body(&state, &body).await?;
    let body = rewrite_model(&body, &target);
    forward_json(state, &target, "/images/generations", body).await
}

/// `POST /v1/audio/speech` — text-to-speech proxy.
pub async fn audio_speech(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Response> {
    let target = target_for_body(&state, &body).await?;
    let body = rewrite_model(&body, &target);
    forward_json(state, &target, "/audio/speech", body).await
}

/// `POST /v1/audio/transcriptions` — multipart pass-through.
pub async fn audio_transcriptions(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response> {
    let target = resolve_target(&state, None).await?;
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok());

    proxy_raw(state, &target, "/audio/transcriptions", content_type, body).await
}

/// `POST /v1/videos/generations` — async video job creation.
pub async fn video_generations(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Response> {
    let target = target_for_body(&state, &body).await?;
    let body = rewrite_model(&body, &target);
    forward_json(state, &target, "/videos/generations", body).await
}

/// `GET /v1/videos/{id}` — async video job polling pass-through.
pub async fn video_status(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Response> {
    let target = resolve_target(&state, None).await?;
    let proxy = MediaProxy::new();
    let response = proxy
        .get(&target, &format!("/videos/{id}"))
        .await?;
    passthrough(response).await
}

/// Picks the target named by the body's `model`, else the default connection.
async fn target_for_body(state: &AppState, body: &serde_json::Value) -> Result<ResolvedTarget> {
    let model = body
        .get("model")
        .and_then(|value| value.as_str())
        .map(str::to_string);

    match model {
        Some(model) if model.contains('/') => resolve_model_reference(state, &model).await,
        other => resolve_target(state, other.as_deref()).await,
    }
}

/// Replaces a router-prefixed model id with the upstream's real model id.
fn rewrite_model(body: &serde_json::Value, target: &ResolvedTarget) -> serde_json::Value {
    let mut body = body.clone();
    if let Some(object) = body.as_object_mut() {
        object.insert("model".to_string(), json!(target.model));
    }
    body
}

async fn forward_json(
    state: AppState,
    target: &ResolvedTarget,
    path: &str,
    body: serde_json::Value,
) -> Result<Response> {
    let _ = state;
    let proxy = MediaProxy::new();
    let response = proxy.post_json(target, path, &body).await?;
    passthrough(response).await
}

async fn proxy_raw(
    state: AppState,
    target: &ResolvedTarget,
    path: &str,
    content_type: Option<&str>,
    body: Bytes,
) -> Result<Response> {
    let _ = state;
    let proxy = MediaProxy::new();
    let response = proxy
        .post_raw(target, path, content_type, body.to_vec(), None)
        .await?;
    passthrough(response).await
}

/// Streams an upstream response back to the client unchanged.
async fn passthrough(response: reqwest::Response) -> Result<Response> {
    let status = StatusCode::from_u16(response.status().as_u16())
        .unwrap_or(StatusCode::BAD_GATEWAY);

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

/// `POST /v1/search` — SearXNG-style search passthrough.
pub async fn search(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Response> {
    let target = resolve_target(&state, None).await?;
    let proxy = MediaProxy::new();
    let response = proxy.post_json(&target, "/search", &body).await?;
    passthrough(response).await
}

/// `POST /v1/web/fetch` — fetches a URL server-side.
///
/// Any URL is fetched directly rather than through a configured connection, so
/// the private-address guard is what keeps this from becoming an SSRF pivot.
pub async fn web_fetch(Json(body): Json<serde_json::Value>) -> Result<Response> {
    let url = body
        .get("url")
        .and_then(|value| value.as_str())
        .ok_or_else(|| Error::BadRequest("url is required".to_string()))?;

    let parsed = url::Url::parse(url)
        .map_err(|error| Error::BadRequest(format!("invalid url: {error}")))?;

    if is_private_url(&parsed) {
        return Err(Error::BadRequest(
            "refusing to fetch a private or loopback address".to_string(),
        ));
    }

    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|error| Error::Internal(error.to_string()))?;

    let response = client
        .get(parsed)
        .send()
        .await
        .map_err(|error| Error::Upstream(error.to_string()))?;

    let status = StatusCode::from_u16(response.status().as_u16())
        .unwrap_or(StatusCode::BAD_GATEWAY);
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);

    let text = response
        .text()
        .await
        .map_err(|error| Error::Upstream(error.to_string()))?;

    let mut builder = Response::builder().status(status);
    if let Some(content_type) = content_type {
        builder = builder.header(axum::http::header::CONTENT_TYPE, content_type);
    }

    builder
        .body(axum::body::Body::from(text))
        .map_err(|error| Error::Internal(error.to_string()))
}

/// Rejects loopback, link-local, and RFC-1918 addresses.
pub fn is_private_url(url: &url::Url) -> bool {
    let Some(host) = url.host_str() else {
        return true;
    };

    // Hostname aliases for the local machine.
    if matches!(host, "localhost" | "localhost.localdomain") {
        return true;
    }

    match url.host() {
        Some(url::Host::Ipv4(address)) => {
            address.is_loopback()
                || address.is_private()
                || address.is_link_local()
                || address.is_unspecified()
        }
        Some(url::Host::Ipv6(address)) => {
            address.is_loopback()
                || address.is_unspecified()
                || address.is_unique_local()
                || address.is_unicast_link_local()
        }
        // A bare hostname could resolve anywhere; only literal IPs are checked.
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_addresses_are_rejected() {
        for url in [
            "http://localhost/admin",
            "http://127.0.0.1:8080/",
            "http://10.0.0.5/",
            "http://192.168.1.1/",
            "http://169.254.169.254/latest/meta-data/",
            "http://[::1]/",
        ] {
            let parsed = url::Url::parse(url).expect("valid url");
            assert!(is_private_url(&parsed), "{url} should be private");
        }
    }

    #[test]
    fn public_addresses_are_allowed() {
        for url in ["https://example.com/", "https://1.1.1.1/"] {
            let parsed = url::Url::parse(url).expect("valid url");
            assert!(!is_private_url(&parsed), "{url} should be allowed");
        }
    }
}
