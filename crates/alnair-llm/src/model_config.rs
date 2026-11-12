use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::providers::PROVIDER_MAX_TRANSPORT_RETRIES;
use crate::types::{ChatError, GenerationOptions, ProviderType};

/// How a credential is presented to the upstream.
///
/// The distinction matters for Anthropic-shaped endpoints: an API key travels
/// in `x-api-key`, while an OAuth/subscription session token travels in
/// `Authorization: Bearer`. Sending the wrong one 401s every request.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthStyle {
    /// Provider default: `x-api-key` for Anthropic, bearer for OpenAI.
    #[default]
    ApiKey,
    /// Always `Authorization: Bearer`, regardless of provider family.
    Bearer,
}

/// Which OpenAI wire API an openai-compatible upstream speaks.
///
/// The two endpoints carry the same conversation but disagree on the request
/// body, the SSE event names and the usage counters, so the choice is a
/// per-connection property rather than something to sniff per request.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WireApi {
    /// `POST {base_url}/chat/completions`.
    #[default]
    Chat,
    /// `POST {base_url}/responses`.
    Responses,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelConfig {
    pub provider_type: ProviderType,
    pub base_url: String,
    pub model_id: String,
    pub api_key: Option<String>,
    #[serde(default)]
    pub auth_style: AuthStyle,
    /// Which OpenAI wire API to call. Ignored by every provider other than
    /// [`ProviderType::OpenaiCompatible`].
    #[serde(default)]
    pub wire_api: WireApi,
    #[serde(default)]
    pub custom_headers: BTreeMap<String, String>,
    /// The `chatgpt_account_id` claim of a ChatGPT/Codex OAuth bearer. The
    /// Codex backend refuses a request without the matching
    /// `chatgpt-account-id` header, and the claim is the only place the id
    /// exists, so the router derives it from the access token per request.
    /// `None` for every other provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chatgpt_account_id: Option<String>,
    #[serde(default)]
    pub supports_thinking: bool,
    #[serde(default)]
    pub supports_cache_control: bool,
    pub cost_rates: Option<ModelCostRates>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ModelCostRates {
    /// USD per million input tokens
    pub input_per_million_usd: f64,
    /// USD per million output tokens
    pub output_per_million_usd: f64,
    /// USD per million cache-read tokens
    pub cache_read_per_million_usd: Option<f64>,
    /// USD per million cache-write tokens
    pub cache_write_per_million_usd: Option<f64>,
    /// USD per million reasoning tokens. Reasoning is billed as a premium over
    /// the output rate, because completion tokens already include it.
    #[serde(default)]
    pub reasoning_per_million_usd: Option<f64>,
    /// Conditional rate overrides, in precedence order. The first tier whose
    /// conditions hold for a request replaces the base rates above for that
    /// whole request; an empty list means the base rates always apply.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tiers: Vec<CostTier>,
    /// Minute of the day (UTC) at which the request was started, captured once
    /// per upstream call. It decides which time-windowed tier applies, so a
    /// request is billed at its start rate even if it finishes after a window
    /// closes. `None` until the provider layer stamps it; a tier with a time
    /// window never matches an unstamped request.
    #[serde(default, skip_serializing)]
    pub request_start_minute_utc: Option<u32>,
}

/// One conditional price override.
///
/// A tier carries a complete rate set, exactly like the base rates: a missing
/// cache rate falls back to this tier's input rate and a missing reasoning rate
/// means no reasoning premium. Conditions are ANDed; an absent condition does
/// not constrain the tier. The presence of any `context_above` threshold makes
/// the tier depend on a known prompt size — a request whose upstream reported
/// no prompt tokens never matches it.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct CostTier {
    /// Applies when the prompt (cached tokens included) exceeds this many
    /// tokens.
    #[serde(default)]
    pub context_above: Option<u64>,
    /// Window start, minute of the day UTC, inclusive. `None` with `end` unset
    /// makes the tier time-independent.
    #[serde(default)]
    pub start_minute_utc: Option<u32>,
    /// Window end, minute of the day UTC, exclusive. A window whose start is
    /// greater than its end wraps past midnight.
    #[serde(default)]
    pub end_minute_utc: Option<u32>,
    /// USD per million input tokens
    pub input_per_million_usd: f64,
    /// USD per million output tokens
    pub output_per_million_usd: f64,
    /// USD per million cache-read tokens; falls back to this tier's input rate
    #[serde(default)]
    pub cache_read_per_million_usd: Option<f64>,
    /// USD per million cache-write tokens; falls back to this tier's input rate
    #[serde(default)]
    pub cache_write_per_million_usd: Option<f64>,
    /// USD per million reasoning tokens, billed as a premium over the output
    /// rate. `None` bills reasoning as ordinary output.
    #[serde(default)]
    pub reasoning_per_million_usd: Option<f64>,
}

