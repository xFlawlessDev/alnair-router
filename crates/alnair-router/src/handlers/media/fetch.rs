//! `/v1/web/fetch`: a server-side fetch with a complete SSRF guard, plus an
//! optional provider-backed path for pages a plain HTTP fetch cannot render.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::Json;
use axum::http::StatusCode;
use axum::response::Response;

use crate::error::{Error, Result};
use crate::model::ResolvedTarget;
use crate::upstream::media::MediaProxy;

/// Maximum redirects followed by `/v1/web/fetch`.
const MAX_FETCH_REDIRECTS: usize = 5;

/// A provider extract request: the URL to read and the format to return.
pub(super) struct ProviderFetch<'a> {
    pub target: &'a ResolvedTarget,
    /// Path appended to the connection's base URL, e.g. `/v1/scrape`.
    pub path: &'a str,
    pub url: &'a str,
    pub format: &'a str,
}

/// Forwards a fetch to a connection's own extract endpoint.
///
/// The request body is the one shape the major extract providers accept —
/// `{ "url": …, "format": … }` — and the reply is normalized into the same
/// envelope `/v1/web/fetch` returns locally, so a client can switch between the
/// two modes without changing how it reads the response.
pub(super) async fn provider_fetch(request: ProviderFetch<'_>) -> Result<Response> {
    let body = serde_json::json!({
        "url": request.url,
        "format": request.format,
    });

    let response = MediaProxy::new()
        .post_json(request.target, request.path, &body)
        .await?;

    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|error| Error::Upstream(format!("extract body: {error}")))?;

    if !status.is_success() {
        // Surface the provider's own message; a bare status hides whether the
        // page was blocked, the key was rejected or the path was wrong.
        let text = String::from_utf8_lossy(&bytes);
        return Err(Error::Upstream(format!(
            "extract endpoint returned {status}: {}",
            text.trim()
        )));
    }

    let parsed: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| Error::Upstream(format!("extract body is not JSON: {error}")))?;

    let envelope = normalize_extract_response(&parsed, request.url, request.format);
    let payload =
        serde_json::to_vec(&envelope).map_err(|error| Error::Internal(error.to_string()))?;

    Response::builder()
        .status(StatusCode::OK)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(payload))
        .map_err(|error| Error::Internal(error.to_string()))
}

/// Maps an extract reply onto `{ url, title, content: { format, text, length } }`.
///
/// Providers disagree on where the text lives: Firecrawl nests it under
/// `data.markdown`/`data.html`/`data.text`, Tavily uses `results[0].raw_content`,
/// Exa `results[0].text`, and Jina returns raw text. An unrecognized shape
/// keeps the whole upstream object under `raw` rather than pretending the page
/// was empty.
fn normalize_extract_response(
    body: &serde_json::Value,
    url: &str,
    format: &str,
) -> serde_json::Value {
    let text = extract_text(body);
    let title = body
        .get("data")
        .and_then(|data| data.get("metadata"))
        .and_then(|metadata| metadata.get("title"))
        .and_then(serde_json::Value::as_str)
        .or_else(|| body.get("title").and_then(serde_json::Value::as_str))
        .or_else(|| {
            body.get("results")
                .and_then(serde_json::Value::as_array)
                .and_then(|results| results.first())
                .and_then(|first| first.get("title"))
                .and_then(serde_json::Value::as_str)
        })
        .map(str::to_string);

    match text {
        Some(text) => serde_json::json!({
            "url": url,
            "title": title,
            "content": {
                "format": format,
                "text": text,
                "length": text.len(),
            },
        }),
        None => serde_json::json!({
            "url": url,
            "title": title,
            "content": { "format": format, "text": "", "length": 0 },
            "raw": body,
        }),
    }
}

/// Finds the page text in whichever shape the provider used.
fn extract_text(body: &serde_json::Value) -> Option<String> {
    let data = body.get("data");
    let first_result = || {
        body.get("results")
            .and_then(serde_json::Value::as_array)
            .and_then(|results| results.first())
    };

    [
        data.and_then(|data| data.get("markdown")),
        data.and_then(|data| data.get("html")),
        data.and_then(|data| data.get("text")),
        first_result().and_then(|first| first.get("raw_content")),
        first_result().and_then(|first| first.get("text")),
        body.get("content"),
        body.get("text"),
    ]
    .into_iter()
    .flatten()
    .filter_map(serde_json::Value::as_str)
    .find(|text| !text.trim().is_empty())
    .map(str::to_string)
}

/// `POST /v1/web/fetch` — fetches a URL server-side.
///
/// Without a `model`, the router fetches the URL itself, so the private-address
/// guard is what keeps this from becoming an SSRF pivot. Redirects are followed
/// manually and every hop is re-validated; hostnames are resolved up front and
/// the connection is pinned to the validated addresses.
///
/// A `model` naming a connection whose `web_fetch_mode` is `provider` forwards
/// the URL to that connection's extract endpoint instead — the way to reach
/// pages that need JavaScript rendering. See [`provider_fetch`].
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
pub(super) async fn validate_fetch_url(url: &url::Url) -> Result<Vec<SocketAddr>> {
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

/// Rejects loopback, RFC-1918, link-local, unique-local, unspecified,
/// broadcast and CGNAT addresses — including IPv4-mapped IPv6 forms of those
/// ranges.
///
/// `100.64.0.0/10` is carrier-grade NAT, but some cloud metadata proxies sit
/// inside it, so it is not a safe fetch target either.
pub(crate) fn is_private_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            address.is_loopback()
                || address.is_private()
                || address.is_link_local()
                || address.is_unspecified()
                || address.is_broadcast()
                || is_cgnat(address)
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

/// True for `100.64.0.0/10`, the shared address space RFC 6598 reserves for
/// carrier-grade NAT.
fn is_cgnat(address: Ipv4Addr) -> bool {
    let octets = address.octets();
    octets[0] == 100 && (64..128).contains(&octets[1])
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

    #[test]
    fn firecrawl_style_extract_is_normalized() {
        let body = serde_json::json!({
            "success": true,
            "data": {
                "markdown": "# Title\n\nBody text.",
                "metadata": { "title": "Title" }
            }
        });

        let envelope = normalize_extract_response(&body, "https://example.com", "markdown");
        assert_eq!(envelope["title"], "Title");
        assert_eq!(envelope["content"]["text"], "# Title\n\nBody text.");
        assert_eq!(envelope["content"]["format"], "markdown");
        assert_eq!(envelope["content"]["length"], "# Title\n\nBody text.".len());
    }

    #[test]
    fn tavily_and_exa_result_shapes_are_normalized() {
        let tavily = serde_json::json!({ "results": [{ "raw_content": "page body" }] });
        let envelope = normalize_extract_response(&tavily, "https://a", "markdown");
        assert_eq!(envelope["content"]["text"], "page body");

        let exa = serde_json::json!({ "results": [{ "text": "exa body", "title": "T" }] });
        let envelope = normalize_extract_response(&exa, "https://b", "text");
        assert_eq!(envelope["content"]["text"], "exa body");
        assert_eq!(envelope["title"], "T");
    }

    #[test]
    fn an_unrecognized_extract_shape_keeps_the_raw_body() {
        let body = serde_json::json!({ "unexpected": { "shape": true } });
        let envelope = normalize_extract_response(&body, "https://c", "markdown");

        assert_eq!(envelope["content"]["length"], 0);
        assert_eq!(envelope["raw"]["unexpected"]["shape"], true);
    }
}
