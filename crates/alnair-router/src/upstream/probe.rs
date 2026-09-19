//! Administrative upstream probes: model listing and connectivity tests.
//!
//! These call the upstream's `/models` endpoint directly with the connection's
//! decrypted credentials. They are deliberately separate from the chat
//! executor: an admin probe is allowed to fail loudly, and it never falls over
//! to another tier.

use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::db::repos::connections::Connection;
use crate::error::{Error, Result};

const PROBE_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const PROBE_TIMEOUT: Duration = Duration::from_secs(15);

/// One model offered by an upstream.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UpstreamModel {
    pub id: String,
    /// Human-friendly label (`display_name` when the provider sends one).
    pub name: String,
}

/// Result of a successful probe.
#[derive(Debug)]
pub struct ProbeOutcome {
    pub models: Vec<UpstreamModel>,
    pub latency_ms: u64,
    /// False when the provider does not publish a `/models` endpoint and the
    /// outcome is inferred from a chat-route liveness probe instead.
    pub enumerable: bool,
}

#[derive(Debug, Deserialize)]
struct ModelsEnvelope {
    #[serde(default)]
    data: Vec<UpstreamModelRaw>,
}

#[derive(Debug, Deserialize)]
struct UpstreamModelRaw {
    id: String,
    #[serde(default)]
    display_name: Option<String>,
}

/// Fetches `GET {base_url}/models` from the connection's upstream.
///
/// When the plain `base_url` answers 404 while `{base_url}/v1` works, the
/// error explains the missing `/v1` instead of dumping the upstream's HTML.
/// When the upstream does not publish a `/models` endpoint at all, the probe
/// falls back to a chat-route liveness check and reports the connection as
/// reachable but not model-enumerable.
pub async fn fetch_models(connection: &Connection) -> Result<ProbeOutcome> {
    let base = connection.base_url.trim_end_matches('/');

    // Command Code serves its model list under the Provider API path; a plan
    // without API access answers 403 there and still routes through the CLI
    // transport, so the error is reported as-is. Older connections carry that
    // path in `base_url` already, hence the strip.
    if connection.provider_type == "command-code" {
        let host = base.strip_suffix("/provider/v1").unwrap_or(base);
        return probe_url(connection, &format!("{host}/provider/v1/models")).await;
    }

    let primary = format!("{base}/models");

    match probe_url(connection, &primary).await {
        Ok(outcome) => Ok(outcome),
        Err(error) => {
            if let Some(hint) = version_hint(connection, base, &error).await {
                return Err(Error::Upstream(hint));
            }
            if let Some(outcome) = chat_liveness_probe(connection, base, &error).await {
                return Ok(outcome);
            }
            Err(error)
        }
    }
}

/// Applies the connection's credential plus the Anthropic version and custom
/// headers to an outgoing probe request.
///
/// Mirrors `media.rs::upstream_headers`: an Anthropic-native upstream takes
/// `x-api-key` unless the connection is flagged `bearer`, which is how an
/// OAuth/subscription session token authenticates. A probe that ignored
/// `auth_style` would report a working `bearer` connection as broken.
fn apply_auth(
    mut request: reqwest::RequestBuilder,
    connection: &Connection,
) -> reqwest::RequestBuilder {
    if let Some(api_key) = connection
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty())
    {
        request = if connection.provider_type == "anthropic-native"
            && connection.auth_style != "bearer"
        {
            request.header("x-api-key", api_key)
        } else {
            request.bearer_auth(api_key)
        };
    }
    if connection.provider_type == "anthropic-native" {
        request = request.header("anthropic-version", "2023-06-01");
    }
    for (name, value) in connection.headers() {
        request = request.header(name, value);
    }
    request
}

/// Probes one URL and parses a models envelope.
async fn probe_url(connection: &Connection, url: &str) -> Result<ProbeOutcome> {
    let client = reqwest::Client::builder()
        .connect_timeout(PROBE_CONNECT_TIMEOUT)
        .timeout(PROBE_TIMEOUT)
        .build()
        .map_err(|error| Error::Internal(error.to_string()))?;

    let request = apply_auth(client.get(url), connection);

    let started = Instant::now();
    let response = request
        .send()
        .await
        .map_err(|error| Error::Upstream(format!("GET {url} failed: {error}")))?;
    let latency_ms = started.elapsed().as_millis() as u64;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| Error::Upstream(format!("failed to read upstream body: {error}")))?;

    if !status.is_success() {
        return Err(Error::Upstream(format!(
            "GET {url} returned {status}: {}",
            summarize_body(&body)
        )));
    }

    let envelope: ModelsEnvelope = serde_json::from_str(&body).map_err(|error| {
        Error::Upstream(format!(
            "GET {url} returned an unexpected models payload: {error}"
        ))
    })?;

    let models = envelope
        .data
        .into_iter()
        .map(|raw| UpstreamModel {
            name: raw.display_name.unwrap_or_else(|| raw.id.clone()),
            id: raw.id,
        })
        .collect();

    Ok(ProbeOutcome {
        models,
        latency_ms,
        enumerable: true,
    })
}

/// One extra probe against `{base}/v1/models` when the configured base 404s.
async fn version_hint(connection: &Connection, base: &str, error: &Error) -> Option<String> {
    if base.ends_with("/v1") || !error.to_string().contains("404") {
        return None;
    }

    let versioned = format!("{base}/v1/models");
    if probe_url(connection, &versioned).await.is_err() {
        return None;
    }

    Some(format!(
        "GET {base}/models returned 404, but {versioned} answered — \
         update the connection's base_url to '{base}/v1' (the router appends \
         /chat/completions to base_url)"
    ))
}

