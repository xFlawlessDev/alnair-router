//! Proxied endpoints: embeddings, images, audio, video, search, web fetch.

use std::net::{IpAddr, SocketAddr};

use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
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
    let _permit = state.limiter.acquire(&target.connection_id).await?;
    let proxy = MediaProxy::new();
    let response = proxy.get(&target, &format!("/videos/{id}")).await?;
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
    let _permit = state.limiter.acquire(&target.connection_id).await?;
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
    let _permit = state.limiter.acquire(&target.connection_id).await?;
    let proxy = MediaProxy::new();
    let response = proxy
        .post_raw(target, path, content_type, body.to_vec(), None)
        .await?;
    passthrough(response).await
}

/// Streams an upstream response back to the client unchanged.
async fn passthrough(response: reqwest::Response) -> Result<Response> {
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

/// `POST /v1/search` — SearXNG-style search passthrough.
pub async fn search(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Response> {
    let target = resolve_target(&state, None).await?;
    let _permit = state.limiter.acquire(&target.connection_id).await?;
    let proxy = MediaProxy::new();
    let response = proxy.post_json(&target, "/search", &body).await?;
    passthrough(response).await
}

/// Maximum redirects followed by `/v1/web/fetch`.
const MAX_FETCH_REDIRECTS: usize = 5;

/// `POST /v1/web/fetch` — fetches a URL server-side.
///
/// Any URL is fetched directly rather than through a configured connection, so
/// the private-address guard is what keeps this from becoming an SSRF pivot.
/// Redirects are followed manually and every hop is re-validated; hostnames are
/// resolved up front and the connection is pinned to the validated addresses.
pub async fn web_fetch(Json(body): Json<serde_json::Value>) -> Result<Response> {
    let url = body
        .get("url")
        .and_then(|value| value.as_str())
        .ok_or_else(|| Error::BadRequest("url is required".to_string()))?;

    let mut parsed =
        url::Url::parse(url).map_err(|error| Error::BadRequest(format!("invalid url: {error}")))?;

    for _ in 0..=MAX_FETCH_REDIRECTS {
        let resolved = validate_fetch_url(&parsed).await?;
        let client = pinned_client(&parsed, &resolved)?;

        let response = client
            .get(parsed.clone())
            .send()
            .await
            .map_err(|error| Error::Upstream(error.to_string()))?;

        if response.status().is_redirection() {
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string);

            let Some(location) = location else {
                return render_fetch_response(response).await;
            };

            parsed = parsed.join(&location).map_err(|error| {
                Error::BadRequest(format!("invalid redirect location: {error}"))
            })?;
            continue;
        }

        return render_fetch_response(response).await;
    }

    Err(Error::BadRequest("too many redirects".to_string()))
}

/// Validates the scheme and host and resolves every candidate address so each
/// one can be checked before a connection is attempted.
async fn validate_fetch_url(url: &url::Url) -> Result<Vec<SocketAddr>> {
    match url.scheme() {
        "http" | "https" => {}
        other => {
            return Err(Error::BadRequest(format!(
                "unsupported scheme '{other}'; only http and https are allowed"
            )));
        }
    }

    let host = url
        .host_str()
        .ok_or_else(|| Error::BadRequest("url has no host".to_string()))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| Error::BadRequest("url has no port".to_string()))?;

    if is_localhost_name(host) {
        return Err(private_address_error());
    }

    match url.host() {
        Some(url::Host::Ipv4(address)) => {
            let address = IpAddr::V4(address);
            ensure_public(address)?;
            Ok(vec![SocketAddr::new(address, port)])
        }
        Some(url::Host::Ipv6(address)) => {
            let address = IpAddr::V6(address);
            ensure_public(address)?;
            Ok(vec![SocketAddr::new(address, port)])
        }
        _ => {
            let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
                .await
                .map_err(|error| Error::BadRequest(format!("cannot resolve '{host}': {error}")))?
                .collect();

            if addresses.is_empty() {
                return Err(Error::BadRequest(format!(
                    "'{host}' resolved to no addresses"
                )));
            }
            for address in &addresses {
                ensure_public(address.ip())?;
            }
            Ok(addresses)
        }
    }
}

fn ensure_public(address: IpAddr) -> Result<()> {
    if is_private_ip(address) {
        return Err(private_address_error());
    }
    Ok(())
}

fn private_address_error() -> Error {
    Error::BadRequest("refusing to fetch a private or loopback address".to_string())
}

fn is_localhost_name(host: &str) -> bool {
    matches!(host, "localhost" | "localhost.localdomain")
}

/// Rejects loopback, RFC-1918, link-local, unique-local, unspecified and
/// broadcast addresses — including IPv4-mapped IPv6 forms of those ranges.
pub(crate) fn is_private_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            address.is_loopback()
                || address.is_private()
                || address.is_link_local()
                || address.is_unspecified()
                || address.is_broadcast()
        }
        IpAddr::V6(address) => {
            address.is_loopback()
                || address.is_unspecified()
                || address.is_unique_local()
                || address.is_unicast_link_local()
                || address
                    .to_ipv4_mapped()
                    .is_some_and(|mapped| is_private_ip(IpAddr::V4(mapped)))
        }
    }
}

/// Builds a client that never follows redirects and, for hostnames, connects
/// only to the addresses that passed validation.
fn pinned_client(url: &url::Url, resolved: &[SocketAddr]) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(30));

    let is_hostname = url
        .host()
        .is_some_and(|host| !matches!(host, url::Host::Ipv4(_) | url::Host::Ipv6(_)));
    if is_hostname && let Some(host) = url.host_str() {
        builder = builder.resolve_to_addrs(host, resolved);
    }

    builder
        .build()
        .map_err(|error| Error::Internal(error.to_string()))
}

/// Streams a fetched response back to the client.
async fn render_fetch_response(response: reqwest::Response) -> Result<Response> {
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    fn ip(value: &str) -> IpAddr {
        value.parse().expect("valid ip")
    }

    #[test]
    fn private_ip_ranges_are_rejected() {
        for address in [
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)),
            IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1)),
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
            IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254)),
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            IpAddr::V4(Ipv4Addr::BROADCAST),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
            IpAddr::V6(Ipv6Addr::UNSPECIFIED),
            ip("fd00::1"),
            ip("fe80::1"),
            ip("::ffff:127.0.0.1"),
        ] {
            assert!(is_private_ip(address), "{address} should be private");
        }
    }

    #[test]
    fn public_ips_are_allowed() {
        for address in ["1.1.1.1", "8.8.8.8", "2606:4700:4700::1111"] {
            assert!(
                !is_private_ip(ip(address)),
                "{address} should be considered public"
            );
        }
    }

    #[tokio::test]
    async fn validate_fetch_url_rejects_private_targets_and_schemes() {
        for url in [
            "file:///etc/passwd",
            "http://localhost/admin",
            "http://127.0.0.1:8080/",
            "http://169.254.169.254/latest/meta-data/",
            "http://[::1]/",
            "http://[::ffff:127.0.0.1]/",
        ] {
            let parsed = url::Url::parse(url).expect("valid url");
            assert!(
                validate_fetch_url(&parsed).await.is_err(),
                "{url} should be rejected"
            );
        }
    }

    #[tokio::test]
    async fn validate_fetch_url_allows_public_literal_ips() {
        for url in ["https://1.1.1.1/", "https://[2606:4700:4700::1111]/"] {
            let parsed = url::Url::parse(url).expect("valid url");
            let resolved = validate_fetch_url(&parsed).await.expect("public ip");
            assert!(!resolved.is_empty(), "{url} should resolve to itself");
        }
    }
}
