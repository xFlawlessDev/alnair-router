//! Parsers for public pricing catalogs: LiteLLM and models.dev.
//!
//! Both are "USD per million tokens" once normalized. LiteLLM stores
//! per-token floats and includes many non-chat entries, so only chat-shaped
//! models with a positive input or output cost are kept. models.dev nests
//! models under providers; when several providers list the same model id the
//! cheapest input rate wins, so one deterministic row lands in the table.
//!
//! Long-context rates are imported where the catalog states them without
//! ambiguity, as `tiers` entries carrying only `context_above`:
//!
//! * LiteLLM spells them as `*_above_<N>k_tokens` sibling keys next to the base
//!   rate. A band is imported only when both `input_cost_per_token_above_…`
//!   and `output_cost_per_token_above_…` are present, and bands are ordered by
//!   descending threshold so the most specific matching band wins.
//! * models.dev spells them as `cost.tiers[]`, each with `tier.size` and a full
//!   input/output cost.
//!
//! Neither catalog publishes peak/off-peak windows in a form this contract can
//! express (LiteLLM's `off_peak_pricing` is weekday-scoped and window-based),
//! so time tiers are dashboard-override only. Where a catalog states a
//! threshold-specific cache or reasoning rate it is used; when it omits one but
//! the base rate exists, the base rate is carried into the tier rather than
//! letting the tier fall back to its full input rate, which would silently
//! remove a cache discount the model still offers.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::pricing::{Price, PriceTier};

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
    /// Everything else, because the long-context rates are dynamic keys such as
    /// `input_cost_per_token_above_200k_tokens`.
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

/// Token threshold encoded in a LiteLLM long-context key, e.g.
/// `input_cost_per_token_above_200k_tokens` → `200_000`.
///
/// The suffix must be exactly `<digits>k_tokens`: LiteLLM also publishes
/// serviced variants (`_batches`, `_flex`, `_priority`, `_12hr`) that price a
/// different purchase, and treating those as ordinary tiers would reprice
/// normal traffic.
fn litellm_threshold(key: &str, base: &str) -> Option<u64> {
    let digits = key.strip_prefix(base)?.strip_prefix("_above_")?;
    let digits = digits.strip_suffix("k_tokens")?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let thousands = digits.parse::<u64>().ok()?;
    Some(thousands.saturating_mul(1_000))
}

/// Reads a LiteLLM per-token rate from the entry's flattened extras.
fn litellm_rate(entry: &LiteLlmEntry, key: &str) -> Option<f64> {
    entry.extra.get(key)?.as_f64()
}