/// Falls back to a chat-route liveness probe when `/models` is not served.
///
/// Some OpenAI-compatible providers (e.g. CodeBuddy Intl) only expose a
/// `/chat/completions` endpoint and return 404 for `/models`. When the models
/// probe 404s we issue an authenticated POST to the chat route with a minimal
/// request; a 401/403/4xx that reaches the route proves the upstream is live,
/// so we report it reachable but not model-enumerable rather than failing.
async fn chat_liveness_probe(
    connection: &Connection,
    base: &str,
    models_error: &Error,
) -> Option<ProbeOutcome> {
    let error_str = models_error.to_string();
    if !error_str.contains("404") {
        return None;
    }
    // If the /models 404 body looked like HTML, the base_url is almost certainly
    // a web UI rather than the API root. Don't paper over a misconfiguration
    // with a chat-route probe — surface the HTML hint instead so the operator
    // can correct the URL.
    if error_str.contains("an HTML page") {
        return None;
    }

    let chat_path = chat_route_path(connection.provider_type.as_str());
    let url = format!("{base}{chat_path}");

    let client = reqwest::Client::builder()
        .connect_timeout(PROBE_CONNECT_TIMEOUT)
        .timeout(PROBE_TIMEOUT)
        .build()
        .map_err(|error| Error::Internal(error.to_string()))
        .ok()?;

    let mut request = client.post(&url).json(&serde_json::json!({
        "model": "openai/gpt-oss-120b",
        "messages": [{ "role": "user", "content": "ping" }],
        "max_tokens": 1,
        "stream": false,
    }));

    request = apply_auth(request, connection);

    let started = Instant::now();
    let response = request.send().await.ok()?;
    let latency_ms = started.elapsed().as_millis() as u64;
    let status = response.status();

    // Liveness means the chat route itself exists. Only a 404 proves it does
    // not — that is a wrong base_url and must fail loudly. Everything else
    // (401/403, 400/422, 429, even a 5xx) means the request reached a handler:
    // CodeBuddy Intl answers 504 for a rejected key, so treating 5xx as "not
    // live" would re-create the false negative this fallback exists to fix.
    if status == reqwest::StatusCode::NOT_FOUND {
        return None;
    }

    Some(ProbeOutcome {
        models: Vec::new(),
        latency_ms,
        enumerable: false,
    })
}

/// Appended to the connection base URL to reach its chat endpoint.
fn chat_route_path(provider_type: &str) -> &'static str {
    match provider_type {
        "anthropic-native" => "/messages",
        _ => "/chat/completions",
    }
}

/// Describes an error body without dumping HTML at the operator.
fn summarize_body(body: &str) -> String {
    let trimmed = body.trim_start();
    if trimmed.starts_with('<') {
        return "an HTML page (base_url looks like a web UI, not the API root — \
                it usually needs to end with /v1)"
            .to_string();
    }

    let compact = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.is_empty() {
        return "an empty body".to_string();
    }
    compact.chars().take(200).collect()
}

/// True when `model` is offered by the upstream (case-insensitive exact match).
pub fn model_available(models: &[UpstreamModel], model: &str) -> bool {
    models
        .iter()
        .any(|candidate| candidate.id.eq_ignore_ascii_case(model))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn models() -> Vec<UpstreamModel> {
        vec![
            UpstreamModel {
                id: "gpt-4o".to_string(),
                name: "GPT-4o".to_string(),
            },
            UpstreamModel {
                id: "gpt-4o-mini".to_string(),
                name: "gpt-4o-mini".to_string(),
            },
        ]
    }

    #[test]
    fn model_available_matches_exactly_and_case_insensitively() {
        assert!(model_available(&models(), "gpt-4o"));
        assert!(model_available(&models(), "GPT-4O"));
        assert!(!model_available(&models(), "gpt-4o-mini-2024"));
        assert!(!model_available(&models(), "claude"));
    }

    #[test]
    fn summarize_body_hides_html_and_compacts_json() {
        let html = summarize_body("<!DOCTYPE html><html><body>page</body></html>");
        assert!(html.contains("HTML page"), "unexpected summary: {html}");

        assert_eq!(
            summarize_body("{\n  \"error\": \"nope\"\n}"),
            "{ \"error\": \"nope\" }"
        );

        assert!(summarize_body("   ").contains("empty"));
    }

    fn connection(provider_type: &str, auth_style: &str) -> Connection {
        Connection {
            id: "c1".to_string(),
            name: "test".to_string(),
            provider_type: provider_type.to_string(),
            base_url: "https://api.example.com/v1".to_string(),
            api_key: Some("secret-key".to_string()),
            custom_headers: "{}".to_string(),
            enabled: 1,
            connect_timeout_ms: None,
            idle_timeout_ms: None,
            pricing_model: None,
            cache_retention: "none".to_string(),
            auth_style: auth_style.to_string(),
            provider_id: None,
            extra_keys: Vec::new(),
            account_count: 0,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    /// Builds the request so the real header map can be inspected.
    fn headers_of(provider_type: &str, auth_style: &str) -> reqwest::header::HeaderMap {
        apply_auth(
            reqwest::Client::new().get("https://api.example.com/v1/models"),
            &connection(provider_type, auth_style),
        )
        .build()
        .expect("buildable request")
        .headers()
        .clone()
    }

    #[test]
    fn probe_uses_x_api_key_for_anthropic_native() {
        let headers = headers_of("anthropic-native", "api_key");

        assert_eq!(headers.get("x-api-key").unwrap(), "secret-key");
        assert!(headers.get("authorization").is_none());
    }

    #[test]
    fn probe_honours_the_bearer_auth_style() {
        let headers = headers_of("anthropic-native", "bearer");

        assert_eq!(headers.get("authorization").unwrap(), "Bearer secret-key");
        assert!(headers.get("x-api-key").is_none());
    }
}
