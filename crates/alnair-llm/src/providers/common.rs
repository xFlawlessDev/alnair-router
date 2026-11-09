use std::time::Duration;

use reqwest::header::HeaderMap;

use crate::model_config::ModelCostRates;
use crate::types::ChatError;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TokenCosts {
    pub(crate) input_usd: Option<f64>,
    pub(crate) output_usd: Option<f64>,
    /// Premium for reasoning tokens over the output rate, reported separately so
    /// the router can store a cost breakdown. `None` when unpriced.
    pub(crate) reasoning_usd: Option<f64>,
    /// The part of `input_usd` billed at the cache rate rather than the input
    /// rate. Reported separately because a fully cached prompt can be almost
    /// all of the input cost while looking like an ordinary input charge: on a
    /// 99% cache hit the dashboard would otherwise claim the tokens cost the
    /// full input rate. `None` when unpriced.
    pub(crate) cache_usd: Option<f64>,
}

pub(crate) fn normalize_base_url(base_url: &str) -> String {
    base_url.trim_end_matches('/').to_string()
}

pub(crate) fn retry_after_delay(headers: &HeaderMap) -> Option<Duration> {
    let value = headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim();
    let seconds = value.parse::<u64>().ok()?;
    Some(Duration::from_secs(seconds))
}

/// True when a response content type proves the body is an HTML page.
///
/// Only `text/html` counts. A gateway that serves its dashboard at the
/// configured `base_url` answers the API path with that page and a `200`, which
/// an SSE reader would otherwise consume as a stream with no events. A missing
/// or unusual content type is left alone: some relays mislabel a real event
/// stream, and dropping those would break working connections.
pub(crate) fn is_html_content_type(content_type: &str) -> bool {
    content_type
        .trim_start()
        .to_ascii_lowercase()
        .starts_with("text/html")
}

pub(crate) fn response_content_type(response: &reqwest::Response) -> Option<String> {
    response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)?
        .to_str()
        .ok()
        .map(str::to_string)
}

/// Describes a provider body that was not the JSON the caller asked for.
///
/// A gateway that serves its dashboard at the configured `base_url` answers the
/// API path with an HTML page and a `200`, so the failure reads as corrupt JSON
/// when the real fault is the missing API prefix in `base_url`. Naming the HTML
/// points at the fix; only a genuinely broken JSON body keeps the raw preview.
pub(crate) fn describe_non_json_body(body: &str) -> String {
    let trimmed = body.trim_start();
    if trimmed.is_empty() {
        return "the endpoint returned an empty body".to_string();
    }
    if trimmed.starts_with('<') {
        return "the endpoint answered an HTML page instead of JSON, so base_url \
                looks like a web UI root rather than the API root — it usually \
                needs to end with /v1"
            .to_string();
    }

    let compact = trimmed.split_whitespace().collect::<Vec<_>>().join(" ");
    let preview: String = compact.chars().take(200).collect();
    format!("unexpected body: {preview}")
}

pub(crate) fn is_retryable_status(status: reqwest::StatusCode) -> bool {
    // 529 is Anthropic's `overloaded_error`; every other provider uses 503.
    matches!(status.as_u16(), 408 | 429 | 500 | 502 | 503 | 504 | 529)
}

/// Absolute ceiling for transport retries, used as the `LlmStreamOptions`
/// default.
///
/// Transport retries cover a connection that never produced a response
/// (reset, refused, TLS failure). They are deliberately separate from status
/// retries: the router owns tier failover and credential rotation, and letting
/// both layers retry would multiply one client request into `tiers × keys ×
/// retries` upstream calls.
pub(crate) const PROVIDER_MAX_TRANSPORT_RETRIES: usize = 1;