/// Long-context tiers for one LiteLLM entry, descending by threshold.
///
/// A band is only kept when it restates both the input and the output rate: a
/// threshold with one half missing would otherwise invent a rate. Thresholds
/// with a published cache or reasoning rate use it; when the catalog omits one
/// but a base rate exists, that base rate is carried in, because a tier's
/// missing cache rate means "bill cache at this tier's input rate" and silently
/// dropping the discount would overcharge.
fn litellm_tiers(entry: &LiteLlmEntry, base: &Price) -> Vec<PriceTier> {
    const BASES: [&str; 5] = [
        "input_cost_per_token",
        "output_cost_per_token",
        "cache_read_input_token_cost",
        "cache_creation_input_token_cost",
        "output_cost_per_reasoning_token",
    ];

    let mut bands: BTreeMap<u64, BTreeMap<&str, f64>> = BTreeMap::new();
    for key in entry.extra.keys() {
        for base_key in BASES {
            let Some(threshold) = litellm_threshold(key, base_key) else {
                continue;
            };
            let Some(rate) = litellm_rate(entry, key) else {
                continue;
            };
            if !rate.is_finite() || rate < 0.0 {
                continue;
            }
            bands.entry(threshold).or_default().insert(base_key, rate);
        }
    }

    bands
        .into_iter()
        .rev()
        .filter_map(|(threshold, rates)| {
            let input = *rates.get("input_cost_per_token")?;
            let output = *rates.get("output_cost_per_token")?;
            let per_million = |rate: f64| rate * 1_000_000.0;
            Some(PriceTier {
                context_above: Some(threshold),
                start_minute_utc: None,
                end_minute_utc: None,
                input_per_million_usd: per_million(input),
                output_per_million_usd: per_million(output),
                cache_read_per_million_usd: rates
                    .get("cache_read_input_token_cost")
                    .map(|rate| per_million(*rate))
                    .or(base.cache_read_per_million_usd),
                cache_write_per_million_usd: rates
                    .get("cache_creation_input_token_cost")
                    .map(|rate| per_million(*rate))
                    .or(base.cache_write_per_million_usd),
                reasoning_per_million_usd: rates
                    .get("output_cost_per_reasoning_token")
                    .map(|rate| per_million(*rate))
                    .or(base.reasoning_per_million_usd),
            })
        })
        .collect()
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

        let base = Price {
            unit_price: None,
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
            tiers: Vec::new(),
        };
        let tiers = litellm_tiers(&entry, &base);

        prices.push(FetchedPrice {
            model,
            price: Price { tiers, ..base },
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
    /// Long-context bands, each with its own complete rate set.
    #[serde(default)]
    tiers: Vec<ModelsDevCostTier>,
}

#[derive(Debug, Deserialize)]
struct ModelsDevCostTier {
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
    #[serde(default)]
    tier: Option<ModelsDevTierBoundary>,
}

#[derive(Debug, Deserialize)]
struct ModelsDevTierBoundary {
    #[serde(default)]
    size: Option<u64>,
}

/// models.dev's `cost.tiers` as long-context bands, descending by threshold.
///
/// `tier.type` is always `context` in the published payload, and a band without
/// a size or without both an input and an output rate cannot be applied, so it
/// is dropped rather than guessed. As with LiteLLM, a base cache or reasoning
/// rate is carried into a band that omits it.
fn models_dev_tiers(cost: &ModelsDevCost, base: &Price) -> Vec<PriceTier> {
    let mut tiers = Vec::new();
    for band in &cost.tiers {
        let (Some(size), Some(input), Some(output)) = (
            band.tier.as_ref().and_then(|tier| tier.size),
            band.input,
            band.output,
        ) else {
            continue;
        };
        let rates = [input, output];
        if rates.iter().any(|rate| !rate.is_finite() || *rate < 0.0) {
            continue;
        }
        let optional = [band.cache_read, band.cache_write, band.reasoning];
        if optional
            .iter()
            .flatten()
            .any(|rate| !rate.is_finite() || *rate < 0.0)
        {
            continue;
        }

        tiers.push(PriceTier {
            context_above: Some(size),
            start_minute_utc: None,
            end_minute_utc: None,
            input_per_million_usd: input,
            output_per_million_usd: output,
            cache_read_per_million_usd: band.cache_read.or(base.cache_read_per_million_usd),
            cache_write_per_million_usd: band.cache_write.or(base.cache_write_per_million_usd),
            reasoning_per_million_usd: band.reasoning.or(base.reasoning_per_million_usd),
        });
    }

    tiers.sort_by_key(|tier| std::cmp::Reverse(tier.context_above));
    tiers
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

            let base = Price {
                unit_price: None,
                input_per_million_usd: input,
                output_per_million_usd: output,
                cache_read_per_million_usd: cost.cache_read,
                cache_write_per_million_usd: cost.cache_write,
                reasoning_per_million_usd: cost.reasoning,
                tiers: Vec::new(),
            };
            let price = Price {
                tiers: models_dev_tiers(&cost, &base),
                ..base
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

    const LITELLM_TIERED: &str = r#"{
        "claude-sonnet-4-5": {
            "mode": "chat",
            "input_cost_per_token": 3e-06,
            "output_cost_per_token": 1.5e-05,
            "cache_read_input_token_cost": 3e-07,
            "cache_creation_input_token_cost": 3.75e-06,
            "input_cost_per_token_above_200k_tokens": 6e-06,
            "output_cost_per_token_above_200k_tokens": 2.25e-05,
            "cache_read_input_token_cost_above_200k_tokens": 6e-07,
            "input_cost_per_token_above_200k_tokens_batches": 3e-06,
            "output_cost_per_token_above_200k_tokens_priority": 4.5e-05
        },
        "half-a-band": {
            "mode": "chat",
            "input_cost_per_token": 1e-06,
            "output_cost_per_token": 2e-06,
            "input_cost_per_token_above_100k_tokens": 2e-06
        }
    }"#;

    const MODELS_DEV_TIERED: &str = r#"{
        "google": {
            "models": {
                "gemini-2.5-pro": {
                    "id": "gemini-2.5-pro",
                    "cost": {
                        "input": 1.25,
                        "output": 10,
                        "cache_read": 0.125,
                        "tiers": [
                            {
                                "input": 2.5,
                                "output": 15,
                                "cache_read": 0.25,
                                "tier": { "type": "context", "size": 200000 }
                            },
                            { "input": 5, "output": 30, "tier": { "type": "context" } }
                        ]
                    }
                }
            }
        }
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

    #[test]
    fn litellm_imports_long_context_bands_and_ignores_serviced_variants() {
        let prices = parse_litellm(LITELLM_TIERED).expect("parse");
        let claude = prices
            .iter()
            .find(|price| price.model == "claude-sonnet-4-5")
            .expect("claude");

        assert_eq!(
            claude.price.tiers.len(),
            1,
            "batches/priority are not tiers"
        );
        let tier = &claude.price.tiers[0];
        assert_eq!(tier.context_above, Some(200_000));
        assert_eq!(tier.start_minute_utc, None);
        assert_eq!(tier.end_minute_utc, None);
        assert_eq!(tier.input_per_million_usd, 6.0);
        assert_eq!(tier.output_per_million_usd, 22.5);
        assert_eq!(tier.cache_read_per_million_usd, Some(0.6));
        // No threshold-specific cache-write rate: the base rate is carried in.
        assert_eq!(tier.cache_write_per_million_usd, Some(3.75));
        assert_eq!(tier.reasoning_per_million_usd, None);
    }

    #[test]
    fn litellm_drops_a_band_missing_half_its_rates() {
        let prices = parse_litellm(LITELLM_TIERED).expect("parse");
        let half = prices
            .iter()
            .find(|price| price.model == "half-a-band")
            .expect("half-a-band");

        assert!(half.price.tiers.is_empty());
        assert_eq!(half.price.input_per_million_usd, 1.0);
    }

    #[test]
    fn litellm_places_highest_threshold_first() {
        let payload = r#"{"model":{"mode":"chat",
          "input_cost_per_token":0.000001,"output_cost_per_token":0.000002,
          "input_cost_per_token_above_200k_tokens":0.000003,
          "output_cost_per_token_above_200k_tokens":0.000004,
          "input_cost_per_token_above_1000k_tokens":0.000005,
          "output_cost_per_token_above_1000k_tokens":0.000006}}"#;
        let prices = parse_litellm(payload).expect("parse");
        assert_eq!(prices[0].price.tiers[0].context_above, Some(1_000_000));
        assert_eq!(prices[0].price.tiers[1].context_above, Some(200_000));
    }

    #[test]
    fn models_dev_imports_context_tiers() {
        let prices = parse_models_dev(MODELS_DEV_TIERED).expect("parse");
        let gemini = prices
            .iter()
            .find(|price| price.model == "gemini-2.5-pro")
            .expect("gemini");

        assert_eq!(gemini.price.input_per_million_usd, 1.25);
        assert_eq!(
            gemini.price.tiers.len(),
            1,
            "a band without a size is dropped"
        );
        let tier = &gemini.price.tiers[0];
        assert_eq!(tier.context_above, Some(200_000));
        assert_eq!(tier.input_per_million_usd, 2.5);
        assert_eq!(tier.output_per_million_usd, 15.0);
        assert_eq!(tier.cache_read_per_million_usd, Some(0.25));
    }
}
