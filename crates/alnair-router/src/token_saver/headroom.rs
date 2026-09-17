//! Headroom: optional deeper compression through an external proxy.
//!
//! Headroom is a separate open-source compression proxy. The router posts its
//! messages to the proxy's `POST /v1/compress` endpoint and uses whatever comes
//! back; routing, auth, fallback and usage tracking stay exactly the same.
//!
//! ```text
//! client → router → headroom /v1/compress → router → provider
//! ```
//!
//! Two contract details drive the code here:
//!
//! - **Fail-open.** A proxy that is down, slow, or confused leaves the request
//!   untouched. Nothing in this module can fail a request.
//! - **Compression may be skipped.** Headroom answers `200` with the original
//!   messages plus `compression_skipped: true` when it times out internally, so
//!   a `200` alone is not proof that anything happened.
//!
//! One deployment gotcha, surfaced in the error messages an operator sees:
//! `/v1/compress` is **loopback-only** on the proxy side and answers non-loopback
//! callers with `404` on purpose. Both the client IP and the inbound `Host`
//! header must name loopback, so a container or remote URL will not work without
//! `HEADROOM_COMPRESS_ALLOW_REMOTE=1` on the proxy.

use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::HeadroomSettings;
use crate::upstream::chat_backend::RouterMessage;

/// A saving smaller than this is treated as noise rather than a result.
const PHANTOM_SHRINK_RATIO: f64 = 0.05;

/// What a successful compression did.
pub struct HeadroomOutcome {
    pub messages: Vec<RouterMessage>,
    pub tokens_saved: u64,
}

#[derive(Debug, Serialize)]
struct CompressRequest<'a> {
    messages: &'a [RouterMessage],
    /// Required by Headroom: it selects the tokenizer and context limit.
    model: &'a str,
}

#[derive(Debug, Deserialize)]
struct CompressResponse {
    #[serde(default)]
    messages: Option<Vec<RouterMessage>>,
    #[serde(default)]
    tokens_saved: Option<u64>,
    /// Set when Headroom decided not to compress (usually an internal timeout),
    /// in which case a `200` carries the original messages.
    #[serde(default)]
    compression_skipped: bool,
}

/// Compresses `messages` through the proxy, or explains why it did not.
///
/// Returning `Err` is the normal fail-open path: the caller keeps its original
/// messages and records the reason.
pub async fn compress(
    settings: &HeadroomSettings,
    messages: Vec<RouterMessage>,
    model: &str,
) -> std::result::Result<HeadroomOutcome, String> {
    if settings.url.trim().is_empty() {
        return Err("no proxy URL configured".to_string());
    }

    let url = format!("{}/v1/compress", settings.url.trim_end_matches('/'));
    let body = CompressRequest {
        messages: &messages,
        model,
    };

    let bytes_before = serde_json::to_string(&body.messages)
        .map(|json| json.len())
        .unwrap_or(0);

    let response = match send(&url, &body, settings.timeout_ms).await {
        Ok(response) => response,
        Err(reason) => return Err(describe_failure(&reason, &url)),
    };

    if !response.status().is_success() {
        return Err(describe_failure(
            &format!("HTTP {}", response.status()),
            &url,
        ));
    }

    let text = response
        .text()
        .await
        .map_err(|error| format!("unreadable response: {error}"))?;
    let parsed: CompressResponse = serde_json::from_str(&text)
        .map_err(|error| format!("unexpected response body: {error}"))?;

    if parsed.compression_skipped {
        return Ok(HeadroomOutcome {
            messages,
            tokens_saved: 0,
        });
    }

    let Some(compressed) = parsed.messages else {
        return Ok(HeadroomOutcome {
            messages,
            tokens_saved: 0,
        });
    };

    // The conversation must keep its shape. The reference implementation runs a
    // normalizer after compression to repair tool_call/tool_result pairs; we
    // take the stricter route and reject any result that changed the message
    // count, so a compressed payload can never reach a provider malformed.
    if compressed.len() != messages.len() {
        tracing::warn!(
            before = messages.len(),
            after = compressed.len(),
            "headroom changed the message count; keeping the original request"
        );
        return Ok(HeadroomOutcome {
            messages,
            tokens_saved: 0,
        });
    }

    let reported = parsed.tokens_saved.unwrap_or(0);
    if reported == 0 {
        return Ok(HeadroomOutcome {
            messages: compressed,
            tokens_saved: 0,
        });
    }

    let bytes_after = serde_json::to_string(&compressed)
        .map(|json| json.len())
        .unwrap_or(bytes_before);
    if is_phantom(bytes_before, bytes_after) {
        // Headroom reports a saving but the payload barely moved, so the
        // provider would still bill close to the original.
        tracing::warn!(
            reported,
            bytes_before,
            bytes_after,
            "headroom reported a saving without shrinking the payload; not counting it"
        );
        return Ok(HeadroomOutcome {
            messages: compressed,
            tokens_saved: 0,
        });
    }

    Ok(HeadroomOutcome {
        messages: compressed,
        tokens_saved: reported,
    })
}