/// Exponential backoff for provider retries: 500 ms doubling per zero-based
/// `attempt`, capped by `max_retry_delay_ms`. A `Retry-After` header, when
/// present, wins over this.
pub(crate) fn retry_delay(attempt: usize, max_retry_delay_ms: u64) -> Duration {
    let factor = 1u64 << attempt.min(6);
    Duration::from_millis(500u64.saturating_mul(factor).min(max_retry_delay_ms))
}

pub(crate) fn calculate_token_costs(
    rates: Option<&ModelCostRates>,
    input_tokens: Option<u64>,
    cached_read_tokens: Option<u64>,
    cached_write_tokens: Option<u64>,
    output_tokens: Option<u64>,
    reasoning_tokens: Option<u64>,
) -> TokenCosts {
    let Some(rates) = rates else {
        return TokenCosts {
            input_usd: None,
            output_usd: None,
            reasoning_usd: None,
            cache_usd: None,
        };
    };

    let uncached_input_tokens = input_tokens
        .unwrap_or(0)
        .saturating_sub(cached_read_tokens.unwrap_or(0))
        .saturating_sub(cached_write_tokens.unwrap_or(0));
    let cached_read_rate = rates
        .cache_read_per_million_usd
        .unwrap_or(rates.input_per_million_usd);
    let cached_write_rate = rates
        .cache_write_per_million_usd
        .unwrap_or(rates.input_per_million_usd);
    let cache_usd = input_tokens.map(|_| {
        (cached_read_tokens.unwrap_or(0) as f64 * cached_read_rate
            + cached_write_tokens.unwrap_or(0) as f64 * cached_write_rate)
            / 1_000_000.0
    });
    let input_usd = input_tokens.map(|_| {
        (uncached_input_tokens as f64 * rates.input_per_million_usd
            + cached_read_tokens.unwrap_or(0) as f64 * cached_read_rate
            + cached_write_tokens.unwrap_or(0) as f64 * cached_write_rate)
            / 1_000_000.0
    });

    let output_usd =
        output_tokens.map(|tokens| tokens as f64 / 1_000_000.0 * rates.output_per_million_usd);

    // Completion tokens already include reasoning tokens, so a dedicated rate
    // only contributes its premium over the output rate.
    let reasoning_usd = match (reasoning_tokens, rates.reasoning_per_million_usd) {
        (Some(tokens), Some(rate)) if tokens > 0 => {
            Some(tokens as f64 * (rate - rates.output_per_million_usd) / 1_000_000.0)
        }
        _ => None,
    };

    TokenCosts {
        input_usd,
        output_usd,
        reasoning_usd,
        cache_usd,
    }
}

/// The two retry budgets for one provider call.
///
/// Kept together so a provider cannot retry a status under the transport
/// allowance (which is what let one client request fan out into dozens of
/// upstream calls). [`RetryBudget::attempts`] is the hard loop bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RetryBudget {
    /// Retries after a retryable HTTP status (408/429/5xx/529).
    pub(crate) status: usize,
    /// Retries after a connection failure that produced no response.
    pub(crate) transport: usize,
}

impl RetryBudget {
    pub(crate) fn new(status: usize, transport: usize) -> Self {
        Self { status, transport }
    }

    /// Upper bound for the attempt loop: the first try plus both budgets.
    pub(crate) fn attempts(&self) -> usize {
        // Saturating so a pathological config cannot overflow into zero tries.
        1 + self.status.saturating_add(self.transport)
    }
}

