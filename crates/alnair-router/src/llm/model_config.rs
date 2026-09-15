use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::llm::providers::PROVIDER_MAX_RETRIES;
use crate::llm::types::{ChatError, GenerationOptions, ProviderType};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelConfig {
    pub provider_type: ProviderType,
    pub base_url: String,
    pub model_id: String,
    pub api_key: Option<String>,
    #[serde(default)]
    pub custom_headers: BTreeMap<String, String>,
    #[serde(default)]
    pub supports_vision: bool,
    #[serde(default)]
    pub supports_thinking: bool,
    #[serde(default)]
    pub supports_cache_control: bool,
    #[serde(default = "default_context_window")]
    pub context_window: u32,
    pub cost_rates: Option<ModelCostRates>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelCostRates {
    /// USD per million input tokens
    pub input_per_million_usd: f64,
    /// USD per million output tokens
    pub output_per_million_usd: f64,
    /// USD per million cache-read tokens
    pub cache_read_per_million_usd: Option<f64>,
    /// USD per million cache-write tokens
    pub cache_write_per_million_usd: Option<f64>,
}

/// Static registry of known model cost rates (USD per million tokens).
/// Updated as providers publish pricing. Returns None for local/unknown models.
pub fn known_cost_rates(model_id: &str) -> Option<ModelCostRates> {
    match model_id {
        "gpt-4o" | "gpt-4o-2024-11-20" => Some(ModelCostRates {
            input_per_million_usd: 2.50,
            output_per_million_usd: 10.00,
            cache_read_per_million_usd: None,
            cache_write_per_million_usd: None,
        }),
        "gpt-4o-mini" | "gpt-4o-mini-2024-07-18" => Some(ModelCostRates {
            input_per_million_usd: 0.15,
            output_per_million_usd: 0.60,
            cache_read_per_million_usd: None,
            cache_write_per_million_usd: None,
        }),
        "gpt-4.1" => Some(ModelCostRates {
            input_per_million_usd: 2.00,
            output_per_million_usd: 8.00,
            cache_read_per_million_usd: None,
            cache_write_per_million_usd: None,
        }),
        "gpt-4.1-mini" => Some(ModelCostRates {
            input_per_million_usd: 0.40,
            output_per_million_usd: 1.60,
            cache_read_per_million_usd: None,
            cache_write_per_million_usd: None,
        }),
        // claude-* rates added when Phase E (AnthropicNative) lands
        _ => None,
    }
}

/// Options passed to a single LLM streaming call.
#[derive(Debug, Clone)]
pub struct LlmStreamOptions {
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub top_k: Option<i32>,
    pub presence_penalty: Option<f64>,
    pub frequency_penalty: Option<f64>,
    pub max_tokens: Option<i32>,
    pub seed: Option<i64>,
    pub stop: Option<Vec<String>>,
    pub repeat_penalty: Option<f64>,
    pub num_ctx: Option<i32>,
    pub thinking_level: Option<ThinkingLevel>,
    pub cache_retention: CacheRetention,
    pub connect_timeout_ms: Option<u64>,
    pub idle_timeout_ms: Option<u64>,
    /// Upper bound for the exponential retry delay, in milliseconds.
    pub max_retry_delay_ms: u64,
    /// Retries after the first attempt fails, inside one provider call.
    pub max_retries: usize,
}

impl Default for LlmStreamOptions {
    fn default() -> Self {
        Self {
            temperature: None,
            top_p: None,
            top_k: None,
            presence_penalty: None,
            frequency_penalty: None,
            max_tokens: None,
            seed: None,
            stop: None,
            repeat_penalty: None,
            num_ctx: None,
            thinking_level: None,
            cache_retention: CacheRetention::None,
            connect_timeout_ms: None,
            idle_timeout_ms: None,
            max_retry_delay_ms: 30_000,
            max_retries: PROVIDER_MAX_RETRIES,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingLevel {
    #[default]
    None,
    Minimal,
    Low,
    Medium,
    High,
    Max,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheRetention {
    #[default]
    None,
    Short,
    Long,
}

fn default_context_window() -> u32 {
    128_000
}

impl ModelConfig {
    pub fn openai_compatible(
        base_url: impl Into<String>,
        model_id: impl Into<String>,
        api_key: Option<String>,
    ) -> Self {
        Self {
            provider_type: ProviderType::OpenaiCompatible,
            base_url: base_url.into(),
            model_id: model_id.into(),
            api_key,
            custom_headers: BTreeMap::new(),
            supports_vision: false,
            supports_thinking: false,
            supports_cache_control: false,
            context_window: default_context_window(),
            cost_rates: None,
        }
    }

    pub fn anthropic(
        base_url: impl Into<String>,
        model_id: impl Into<String>,
        api_key: Option<String>,
    ) -> Self {
        Self {
            provider_type: ProviderType::AnthropicNative,
            base_url: base_url.into(),
            model_id: model_id.into(),
            api_key,
            custom_headers: BTreeMap::new(),
            supports_vision: false,
            supports_thinking: false,
            supports_cache_control: false,
            context_window: default_context_window(),
            cost_rates: None,
        }
    }

    /// Effective cost rates: explicit override on the config > static registry > None.
    /// Returns None for local/unmetered models (Ollama) or unknown model IDs.
    pub fn effective_cost_rates(&self) -> Option<ModelCostRates> {
        self.cost_rates
            .clone()
            .or_else(|| known_cost_rates(&self.model_id))
    }
}

pub(crate) fn apply_custom_headers(
    mut request: reqwest::RequestBuilder,
    headers: &BTreeMap<String, String>,
) -> Result<reqwest::RequestBuilder, ChatError> {
    for (name, value) in headers {
        let header_name =
            reqwest::header::HeaderName::from_bytes(name.as_bytes()).map_err(|error| {
                ChatError::BadRequest(format!("invalid provider header name: {error}"))
            })?;
        let header_value = reqwest::header::HeaderValue::from_str(value).map_err(|error| {
            ChatError::BadRequest(format!(
                "invalid provider header value for '{name}': {error}"
            ))
        })?;
        request = request.header(header_name, header_value);
    }
    Ok(request)
}

impl From<&GenerationOptions> for LlmStreamOptions {
    fn from(value: &GenerationOptions) -> Self {
        Self {
            temperature: value.temperature,
            top_p: value.top_p,
            top_k: value.top_k,
            presence_penalty: value.presence_penalty,
            frequency_penalty: value.frequency_penalty,
            max_tokens: value.max_tokens.or(value.num_predict),
            seed: value.seed,
            stop: value.stop.clone(),
            repeat_penalty: value.repeat_penalty,
            num_ctx: value.num_ctx,
            thinking_level: value.thinking_level,
            cache_retention: CacheRetention::None,
            connect_timeout_ms: None,
            idle_timeout_ms: None,
            max_retry_delay_ms: 30_000,
            max_retries: PROVIDER_MAX_RETRIES,
        }
    }
}

impl LlmStreamOptions {
    pub fn apply_to_openai_compatible_body(
        &self,
        body: &mut serde_json::Value,
        supports_thinking: bool,
        supports_cache_control: bool,
    ) {
        macro_rules! set {
            ($field:ident) => {
                if let Some(v) = self.$field {
                    body[stringify!($field)] = serde_json::json!(v);
                }
            };
        }

        set!(temperature);
        set!(top_p);
        set!(presence_penalty);
        set!(frequency_penalty);
        if let Some(v) = self.max_tokens
            && v > 0
        {
            body["max_tokens"] = serde_json::json!(v);
        }
        set!(seed);
        if let Some(ref v) = self.stop {
            body["stop"] = serde_json::json!(v);
        }
        if supports_thinking && let Some(reasoning_effort) = self.openai_reasoning_effort() {
            body["reasoning_effort"] = serde_json::json!(reasoning_effort);
        }
        if supports_cache_control && let Some(cache_control) = self.openai_cache_control() {
            body["cache_control"] = cache_control;
        }
    }

    fn openai_reasoning_effort(&self) -> Option<&'static str> {
        match self.thinking_level.unwrap_or(ThinkingLevel::None) {
            ThinkingLevel::None => None,
            ThinkingLevel::Minimal => Some("minimal"),
            ThinkingLevel::Low => Some("low"),
            ThinkingLevel::Medium => Some("medium"),
            ThinkingLevel::High => Some("high"),
            ThinkingLevel::Max => Some("xhigh"),
        }
    }

    fn openai_cache_control(&self) -> Option<serde_json::Value> {
        match self.cache_retention {
            CacheRetention::None => None,
            CacheRetention::Short => Some(serde_json::json!({ "type": "ephemeral" })),
            CacheRetention::Long => Some(serde_json::json!({ "type": "persistent" })),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_config_roundtrip_serde() {
        let config = ModelConfig::openai_compatible("https://api.example.com", "gpt-4o", None);
        let encoded = serde_json::to_string(&config).expect("serialize model config");
        let decoded: ModelConfig =
            serde_json::from_str(&encoded).expect("deserialize model config");
        assert!(matches!(
            decoded.provider_type,
            ProviderType::OpenaiCompatible
        ));
        assert_eq!(decoded.base_url, "https://api.example.com");
        assert_eq!(decoded.model_id, "gpt-4o");
        assert_eq!(decoded.context_window, 128_000);
        assert!(decoded.cost_rates.is_none());
    }

    #[test]
    fn known_cost_rates_returns_gpt_4o_rates() {
        let rates = known_cost_rates("gpt-4o").expect("known gpt-4o rates");

        assert_eq!(rates.input_per_million_usd, 2.50);
        assert_eq!(rates.output_per_million_usd, 10.00);
        assert_eq!(rates.cache_read_per_million_usd, None);
        assert_eq!(rates.cache_write_per_million_usd, None);
    }

    #[test]
    fn known_cost_rates_returns_none_for_unknown_model() {
        assert!(known_cost_rates("qwen3:8b").is_none());
    }

    #[test]
    fn explicit_cost_rates_override_static_registry() {
        let custom = ModelCostRates {
            input_per_million_usd: 1.23,
            output_per_million_usd: 4.56,
            cache_read_per_million_usd: Some(0.12),
            cache_write_per_million_usd: Some(0.34),
        };
        let mut config = ModelConfig::openai_compatible("https://api.example.com", "gpt-4o", None);
        config.cost_rates = Some(custom.clone());

        let rates = config.effective_cost_rates().expect("custom rates");

        assert_eq!(rates.input_per_million_usd, custom.input_per_million_usd);
        assert_eq!(rates.output_per_million_usd, custom.output_per_million_usd);
        assert_eq!(
            rates.cache_read_per_million_usd,
            custom.cache_read_per_million_usd
        );
        assert_eq!(
            rates.cache_write_per_million_usd,
            custom.cache_write_per_million_usd
        );
    }

    #[test]
    fn llm_stream_options_from_generation_options_maps_common_fields() {
        let generation = GenerationOptions {
            temperature: Some(0.35),
            top_p: Some(0.9),
            presence_penalty: Some(0.1),
            frequency_penalty: Some(0.2),
            max_tokens: Some(256),
            thinking_level: Some(ThinkingLevel::Medium),
            ..GenerationOptions::default()
        };
        let options = LlmStreamOptions::from(&generation);
        assert_eq!(options.temperature, Some(0.35));
        assert_eq!(options.top_p, Some(0.9));
        assert_eq!(options.presence_penalty, Some(0.1));
        assert_eq!(options.frequency_penalty, Some(0.2));
        assert_eq!(options.max_tokens, Some(256));
        assert!(matches!(
            options.thinking_level,
            Some(ThinkingLevel::Medium)
        ));
        assert_eq!(options.max_retry_delay_ms, 30_000);
        assert_eq!(options.max_retries, PROVIDER_MAX_RETRIES);
    }

    #[test]
    fn llm_stream_options_default_retries_are_pinned() {
        let options = LlmStreamOptions::default();
        assert_eq!(options.max_retries, PROVIDER_MAX_RETRIES);
        assert_eq!(options.max_retry_delay_ms, 30_000);
    }
}
