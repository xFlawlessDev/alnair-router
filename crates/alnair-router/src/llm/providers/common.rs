use std::time::Duration;

use reqwest::header::HeaderMap;

use crate::llm::model_config::ModelCostRates;
use crate::llm::types::ChatError;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TokenCosts {
    pub(crate) input_usd: Option<f64>,
    pub(crate) output_usd: Option<f64>,
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

pub(crate) fn is_retryable_status(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 429 | 500 | 502 | 503 | 504)
}

pub(crate) const PROVIDER_MAX_RETRIES: usize = 10;

pub(crate) fn retry_delay(max_retry_delay_ms: Option<u64>) -> Duration {
    let delay = Duration::from_secs(30);
    max_retry_delay_ms.map_or(delay, |max| delay.min(Duration::from_millis(max)))
}

pub(crate) fn calculate_token_costs(
    rates: Option<&ModelCostRates>,
    input_tokens: Option<u64>,
    cached_read_tokens: Option<u64>,
    cached_write_tokens: Option<u64>,
    output_tokens: Option<u64>,
) -> TokenCosts {
    let Some(rates) = rates else {
        return TokenCosts {
            input_usd: None,
            output_usd: None,
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
    let input_usd = input_tokens.map(|_| {
        (uncached_input_tokens as f64 * rates.input_per_million_usd
            + cached_read_tokens.unwrap_or(0) as f64 * cached_read_rate
            + cached_write_tokens.unwrap_or(0) as f64 * cached_write_rate)
            / 1_000_000.0
    });
    let output_usd =
        output_tokens.map(|tokens| tokens as f64 / 1_000_000.0 * rates.output_per_million_usd);

    TokenCosts {
        input_usd,
        output_usd,
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
    fn retry_delay_defaults_to_thirty_seconds() {
        assert_eq!(retry_delay(None), Duration::from_secs(30));
    }

    #[test]
    fn retry_delay_respects_lower_maximum() {
        assert_eq!(retry_delay(Some(1_000)), Duration::from_secs(1));
    }

    #[test]
    fn calculate_token_costs_uses_cache_read_rate() {
        let rates = ModelCostRates {
            input_per_million_usd: 2.0,
            output_per_million_usd: 8.0,
            cache_read_per_million_usd: Some(0.5),
            cache_write_per_million_usd: None,
        };

        let costs = calculate_token_costs(Some(&rates), Some(100), Some(40), None, Some(10));

        assert_eq!(
            costs.input_usd,
            Some((60.0 * 2.0 + 40.0 * 0.5) / 1_000_000.0)
        );
        assert_eq!(costs.output_usd, Some(10.0 * 8.0 / 1_000_000.0));
    }

    #[test]
    fn parse_tool_arguments_strict_rejects_malformed_non_empty_json() {
        assert!(parse_tool_arguments_strict(r#"{"broken""#).is_err());
    }
}