/// The rates that actually apply to one request.
///
/// Resolved from [`ModelCostRates`] once the prompt size and start minute are
/// known, so the token arithmetic has a single flat rate set per request.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EffectiveCostRates {
    pub input_per_million_usd: f64,
    pub output_per_million_usd: f64,
    pub cache_read_per_million_usd: Option<f64>,
    pub cache_write_per_million_usd: Option<f64>,
    pub reasoning_per_million_usd: Option<f64>,
}

impl CostTier {
    /// Whether this tier's conditions hold for the given request.
    pub fn matches(&self, prompt_tokens: Option<u64>, minute_utc: Option<u32>) -> bool {
        let context_matches = match self.context_above {
            // A threshold is strictly greater-than, so a prompt exactly at it
            // keeps the cheaper tier. Without a prompt count the condition is
            // unknowable and must not match.
            Some(threshold) => prompt_tokens.is_some_and(|prompt| prompt > threshold),
            None => true,
        };
        if !context_matches {
            return false;
        }

        match (self.start_minute_utc, self.end_minute_utc) {
            (None, None) => true,
            (Some(start), Some(end)) if start < 1440 && end < 1440 && start != end => {
                let Some(minute) = minute_utc else {
                    return false;
                };
                if start < end {
                    minute >= start && minute < end
                } else {
                    // Wraps midnight: [start, 1440) ∪ [0, end).
                    minute >= start || minute < end
                }
            }
            // Half a window is unusable, and an empty one would otherwise read
            // as "any minute". Neither may silently reprice a request.
            _ => false,
        }
    }

    fn effective(&self) -> EffectiveCostRates {
        EffectiveCostRates {
            input_per_million_usd: self.input_per_million_usd,
            output_per_million_usd: self.output_per_million_usd,
            cache_read_per_million_usd: self.cache_read_per_million_usd,
            cache_write_per_million_usd: self.cache_write_per_million_usd,
            reasoning_per_million_usd: self.reasoning_per_million_usd,
        }
    }
}

impl ModelCostRates {
    /// The first tier that matches this request, else the base rates.
    ///
    /// `prompt_tokens` is the upstream's prompt total including cached tokens.
    /// The whole request uses the selected tier, so a request never mixes
    /// tiers across its input and output charges.
    pub fn resolve(&self, prompt_tokens: Option<u64>) -> EffectiveCostRates {
        self.tiers
            .iter()
            .find(|tier| tier.matches(prompt_tokens, self.request_start_minute_utc))
            .map(CostTier::effective)
            .unwrap_or(EffectiveCostRates {
                input_per_million_usd: self.input_per_million_usd,
                output_per_million_usd: self.output_per_million_usd,
                cache_read_per_million_usd: self.cache_read_per_million_usd,
                cache_write_per_million_usd: self.cache_write_per_million_usd,
                reasoning_per_million_usd: self.reasoning_per_million_usd,
            })
    }
}

/// Who picks the tool, and which one. Provider-neutral, so every dialect can
/// spell it the way its own endpoint documents.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolChoice {
    Auto,
    Required,
    None,
    Named { name: String },
}