pub(crate) fn parse_tool_arguments_strict(raw: &str) -> Result<serde_json::Value, ChatError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(serde_json::json!({}));
    }

    let value: serde_json::Value = serde_json::from_str(trimmed).map_err(|error| {
        ChatError::Provider(format!("provider returned malformed tool JSON: {error}"))
    })?;
    if value.is_object() {
        Ok(value)
    } else {
        Err(ChatError::Provider(
            "provider returned non-object tool JSON".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_base_url_removes_trailing_slash() {
        assert_eq!(
            normalize_base_url("https://api.example.com/"),
            "https://api.example.com"
        );
    }

    #[test]
    fn is_retryable_status_recognizes_rate_limit() {
        assert!(is_retryable_status(reqwest::StatusCode::TOO_MANY_REQUESTS));
    }

    #[test]
    fn is_retryable_status_rejects_bad_request() {
        assert!(!is_retryable_status(reqwest::StatusCode::BAD_REQUEST));
    }

    #[test]
    fn transport_retries_stay_small_because_the_router_owns_failover() {
        // The router retries across tiers and credentials; a large transport
        // budget here would multiply one client request into many upstream
        // calls, so this ceiling is part of the contract.
        assert_eq!(PROVIDER_MAX_TRANSPORT_RETRIES, 1);
    }

    #[test]
    fn retry_delay_grows_exponentially() {
        assert_eq!(retry_delay(0, 30_000), Duration::from_millis(500));
        assert_eq!(retry_delay(1, 30_000), Duration::from_secs(1));
        assert_eq!(retry_delay(2, 30_000), Duration::from_secs(2));
        assert_eq!(retry_delay(3, 30_000), Duration::from_secs(4));
    }

    #[test]
    fn retry_delay_respects_the_cap() {
        assert_eq!(retry_delay(3, 1_000), Duration::from_secs(1));
        assert_eq!(retry_delay(10, 30_000), Duration::from_secs(30));
    }

    #[test]
    fn calculate_token_costs_uses_cache_read_rate() {
        let rates = ModelCostRates {
            input_per_million_usd: 2.0,
            output_per_million_usd: 8.0,
            cache_read_per_million_usd: Some(0.5),
            cache_write_per_million_usd: None,
            reasoning_per_million_usd: None,
        };

        let costs = calculate_token_costs(Some(&rates), Some(100), Some(40), None, Some(10), None);

        assert_eq!(
            costs.input_usd,
            Some((60.0 * 2.0 + 40.0 * 0.5) / 1_000_000.0)
        );
        assert_eq!(costs.output_usd, Some(10.0 * 8.0 / 1_000_000.0));
    }

    #[test]
    fn calculate_token_costs_reports_the_cache_portion_of_input() {
        let rates = ModelCostRates {
            input_per_million_usd: 2.0,
            output_per_million_usd: 8.0,
            cache_read_per_million_usd: Some(0.5),
            cache_write_per_million_usd: None,
            reasoning_per_million_usd: None,
        };

        // 100 prompt tokens of which 40 are a cache read.
        let costs = calculate_token_costs(Some(&rates), Some(100), Some(40), None, Some(10), None);

        let cache = costs.cache_usd.expect("cache portion");
        assert!(
            (cache - 40.0 * 0.5 / 1_000_000.0).abs() < f64::EPSILON,
            "the cache row must be the cached tokens at the cache rate, got {cache}"
        );

        // The cache portion is a slice of the input total, never an addition to
        // it: input + output must still be the whole bill.
        let input = costs.input_usd.expect("input");
        assert!(
            cache <= input,
            "cache {cache} must not exceed input {input}"
        );
    }

    #[test]
    fn a_fully_cached_prompt_is_almost_all_cache_cost() {
        let rates = ModelCostRates {
            input_per_million_usd: 2.0,
            output_per_million_usd: 8.0,
            cache_read_per_million_usd: Some(0.5),
            cache_write_per_million_usd: None,
            reasoning_per_million_usd: None,
        };

        // The case that made the dashboard misread: 99% of the prompt cached,
        // so 96% of the input charge is cache rather than ordinary input.
        let costs =
            calculate_token_costs(Some(&rates), Some(10_000), Some(9_900), None, None, None);

        let input = costs.input_usd.expect("input");
        let cache = costs.cache_usd.expect("cache");
        assert!(
            cache / input > 0.95,
            "nearly the whole input charge is cache, got {cache} of {input}"
        );
    }

    #[test]
    fn unpriced_models_report_no_cost_at_all() {
        let costs = calculate_token_costs(None, Some(100), Some(40), None, Some(10), None);

        assert!(costs.input_usd.is_none());
        assert!(costs.output_usd.is_none());
        assert!(costs.reasoning_usd.is_none());
        assert!(costs.cache_usd.is_none());
    }

    #[test]
    fn calculate_token_costs_bills_reasoning_as_a_premium() {
        let rates = ModelCostRates {
            input_per_million_usd: 2.0,
            output_per_million_usd: 10.0,
            cache_read_per_million_usd: None,
            cache_write_per_million_usd: None,
            reasoning_per_million_usd: Some(15.0),
        };

        // 100 completion tokens include 40 reasoning tokens: the base output
        // rate covers them, and the premium is reported separately.
        let costs = calculate_token_costs(Some(&rates), Some(0), None, None, Some(100), Some(40));

        let output = costs.output_usd.expect("output cost");
        let expected_output = 100.0 * 10.0 / 1_000_000.0;
        assert!(
            (output - expected_output).abs() < f64::EPSILON * 10.0,
            "expected {expected_output}, got {output}"
        );

        let reasoning = costs.reasoning_usd.expect("reasoning premium");
        let expected_reasoning = 40.0 * (15.0 - 10.0) / 1_000_000.0;
        assert!(
            (reasoning - expected_reasoning).abs() < f64::EPSILON * 10.0,
            "expected {expected_reasoning}, got {reasoning}"
        );
    }

    #[test]
    fn calculate_token_costs_without_a_reasoning_rate_uses_output_only() {
        let rates = ModelCostRates {
            input_per_million_usd: 2.0,
            output_per_million_usd: 10.0,
            cache_read_per_million_usd: None,
            cache_write_per_million_usd: None,
            reasoning_per_million_usd: None,
        };

        let costs = calculate_token_costs(Some(&rates), Some(0), None, None, Some(100), Some(40));

        assert_eq!(costs.output_usd, Some(100.0 * 10.0 / 1_000_000.0));
        assert_eq!(costs.reasoning_usd, None);
    }

    #[test]
    fn parse_tool_arguments_strict_rejects_malformed_non_empty_json() {
        assert!(parse_tool_arguments_strict(r#"{"broken""#).is_err());
    }

    #[test]
    fn describe_non_json_body_points_at_the_base_url_when_html_answers() {
        let described = describe_non_json_body("<!doctype html><html><body>app</body></html>");

        assert!(described.contains("HTML page"), "got: {described}");
        assert!(described.contains("/v1"), "got: {described}");
        assert!(!described.contains("<html>"), "must not echo the page");
    }

    #[test]
    fn describe_non_json_body_summarizes_other_bodies() {
        assert!(describe_non_json_body("   ").contains("empty"));

        let described = describe_non_json_body("{\n  \"error\": \"nope\"\n}");
        assert_eq!(described, r#"unexpected body: { "error": "nope" }"#);
    }

    #[test]
    fn describe_non_json_body_caps_the_preview() {
        let described = describe_non_json_body(&"x".repeat(1_000));

        assert!(described.len() < 240, "got {} chars", described.len());
    }

    #[test]
    fn html_content_type_matches_case_and_parameters() {
        assert!(is_html_content_type("text/html"));
        assert!(is_html_content_type("text/html; charset=utf-8"));
        assert!(is_html_content_type("TEXT/HTML"));
    }

    #[test]
    fn html_content_type_ignores_streams_it_must_not_touch() {
        // A missing type and a mislabelled event stream both stay untouched:
        // dropping a real stream would break working connections.
        assert!(!is_html_content_type("text/event-stream"));
        assert!(!is_html_content_type("application/json"));
        assert!(!is_html_content_type("application/octet-stream"));
        assert!(!is_html_content_type(""));
    }
}
