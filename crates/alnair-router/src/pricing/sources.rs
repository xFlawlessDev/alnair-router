//! Parsers for public pricing catalogs: LiteLLM and models.dev.
//!
//! Both are "USD per million tokens" once normalized. LiteLLM stores
//! per-token floats and includes many non-chat entries, so only chat-shaped
//! models with a positive input or output cost are kept. models.dev nests
//! models under providers; when several providers list the same model id the
//! cheapest input rate wins, so one deterministic row lands in the table.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::pricing::Price;

/// One priced model from a remote catalog.
#[derive(Debug, Clone, PartialEq)]
pub struct FetchedPrice {
    pub model: String,
    pub price: Price,
}

/// Parses a catalog body, choosing the parser from the URL, then from the
/// payload shape when the URL is unrecognized.
pub fn parse(source_url: &str, body: &str) -> Result<Vec<FetchedPrice>> {
    let url = source_url.to_lowercase();
    if url.contains("litellm") {
        return parse_litellm(body);
    }
    if url.contains("models.dev") {
        return parse_models_dev(body);
    }

    parse_litellm(body).or_else(|_| parse_models_dev(body))
}

#[derive(Debug, Deserialize)]
struct LiteLlmEntry {
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    input_cost_per_token: Option<f64>,
    #[serde(default)]
    output_cost_per_token: Option<f64>,
    #[serde(default)]
    cache_read_input_token_cost: Option<f64>,
    #[serde(default)]
    cache_creation_input_token_cost: Option<f64>,
    #[serde(default)]
    output_cost_per_reasoning_token: Option<f64>,
}

/// LiteLLM's `model_prices_and_context_window.json`: per-token rates keyed by
/// model id, with a `mode` that marks chat entries (embeddings, images and
/// audio are skipped).
fn parse_litellm(body: &str) -> Result<Vec<FetchedPrice>> {
    let entries: BTreeMap<String, LiteLlmEntry> = serde_json::from_str(body)
        .map_err(|error| Error::BadRequest(format!("invalid LiteLLM pricing payload: {error}")))?;

    let mut prices = Vec::new();
    for (model, entry) in entries {
        if matches!(entry.mode.as_deref(), Some(mode) if mode != "chat") {
            continue;
        }
        let (Some(input), Some(output)) = (entry.input_cost_per_token, entry.output_cost_per_token)
        else {
            continue;
        };
        if !input.is_finite() || !output.is_finite() || (input <= 0.0 && output <= 0.0) {
            continue;
        }

        prices.push(FetchedPrice {
            model,
            price: Price {
                input_per_million_usd: input * 1_000_000.0,
                output_per_million_usd: output * 1_000_000.0,
                cache_read_per_million_usd: entry
                    .cache_read_input_token_cost
                    .map(|cost| cost * 1_000_000.0),
                cache_write_per_million_usd: entry
                    .cache_creation_input_token_cost
                    .map(|cost| cost * 1_000_000.0),
                reasoning_per_million_usd: entry
                    .output_cost_per_reasoning_token
                    .map(|cost| cost * 1_000_000.0),
            },
        });
    }

    if prices.is_empty() {
        return Err(Error::BadRequest(
            "no chat models with pricing found in the LiteLLM payload".to_string(),
        ));
    }
    Ok(prices)
}

#[derive(Debug, Deserialize)]
struct ModelsDevProvider {
    #[serde(default)]
    models: BTreeMap<String, ModelsDevModel>,
}

#[derive(Debug, Deserialize)]
struct ModelsDevModel {
    #[serde(default)]
    cost: Option<ModelsDevCost>,
}

#[derive(Debug, Deserialize)]
struct ModelsDevCost {
    #[serde(default)]
    input: Option<f64>,
    #[serde(default)]
    output: Option<f64>,
    #[serde(default)]
    cache_read: Option<f64>,
    #[serde(default)]
    cache_write: Option<f64>,
    #[serde(default)]
    reasoning: Option<f64>,
}