/// Sends the request with the configured timeout.
async fn send(
    url: &str,
    body: &CompressRequest<'_>,
    timeout_ms: u64,
) -> std::result::Result<reqwest::Response, String> {
    let client = Client::builder()
        .timeout(Duration::from_millis(timeout_ms.max(1)))
        .build()
        .map_err(|error| error.to_string())?;

    client
        .post(url)
        .json(body)
        .send()
        .await
        .map_err(|error| error.to_string())
}

/// True when a reported saving is not backed by a meaningfully smaller payload.
fn is_phantom(bytes_before: usize, bytes_after: usize) -> bool {
    if bytes_before == 0 || bytes_after == 0 {
        return false;
    }

    bytes_after >= (bytes_before as f64 * (1.0 - PHANTOM_SHRINK_RATIO)) as usize
}

/// Turns a transport/HTTP failure into something an operator can act on.
fn describe_failure(reason: &str, url: &str) -> String {
    if reason.contains("404") {
        return format!(
            "proxy at {url} answered 404 — /v1/compress is loopback-only, so a remote or \
             container URL looks like a missing route (set HEADROOM_COMPRESS_ALLOW_REMOTE=1 \
             on the proxy to allow it)"
        );
    }

    format!("proxy at {url} unreachable ({reason})")
}

/// Result of probing a Headroom proxy for the Test connection button.
#[derive(Debug, Clone, Serialize)]
pub struct HeadroomProbeResult {
    pub ok: bool,
    pub message: String,
    pub latency_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ReadyzResponse {
    #[serde(default)]
    version: Option<String>,
}

/// Probes `GET /readyz`, the endpoint Headroom's own container healthcheck uses.
///
/// A `503` counts as reachable: it means the proxy answered but is not ready
/// yet, which is materially different from an unreachable proxy and is exactly
/// what an operator needs to be told.
pub async fn probe_headroom(url: &str, timeout_ms: u64) -> HeadroomProbeResult {
    let started = std::time::Instant::now();
    let trimmed = url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return HeadroomProbeResult {
            ok: false,
            message: "no proxy URL configured".to_string(),
            latency_ms: 0,
            version: None,
        };
    }

    let endpoint = format!("{trimmed}/readyz");
    let client = match Client::builder()
        .timeout(Duration::from_millis(timeout_ms.max(1)))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            return HeadroomProbeResult {
                ok: false,
                message: format!("cannot build http client: {error}"),
                latency_ms: started.elapsed().as_millis() as u64,
                version: None,
            };
        }
    };

    let response = match client.get(&endpoint).send().await {
        Ok(response) => response,
        Err(error) => {
            return HeadroomProbeResult {
                ok: false,
                message: describe_failure(&error.to_string(), trimmed),
                latency_ms: started.elapsed().as_millis() as u64,
                version: None,
            };
        }
    };

    let latency_ms = started.elapsed().as_millis() as u64;
    let status = response.status();
    if !status.is_success() && status.as_u16() != 503 {
        return HeadroomProbeResult {
            ok: false,
            message: describe_failure(&format!("HTTP {status}"), trimmed),
            latency_ms,
            version: None,
        };
    }

    let version = response
        .json::<ReadyzResponse>()
        .await
        .ok()
        .and_then(|body| body.version);

    let ready = status.is_success();
    let label = match &version {
        Some(version) => format!("headroom {version}"),
        None => "headroom".to_string(),
    };

    HeadroomProbeResult {
        ok: true,
        message: if ready {
            format!("connected ({label})")
        } else {
            format!("{label} reachable but not ready yet ({status})")
        },
        latency_ms,
        version,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phantom_detection_requires_a_real_shrink() {
        // 4% smaller: inside the noise band, reported savings are not trusted.
        assert!(is_phantom(10_000, 9_600));
        // 40% smaller: a real result.
        assert!(!is_phantom(10_000, 6_000));
        // Degenerate inputs must not be flagged.
        assert!(!is_phantom(0, 0));
        assert!(!is_phantom(1_000, 0));
    }

    #[test]
    fn a_404_explains_the_loopback_rule() {
        let message = describe_failure("HTTP 404", "http://headroom:8787");
        assert!(message.contains("loopback-only"), "{message}");
        assert!(
            message.contains("HEADROOM_COMPRESS_ALLOW_REMOTE"),
            "{message}"
        );
    }

    #[test]
    fn other_failures_name_the_url() {
        let message = describe_failure("connection refused", "http://localhost:8787");
        assert!(message.contains("http://localhost:8787"), "{message}");
        assert!(message.contains("unreachable"), "{message}");
    }
}
