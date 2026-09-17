//! `/v1/web/fetch`: a server-side fetch with a complete SSRF guard.

use std::net::{IpAddr, SocketAddr};

use axum::Json;
use axum::http::StatusCode;
use axum::response::Response;

use crate::error::{Error, Result};

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