/// models.dev's `api.json`: providers → models → `cost` in USD per million.
fn parse_models_dev(body: &str) -> Result<Vec<FetchedPrice>> {
    let providers: BTreeMap<String, ModelsDevProvider> =
        serde_json::from_str(body).map_err(|error| {
            Error::BadRequest(format!("invalid models.dev pricing payload: {error}"))
        })?;

    let mut best: BTreeMap<String, Price> = BTreeMap::new();
    for provider in providers.into_values() {
        for (model, entry) in provider.models {
            let Some(cost) = entry.cost else {
                continue;
            };
            let (Some(input), Some(output)) = (cost.input, cost.output) else {
                continue;
            };
            if !input.is_finite() || !output.is_finite() || (input <= 0.0 && output <= 0.0) {
                continue;
            }

            let price = Price {
                input_per_million_usd: input,
                output_per_million_usd: output,
                cache_read_per_million_usd: cost.cache_read,
                cache_write_per_million_usd: cost.cache_write,
                reasoning_per_million_usd: cost.reasoning,
            };
            match best.get(&model) {
                Some(existing) if existing.input_per_million_usd <= price.input_per_million_usd => {
                }
                _ => {
                    best.insert(model, price);
                }
            }
        }
    }

    if best.is_empty() {
        return Err(Error::BadRequest(
            "no priced models found in the models.dev payload".to_string(),
        ));
    }
    Ok(best
        .into_iter()
        .map(|(model, price)| FetchedPrice { model, price })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LITELLM: &str = r#"{
        "gpt-4o": {
            "mode": "chat",
            "input_cost_per_token": 2.5e-06,
            "output_cost_per_token": 1e-05,
            "cache_read_input_token_cost": 1.25e-06,
            "output_cost_per_reasoning_token": 1.5e-05
        },
        "text-embedding-3-small": {
            "mode": "embedding",
            "input_cost_per_token": 2e-08,
            "output_cost_per_token": 0
        },
        "free-local": { "mode": "chat" }
    }"#;

    const MODELS_DEV: &str = r#"{
        "openai": {
            "models": {
                "gpt-4o": { "id": "gpt-4o", "cost": { "input": 2.5, "output": 10, "cache_read": 1.25 } }
            }
        },
        "azure": {
            "models": {
                "gpt-4o": { "id": "gpt-4o", "cost": { "input": 2.75, "output": 11 } },
                "claude-sonnet-4-5": { "id": "claude-sonnet-4-5", "cost": { "input": 3, "output": 15, "cache_write": 3.75 } }
            }
        },
        "unpriced": { "models": { "whatever": { "id": "whatever" } } }
    }"#;

    #[test]
    fn litellm_parses_per_token_rates_to_per_million() {
        let prices = parse_litellm(LITELLM).expect("parse");

        assert_eq!(prices.len(), 1);
        let gpt = &prices[0];
        assert_eq!(gpt.model, "gpt-4o");
        assert_eq!(gpt.price.input_per_million_usd, 2.5);
        assert_eq!(gpt.price.output_per_million_usd, 10.0);
        assert_eq!(gpt.price.cache_read_per_million_usd, Some(1.25));
        assert_eq!(gpt.price.reasoning_per_million_usd, Some(15.0));
    }

    #[test]
    fn models_dev_keeps_the_cheapest_provider_per_model() {
        let prices = parse_models_dev(MODELS_DEV).expect("parse");

        assert_eq!(prices.len(), 2);
        let gpt = prices
            .iter()
            .find(|price| price.model == "gpt-4o")
            .expect("gpt-4o");
        assert_eq!(gpt.price.input_per_million_usd, 2.5, "openai beats azure");
        assert_eq!(gpt.price.cache_read_per_million_usd, Some(1.25));
        let claude = prices
            .iter()
            .find(|price| price.model == "claude-sonnet-4-5")
            .expect("claude");
        assert_eq!(claude.price.cache_write_per_million_usd, Some(3.75));
    }

    #[test]
    fn parse_prefers_the_url_hint_and_sniffs_unknown_sources() {
        assert_eq!(
            parse("https://x/litellm.json", LITELLM)
                .expect("url hint")
                .len(),
            1
        );
        assert_eq!(
            parse("https://x/models.dev.json", MODELS_DEV)
                .expect("url hint")
                .len(),
            2
        );
        assert_eq!(
            parse("https://x/unknown.json", MODELS_DEV)
                .expect("sniff")
                .len(),
            2
        );
        assert_eq!(
            parse("https://x/unknown.json", LITELLM)
                .expect("sniff")
                .len(),
            1
        );
    }

    #[test]
    fn empty_payloads_are_rejected() {
        assert!(parse_litellm(r#"{"nothing": {"mode": "chat"}}"#).is_err());
        assert!(parse_models_dev(r#"{"p": {"models": {}}}"#).is_err());
    }
}