/// Structured-output constraint, provider-neutral.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResponseFormat {
    Text,
    JsonObject,
    JsonSchema {
        name: String,
        schema: serde_json::Value,
        #[serde(default)]
        strict: bool,
    },
}

/// A citation an upstream attached to a span of output text.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct Citation {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Byte range in the accumulated output text, when the protocol reports one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_index: Option<usize>,
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
    pub thinking_level: Option<ThinkingLevel>,
    pub cache_retention: CacheRetention,
    /// Who picks the tool. `None` leaves the upstream's own default in place.
    pub tool_choice: Option<ToolChoice>,
    /// Whether the model may issue tool calls concurrently.
    pub parallel_tool_calls: Option<bool>,
    /// Structured-output constraint.
    pub response_format: Option<ResponseFormat>,
    /// Client metadata, replayed on the dialects that define it.
    pub metadata: Option<serde_json::Value>,
    /// End-user identifier.
    pub user: Option<String>,
    /// Priority tier, e.g. `auto`, `default`, `flex`.
    pub service_tier: Option<String>,
    /// Ask the upstream to retain the turn server-side.
    pub store: Option<bool>,
    /// Prompt-cache routing hint.
    pub prompt_cache_key: Option<String>,
    /// Output verbosity, `low|medium|high`.
    pub verbosity: Option<String>,
    /// Whether the caller wants log probabilities.
    pub logprobs: Option<bool>,
    /// Alternatives per position, 0-20.
    pub top_logprobs: Option<u8>,
    /// Responses `include` opt-ins, e.g. `message.output_text.logprobs`.
    pub include: Vec<String>,
    /// OpenAI `web_search_options`, replayed verbatim on an OpenAI-compatible
    /// upstream. The router does not interpret the object; providers that
    /// support built-in web search read it, and the rest reject the request,
    /// which surfaces as an error rather than a silent drop.
    pub web_search_options: Option<serde_json::Value>,
    /// Upper bound for the exponential retry delay, in milliseconds.
    pub max_retry_delay_ms: u64,
    /// Transport retries after a request fails without a response (connection
    /// reset, refused, TLS failure). Fixed and small: the router owns tier
    /// failover, so an extra layer of transport retries only multiplies load.
    pub max_transport_retries: usize,
    /// Retries after a retryable HTTP status (408/429/5xx/529), per tier. The
    /// router maps `router.max_retries_per_tier` here.
    pub max_status_retries: usize,
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
            thinking_level: None,
            cache_retention: CacheRetention::None,
            tool_choice: None,
            parallel_tool_calls: None,
            response_format: None,
            metadata: None,
            user: None,
            service_tier: None,
            store: None,
            prompt_cache_key: None,
            verbosity: None,
            logprobs: None,
            top_logprobs: None,
            include: Vec::new(),
            web_search_options: None,
            max_retry_delay_ms: 30_000,
            max_transport_retries: PROVIDER_MAX_TRANSPORT_RETRIES,
            max_status_retries: PROVIDER_MAX_TRANSPORT_RETRIES,
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

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CacheRetention {
    #[default]
    None,
    Short,
    Long,
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
            auth_style: AuthStyle::default(),
            wire_api: WireApi::default(),
            custom_headers: BTreeMap::new(),
            chatgpt_account_id: None,
            supports_thinking: false,
            supports_cache_control: false,
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
            auth_style: AuthStyle::default(),
            wire_api: WireApi::default(),
            custom_headers: BTreeMap::new(),
            chatgpt_account_id: None,
            supports_thinking: false,
            supports_cache_control: false,
            cost_rates: None,
        }
    }

    /// Explicit rates on this config, when the caller resolved them.
    ///
    /// The provider layer no longer carries a built-in price table: pricing is
    /// router policy, so an upstream is only ever told the rate the router
    /// already decided on.
    ///
    /// The returned snapshot stamps the UTC minute of the call when the caller
    /// left it unset, so conditional tiers are chosen by when the request was
    /// *started*. Providers call this once, before sending, and bill the whole
    /// turn from that snapshot; a caller that pre-computed the minute keeps it.
    pub fn effective_cost_rates(&self) -> Option<ModelCostRates> {
        self.cost_rates.clone().map(|mut rates| {
            rates
                .request_start_minute_utc
                .get_or_insert_with(current_utc_minute);
            rates
        })
    }
}

