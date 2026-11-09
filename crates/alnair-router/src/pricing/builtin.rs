//! Built-in price table, used when neither a dashboard override nor a crawled
//! catalog has an entry for a model.
//!
//! Prices are USD per million tokens. Publishers change them without notice, so
//! this is a fallback, not a source of truth: once the pricing sync runs (or an
//! override is saved) the stored row wins. Verified 2026-11-09 against
//! OpenAI's published list prices.
//!
//! The table lives here rather than in the provider crate because pricing is a
//! router policy, not provider behaviour: an upstream never needs to know what
//! a token costs.

use crate::pricing::Price;

/// Known list prices for widely used chat models.
///
/// `reasoning_per_million_usd` is a premium over the output rate, because
/// completion tokens already include reasoning tokens.
pub fn price(model_id: &str) -> Option<Price> {
    let price = match model_id {
        "gpt-4o" | "gpt-4o-2024-11-20" => Price {
            input_per_million_usd: 2.50,
            output_per_million_usd: 10.00,
            cache_read_per_million_usd: Some(1.25),
            cache_write_per_million_usd: None,
            reasoning_per_million_usd: Some(15.00),
        },
        "gpt-4o-mini" | "gpt-4o-mini-2024-07-18" => Price {
            input_per_million_usd: 0.15,
            output_per_million_usd: 0.60,
            cache_read_per_million_usd: Some(0.075),
            cache_write_per_million_usd: None,
            reasoning_per_million_usd: Some(0.90),
        },
        "gpt-4.1" => Price {
            input_per_million_usd: 2.00,
            output_per_million_usd: 8.00,
            cache_read_per_million_usd: Some(0.50),
            cache_write_per_million_usd: None,
            reasoning_per_million_usd: Some(12.00),
        },
        "gpt-4.1-mini" => Price {
            input_per_million_usd: 0.40,
            output_per_million_usd: 1.60,
            cache_read_per_million_usd: Some(0.10),
            cache_write_per_million_usd: None,
            reasoning_per_million_usd: Some(2.40),
        },
        _ => return None,
    };

    Some(price)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn price_answers_for_a_known_model() {
        let price = price("gpt-4o").expect("known gpt-4o price");

        assert_eq!(price.input_per_million_usd, 2.50);
        assert_eq!(price.output_per_million_usd, 10.00);
        assert_eq!(price.cache_read_per_million_usd, Some(1.25));
        assert_eq!(price.cache_write_per_million_usd, None);
        assert_eq!(price.reasoning_per_million_usd, Some(15.00));
    }

    #[test]
    fn price_answers_for_a_dated_snapshot_id() {
        assert!(price("gpt-4o-2024-11-20").is_some());
        assert!(price("gpt-4o-mini-2024-07-18").is_some());
    }

    #[test]
    fn price_is_none_for_an_unknown_model() {
        assert!(price("qwen3:8b").is_none());
    }
}