/// Minute of the day (UTC), 0..=1439.
fn current_utc_minute() -> u32 {
    use chrono::Timelike;

    let now = chrono::Utc::now();
    now.hour() * 60 + now.minute()
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
            thinking_level: value.thinking_level,
            cache_retention: CacheRetention::None,
            web_search_options: None,
            max_retry_delay_ms: 30_000,
            max_transport_retries: PROVIDER_MAX_TRANSPORT_RETRIES,
            max_status_retries: PROVIDER_MAX_TRANSPORT_RETRIES,
            ..Self::default()
        }
    }
}

/// The OpenAI spelling of a tool choice.
///
/// Anthropic's `any`/`{type: "tool"}` are handled by its own conversion; this
/// is the chat-completions and Responses shape.
pub fn tool_choice_to_openai(choice: &ToolChoice) -> serde_json::Value {
    match choice {
        ToolChoice::Auto => serde_json::json!("auto"),
        ToolChoice::Required => serde_json::json!("required"),
        ToolChoice::None => serde_json::json!("none"),
        ToolChoice::Named { name } => serde_json::json!({
            "type": "function",
            "function": { "name": name }
        }),
    }
}

/// The OpenAI `response_format` spelling of a structured-output request.
pub fn response_format_to_openai(format: &ResponseFormat) -> serde_json::Value {
    match format {
        ResponseFormat::Text => serde_json::json!({ "type": "text" }),
        ResponseFormat::JsonObject => serde_json::json!({ "type": "json_object" }),
        ResponseFormat::JsonSchema {
            name,
            schema,
            strict,
        } => serde_json::json!({
            "type": "json_schema",
            "json_schema": {
                "name": name,
                "schema": schema,
                "strict": strict,
            }
        }),
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
        // Replayed verbatim: the router validates the shape but never rewrites
        // the provider's own web-search knobs.
        if let Some(web_search_options) = &self.web_search_options {
            body["web_search_options"] = web_search_options.clone();
        }
        self.apply_translation_fields_to_chat_body(body);
    }

    /// Writes the fields the widened hub carries onto an OpenAI chat body.
    ///
    /// Kept separate from the sampler knobs above because the two have
    /// different rules: the sampler block is unconditional, while a field here
    /// is only written when the client actually set it, so a request that sets
    /// nothing produces the same body it always did.
    fn apply_translation_fields_to_chat_body(&self, body: &mut serde_json::Value) {
        if let Some(choice) = &self.tool_choice {
            body["tool_choice"] = tool_choice_to_openai(choice);
        }
        if let Some(parallel) = self.parallel_tool_calls {
            body["parallel_tool_calls"] = serde_json::json!(parallel);
        }
        if let Some(format) = &self.response_format {
            body["response_format"] = response_format_to_openai(format);
        }
        if let Some(metadata) = &self.metadata {
            body["metadata"] = metadata.clone();
        }
        if let Some(user) = &self.user {
            body["user"] = serde_json::json!(user);
        }
        if let Some(tier) = &self.service_tier {
            body["service_tier"] = serde_json::json!(tier);
        }
        if let Some(store) = self.store {
            body["store"] = serde_json::json!(store);
        }
        if let Some(key) = &self.prompt_cache_key {
            body["prompt_cache_key"] = serde_json::json!(key);
        }
        if let Some(verbosity) = &self.verbosity {
            body["verbosity"] = serde_json::json!(verbosity);
        }
        if let Some(logprobs) = self.logprobs {
            body["logprobs"] = serde_json::json!(logprobs);
            if let Some(top) = self.top_logprobs {
                body["top_logprobs"] = serde_json::json!(top);
            }
        }
        // `top_k` is not part of the chat-completions API but several vendors
        // accept it. It is forwarded optimistically; a refusal is handled by
        // the provider's strip-and-retry path rather than failing the turn.
        if let Some(top_k) = self.top_k {
            body["top_k"] = serde_json::json!(top_k);
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
        assert!(decoded.cost_rates.is_none());
    }

    #[test]
    fn effective_cost_rates_are_exactly_what_the_router_supplied() {
        let config = ModelConfig::openai_compatible("https://api.example.com", "gpt-4o", None);

        assert!(
            config.effective_cost_rates().is_none(),
            "the provider layer must not invent prices for a model the router left unpriced"
        );
    }

    #[test]
    fn explicit_cost_rates_are_returned_unchanged() {
        let custom = ModelCostRates {
            input_per_million_usd: 1.23,
            output_per_million_usd: 4.56,
            cache_read_per_million_usd: Some(0.12),
            cache_write_per_million_usd: Some(0.34),
            reasoning_per_million_usd: Some(6.78),
            ..ModelCostRates::default()
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
        assert_eq!(
            options.max_transport_retries,
            PROVIDER_MAX_TRANSPORT_RETRIES
        );
        assert_eq!(options.max_status_retries, PROVIDER_MAX_TRANSPORT_RETRIES);
    }

    #[test]
    fn llm_stream_options_default_retries_are_pinned() {
        let options = LlmStreamOptions::default();
        assert_eq!(
            options.max_transport_retries,
            PROVIDER_MAX_TRANSPORT_RETRIES
        );
        assert_eq!(options.max_status_retries, PROVIDER_MAX_TRANSPORT_RETRIES);
        assert_eq!(options.max_retry_delay_ms, 30_000);
    }

    #[test]
    fn no_tiers_always_yields_the_base_rates() {
        let rates = ModelCostRates {
            input_per_million_usd: 2.0,
            output_per_million_usd: 8.0,
            cache_read_per_million_usd: Some(0.5),
            cache_write_per_million_usd: None,
            reasoning_per_million_usd: None,
            ..ModelCostRates::default()
        };

        let effective = rates.resolve(Some(10_000_000));

        assert_eq!(effective.input_per_million_usd, 2.0);
        assert_eq!(effective.output_per_million_usd, 8.0);
        assert_eq!(effective.cache_read_per_million_usd, Some(0.5));
        assert_eq!(effective.cache_write_per_million_usd, None);
        assert_eq!(effective.reasoning_per_million_usd, None);
    }

    fn tier_input(context_above: Option<u64>, start: Option<u32>, end: Option<u32>) -> CostTier {
        CostTier {
            context_above,
            start_minute_utc: start,
            end_minute_utc: end,
            input_per_million_usd: 9.0,
            output_per_million_usd: 90.0,
            ..CostTier::default()
        }
    }

    fn rates_with(tiers: Vec<CostTier>, minute: Option<u32>) -> ModelCostRates {
        ModelCostRates {
            input_per_million_usd: 1.0,
            output_per_million_usd: 2.0,
            tiers,
            request_start_minute_utc: minute,
            ..ModelCostRates::default()
        }
    }

    #[test]
    fn a_context_threshold_applies_strictly_above_it() {
        let rates = rates_with(vec![tier_input(Some(1_000), None, None)], None);

        assert_eq!(rates.resolve(Some(1_001)).input_per_million_usd, 9.0);
        assert_eq!(rates.resolve(Some(1_000)).input_per_million_usd, 1.0);
        assert_eq!(rates.resolve(Some(999)).input_per_million_usd, 1.0);
    }

    #[test]
    fn an_unknown_prompt_count_cannot_satisfy_a_context_tier() {
        let rates = rates_with(vec![tier_input(Some(1_000), None, None)], None);

        assert_eq!(
            rates.resolve(None).input_per_million_usd,
            1.0,
            "an upstream that reported no prompt must not be billed a context tier by guess"
        );
    }

    #[test]
    fn a_time_window_is_start_inclusive_and_end_exclusive() {
        let rates = rates_with(vec![tier_input(None, Some(540), Some(1020))], Some(540));
        assert_eq!(rates.resolve(None).input_per_million_usd, 9.0);

        let rates = rates_with(vec![tier_input(None, Some(540), Some(1020))], Some(1019));
        assert_eq!(rates.resolve(None).input_per_million_usd, 9.0);

        let rates = rates_with(vec![tier_input(None, Some(540), Some(1020))], Some(1020));
        assert_eq!(rates.resolve(None).input_per_million_usd, 1.0);

        let rates = rates_with(vec![tier_input(None, Some(540), Some(1020))], Some(539));
        assert_eq!(rates.resolve(None).input_per_million_usd, 1.0);
    }

    #[test]
    fn a_window_wraps_past_midnight() {
        let rates = rates_with(vec![tier_input(None, Some(1380), Some(60))], Some(1400));
        assert_eq!(rates.resolve(None).input_per_million_usd, 9.0);

        let rates = rates_with(vec![tier_input(None, Some(1380), Some(60))], Some(0));
        assert_eq!(rates.resolve(None).input_per_million_usd, 9.0);

        let rates = rates_with(vec![tier_input(None, Some(1380), Some(60))], Some(59));
        assert_eq!(rates.resolve(None).input_per_million_usd, 9.0);

        let rates = rates_with(vec![tier_input(None, Some(1380), Some(60))], Some(60));
        assert_eq!(rates.resolve(None).input_per_million_usd, 1.0);

        let rates = rates_with(vec![tier_input(None, Some(1380), Some(60))], Some(1200));
        assert_eq!(rates.resolve(None).input_per_million_usd, 1.0);
    }

    #[test]
    fn a_time_tier_never_matches_an_unstamped_request() {
        let rates = rates_with(vec![tier_input(None, Some(0), Some(720))], None);
        assert_eq!(
            rates.resolve(None).input_per_million_usd,
            1.0,
            "without a start minute a windowed tier cannot be chosen"
        );
    }

    #[test]
    fn half_a_time_window_is_not_a_match() {
        let rates = rates_with(vec![tier_input(None, Some(0), None)], Some(0));
        assert_eq!(rates.resolve(None).input_per_million_usd, 1.0);

        let rates = rates_with(vec![tier_input(None, None, Some(600))], Some(0));
        assert_eq!(rates.resolve(None).input_per_million_usd, 1.0);

        let rates = rates_with(vec![tier_input(None, Some(600), Some(600))], Some(600));
        assert_eq!(rates.resolve(None).input_per_million_usd, 1.0);
    }

    #[test]
    fn the_first_matching_tier_wins_over_later_ones() {
        let rates = rates_with(
            vec![
                // Off-peak discount listed first, so it keeps precedence at any
                // prompt size it matches.
                tier_input(None, Some(0), Some(480)),
                tier_input(Some(100_000), None, None),
            ],
            Some(120),
        );
        assert_eq!(rates.resolve(Some(1_000)).input_per_million_usd, 9.0);

        let rates = rates_with(
            vec![
                CostTier {
                    input_per_million_usd: 5.0,
                    ..tier_input(None, Some(0), Some(480))
                },
                CostTier {
                    input_per_million_usd: 6.0,
                    ..tier_input(None, None, None)
                },
            ],
            Some(120),
        );
        assert_eq!(
            rates.resolve(None).input_per_million_usd,
            5.0,
            "two matching tiers must resolve to the first one, not a mix"
        );
    }

    #[test]
    fn conditions_on_one_tier_are_anded() {
        let rates = rates_with(
            vec![tier_input(Some(1_000), Some(540), Some(1020))],
            Some(540),
        );

        // Time matches but the prompt is too small.
        assert_eq!(rates.resolve(Some(500)).input_per_million_usd, 1.0);
        // Prompt matches but the minute is outside the window.
        let outside = rates_with(
            vec![tier_input(Some(1_000), Some(540), Some(1020))],
            Some(60),
        );
        assert_eq!(outside.resolve(Some(5_000)).input_per_million_usd, 1.0);
        // Both hold.
        assert_eq!(rates.resolve(Some(5_000)).input_per_million_usd, 9.0);
    }

    #[test]
    fn a_tier_carries_the_whole_rate_set_including_nulls() {
        let rates = rates_with(
            vec![CostTier {
                input_per_million_usd: 9.0,
                output_per_million_usd: 90.0,
                cache_read_per_million_usd: None,
                cache_write_per_million_usd: Some(3.0),
                reasoning_per_million_usd: None,
                ..CostTier::default()
            }],
            None,
        );

        let effective = rates.resolve(None);

        assert_eq!(effective.cache_read_per_million_usd, None);
        assert_eq!(effective.cache_write_per_million_usd, Some(3.0));
        assert_eq!(
            effective.reasoning_per_million_usd, None,
            "a tier's null reasoning rate must not inherit the base premium"
        );
    }

    #[test]
    fn tiers_deserialize_from_the_shared_json_contract() {
        let json = r#"{
            "input_per_million_usd": 1.0,
            "output_per_million_usd": 2.0,
            "cache_read_per_million_usd": null,
            "cache_write_per_million_usd": null,
            "reasoning_per_million_usd": null,
            "tiers": [
                {
                    "context_above": 200000,
                    "start_minute_utc": null,
                    "end_minute_utc": null,
                    "input_per_million_usd": 1.5,
                    "output_per_million_usd": 3.0,
                    "cache_read_per_million_usd": null,
                    "cache_write_per_million_usd": null,
                    "reasoning_per_million_usd": null
                },
                {
                    "context_above": null,
                    "start_minute_utc": 480,
                    "end_minute_utc": 1200,
                    "input_per_million_usd": 2.0,
                    "output_per_million_usd": 4.0,
                    "cache_read_per_million_usd": 0.2,
                    "cache_write_per_million_usd": 0.4,
                    "reasoning_per_million_usd": 6.0
                }
            ]
        }"#;

        let rates: ModelCostRates = serde_json::from_str(json).expect("shared contract");

        assert_eq!(rates.tiers.len(), 2);
        assert_eq!(rates.tiers[0].context_above, Some(200_000));
        assert_eq!(rates.tiers[0].start_minute_utc, None);
        assert_eq!(rates.tiers[1].start_minute_utc, Some(480));
        assert_eq!(rates.tiers[1].end_minute_utc, Some(1200));
        assert_eq!(rates.tiers[1].cache_read_per_million_usd, Some(0.2));
        assert_eq!(rates.request_start_minute_utc, None);

        // The request-start minute is a runtime stamp, never part of the wire
        // price the router configures.
        let encoded = serde_json::to_value(&rates).expect("serialize");
        assert!(
            encoded.get("request_start_minute_utc").is_none(),
            "the runtime stamp must not be serialized"
        );
        assert_eq!(encoded["tiers"].as_array().expect("tiers").len(), 2);
    }

    #[test]
    fn a_missing_tiers_field_means_base_rates_only() {
        let rates: ModelCostRates = serde_json::from_str(
            r#"{"input_per_million_usd": 1.0, "output_per_million_usd": 2.0}"#,
        )
        .expect("base-only price");

        assert!(rates.tiers.is_empty());
        assert_eq!(rates.resolve(Some(1_000_000)).input_per_million_usd, 1.0);
    }

    #[test]
    fn effective_cost_rates_stamp_the_start_minute_only_when_unset() {
        let config = ModelConfig::openai_compatible("https://api.example.com", "gpt-4o", None);

        let mut unpriced = config.clone();
        unpriced.cost_rates = None;
        assert!(unpriced.effective_cost_rates().is_none());

        let mut stamped = config.clone();
        stamped.cost_rates = Some(rates_with(Vec::new(), Some(123)));
        assert_eq!(
            stamped
                .effective_cost_rates()
                .expect("rates")
                .request_start_minute_utc,
            Some(123),
            "a caller-provided minute must be kept"
        );

        let mut unstamped = config;
        unstamped.cost_rates = Some(rates_with(Vec::new(), None));
        let minute = unstamped
            .effective_cost_rates()
            .expect("rates")
            .request_start_minute_utc
            .expect("the provider layer stamps the start minute");
        assert!(minute < 1440, "a minute of the day, got {minute}");
    }
}

#[test]
fn a_bare_request_writes_none_of_the_widened_fields() {
    // The regression guard for the whole change: a request that uses only
    // today's fields must produce the body it always did.
    let mut body = serde_json::json!({});
    LlmStreamOptions::default().apply_to_openai_compatible_body(&mut body, false, false);

    for field in [
        "tool_choice",
        "parallel_tool_calls",
        "response_format",
        "metadata",
        "user",
        "service_tier",
        "store",
        "prompt_cache_key",
        "verbosity",
        "logprobs",
        "top_k",
    ] {
        assert!(
            body.get(field).is_none(),
            "`{field}` must be absent from a request that did not ask for it"
        );
    }
}

#[test]
fn the_widened_fields_reach_the_chat_body() {
    let options = LlmStreamOptions {
        tool_choice: Some(ToolChoice::Required),
        parallel_tool_calls: Some(false),
        response_format: Some(ResponseFormat::JsonObject),
        metadata: Some(serde_json::json!({ "trace": "abc" })),
        user: Some("user-42".to_string()),
        service_tier: Some("flex".to_string()),
        store: Some(true),
        prompt_cache_key: Some("key-1".to_string()),
        verbosity: Some("low".to_string()),
        logprobs: Some(true),
        top_logprobs: Some(3),
        top_k: Some(40),
        ..Default::default()
    };

    let mut body = serde_json::json!({});
    options.apply_to_openai_compatible_body(&mut body, false, false);

    assert_eq!(body["tool_choice"], serde_json::json!("required"));
    assert_eq!(body["parallel_tool_calls"], serde_json::json!(false));
    assert_eq!(
        body["response_format"]["type"],
        serde_json::json!("json_object")
    );
    assert_eq!(body["metadata"]["trace"], serde_json::json!("abc"));
    assert_eq!(body["user"], serde_json::json!("user-42"));
    assert_eq!(body["service_tier"], serde_json::json!("flex"));
    assert_eq!(body["store"], serde_json::json!(true));
    assert_eq!(body["prompt_cache_key"], serde_json::json!("key-1"));
    assert_eq!(body["verbosity"], serde_json::json!("low"));
    assert_eq!(body["logprobs"], serde_json::json!(true));
    assert_eq!(body["top_logprobs"], serde_json::json!(3));
    assert_eq!(body["top_k"], serde_json::json!(40));
}

#[test]
fn a_tool_choice_translates_to_each_dialect_spelling() {
    assert_eq!(
        tool_choice_to_openai(&ToolChoice::Required),
        serde_json::json!("required")
    );
    assert_eq!(
        tool_choice_to_openai(&ToolChoice::Named {
            name: "search".to_string()
        }),
        serde_json::json!({ "type": "function", "function": { "name": "search" } })
    );
}

#[test]
fn a_json_schema_response_format_carries_its_name_and_strictness() {
    let format = ResponseFormat::JsonSchema {
        name: "ticket".to_string(),
        schema: serde_json::json!({ "type": "object" }),
        strict: true,
    };

    let wire = response_format_to_openai(&format);

    assert_eq!(wire["type"], serde_json::json!("json_schema"));
    assert_eq!(wire["json_schema"]["name"], serde_json::json!("ticket"));
    assert_eq!(wire["json_schema"]["strict"], serde_json::json!(true));
}
