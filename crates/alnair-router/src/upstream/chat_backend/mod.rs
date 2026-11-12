//! The single seam to the vendored provider layer.
//!
//! **This is the only module permitted to reach into the provider stack.** The
//! vendored crate is referenced from this file alone, and
//! `vendored_llm_layer_is_imported_from_exactly_one_file` (in
//! `tests/routes.rs`) enforces it, so every coupling point stays in one
//! reviewable place.
//!
//! Layout:
//!
//! - [`hub`] — the canonical types every endpoint and provider translates
//!   against. No provider-crate names appear there.
//! - this file — the conversion layer: router types → provider options, and
//!   provider chunks → [`StreamChunk`].
//!
//! Verify the boundary with:
//!
//! ```text
//! cargo test -p alnair-router vendored_llm_layer_is_imported_from_exactly_one_file
//! ```

use std::collections::BTreeMap;

mod web;
pub use alnair_llm::WebRequest;
pub use web::{hosted_web, supports_hosted_web};

use futures::StreamExt;
use futures::stream::{BoxStream, Stream};

use crate::error::{Error, Result};
use crate::pricing::Price;
use alnair_llm::{
    AuthStyle, CacheRetention, ContentPart, CostTier, ImageUrlContentPart, LlmStreamChunk,
    LlmStreamOptions, Message, MessageContent, MessageToolCall, ModelConfig, ModelCostRates,
    TextContentPart, ThinkingLevel,
};

mod capability;
mod filter;
mod hub;

pub(crate) use capability::effective_wire_api;
pub use capability::{
    TranslationSupport, UnsupportedFeature, translation_support, unsupported_features,
};
pub(crate) use filter::filter_targets;
pub use hub::*;

/// Re-exported at crate visibility so the seam's own submodules can name the
/// provider enums without each reaching into the provider crate; the boundary
/// guard allows exactly one file to do that.
pub(crate) use alnair_llm::{ProviderType, WireApi};

/// Re-exported provider registry, so callers never name the provider crate.
pub use alnair_llm::ProviderRegistry;

/// Re-exported conversation message, so callers never name the provider crate.
pub type RouterMessage = Message;

/// Builds a plain-text message.
pub fn message_text(role: impl Into<String>, text: impl Into<String>) -> RouterMessage {
    Message::new(role, text)
}

/// Replaces a message's content with plain text, dropping any multimodal parts.
///
/// Used by the token-saving pipeline, which rewrites message bodies in place
/// without touching tool calls or thinking blocks.
pub fn set_message_text(message: &mut RouterMessage, text: impl Into<String>) {
    message.content = MessageContent::Text(text.into());
}

/// True when a message carries plain text only, so a caller may rewrite it
/// in place without destroying multimodal parts.
pub fn message_is_text(message: &RouterMessage) -> bool {
    matches!(message.content, MessageContent::Text(_))
}

/// Renders a message's content as wire parts, or `None` when it is plain text.
///
/// Callers outside this module cannot name the provider-layer content types, so
/// this is how the rest of the crate inspects (and tests) a multipart message.
pub fn message_parts_json(message: &RouterMessage) -> Option<Vec<serde_json::Value>> {
    match &message.content {
        MessageContent::Text(_) => None,
        MessageContent::Parts(parts) => Some(parts.iter().map(ContentPart::to_json).collect()),
    }
}

/// Builds a multimodal message from ordered parts.
pub fn message_parts(role: impl Into<String>, parts: Vec<MessagePart>) -> RouterMessage {
    let parts = parts
        .into_iter()
        .map(|part| match part {
            MessagePart::Text(text) => ContentPart::Text(TextContentPart {
                content_type: "text".to_string(),
                text,
            }),
            MessagePart::ImageUrl(url) => ContentPart::ImageUrl(ImageUrlContentPart {
                content_type: "image_url".to_string(),
                image_url: alnair_llm::ImageUrl { url },
            }),
            MessagePart::Media(raw) => ContentPart::Media(alnair_llm::MediaContentPart { raw }),
        })
        .collect();

    RouterMessage {
        role: role.into(),
        content: MessageContent::Parts(parts),
        thinking: None,
        thinking_signature: None,
        redacted_thinking: None,
        tool_call_id: None,
        tool_calls: None,
        cache_control: false,
    }
}

/// Builds an assistant message that requests tool calls.
pub fn message_assistant_tool_calls(
    content: impl Into<String>,
    calls: Vec<ToolCallSpec>,
) -> RouterMessage {
    RouterMessage {
        role: "assistant".to_string(),
        content: MessageContent::Text(content.into()),
        thinking: None,
        thinking_signature: None,
        redacted_thinking: None,
        tool_call_id: None,
        tool_calls: Some(
            calls
                .into_iter()
                .map(|call| MessageToolCall {
                    id: call.id,
                    name: call.name,
                    arguments: call.arguments,
                })
                .collect(),
        ),
        cache_control: false,
    }
}

/// Builds a tool-result message answering a prior tool call.
pub fn message_tool_result(
    content: impl Into<String>,
    tool_call_id: impl Into<String>,
) -> RouterMessage {
    RouterMessage {
        role: "tool".to_string(),
        content: MessageContent::Text(content.into()),
        thinking: None,
        thinking_signature: None,
        redacted_thinking: None,
        tool_call_id: Some(tool_call_id.into()),
        tool_calls: None,
        cache_control: false,
    }
}

/// Parses a thinking effort string into the provider enum, ignoring unknowns.
///
/// OpenAI's own vocabulary is `none|minimal|low|medium|high|xhigh|max`, where
/// `xhigh` and `max` are distinct levels and `max` is the highest. The provider
/// layer has one top tier ([`ThinkingLevel::Max`]), so `xhigh` also lands there;
/// each dialect then spells it the way its endpoint documents.
fn parse_thinking_level(value: Option<&str>) -> Option<ThinkingLevel> {
    match value? {
        "minimal" => Some(ThinkingLevel::Minimal),
        "low" => Some(ThinkingLevel::Low),
        "medium" => Some(ThinkingLevel::Medium),
        "high" => Some(ThinkingLevel::High),
        "xhigh" | "max" => Some(ThinkingLevel::Max),
        _ => None,
    }
}

/// Maps a connection's stored cache-retention value onto the provider enum.
///
/// Anything unrecognised keeps today's behaviour (`none`) but is logged: the
/// repository validates on write, so a stray value means a hand-edited row or a
/// migration artefact, and silently changing what the upstream receives would
/// hide that.
pub fn cache_retention_from_str(value: &str) -> CacheRetention {
    match value {
        "none" | "" => CacheRetention::None,
        "short" => CacheRetention::Short,
        "long" => CacheRetention::Long,
        other => {
            tracing::warn!(
                value = other,
                "unknown cache_retention on the connection row; treating it as none"
            );
            CacheRetention::None
        }
    }
}

/// Maps a connection's stored auth-style value onto the provider enum.
///
/// Anything unrecognised falls back to the provider default (`x-api-key` for
/// Anthropic-native), so a bad value degrades to today's behaviour rather than
/// silently switching the header. The fallback is logged for the same reason as
/// [`cache_retention_from_str`].
pub fn auth_style_from_str(value: &str) -> AuthStyle {
    match value {
        "api_key" | "" => AuthStyle::ApiKey,
        "bearer" => AuthStyle::Bearer,
        other => {
            tracing::warn!(
                value = other,
                "unknown auth_style on the connection row; treating it as api_key"
            );
            AuthStyle::ApiKey
        }
    }
}

/// Maps a connection's stored wire-api value onto the provider enum.
///
/// Anything unrecognised falls back to `/chat/completions`, which is what every
/// OpenAI-compatible endpoint serves, so a bad value degrades to today's
/// behaviour rather than failing the request. The fallback is logged.
pub fn wire_api_from_str(value: &str) -> WireApi {
    match value {
        "chat" | "" => WireApi::Chat,
        "responses" => WireApi::Responses,
        other => {
            tracing::warn!(
                value = other,
                "unknown wire_api on the connection row; treating it as chat"
            );
            WireApi::Chat
        }
    }
}

/// Provider retry behaviour applied inside a single tier, before the executor
/// fails over to the next one.
///
/// Transport retries (a connection that never answered) are deliberately fixed
/// and small: the executor already fans one request across tiers and
/// credentials, so letting both layers retry freely multiplies upstream calls.
/// [`Self::max_retries_per_tier`] is the status-retry budget only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Retries after a retryable HTTP status (408/429/5xx/529), per tier.
    pub max_retries_per_tier: usize,
    /// Upper bound for the exponential retry delay, in milliseconds.
    pub max_retry_delay_ms: u64,
    /// Retries after a failed connection attempt, per tier. Fixed at one.
    pub transport_retries: usize,
}

/// Worst-case upstream calls for one client request, before credential
/// rotation: `tiers × (1 + status retries + transport retries)`.
///
/// Kept next to the policy so the multiplication is stated once rather than
/// re-derived (and mis-derived) at the call sites.
pub fn worst_case_upstream_calls(policy: RetryPolicy, tiers: usize) -> usize {
    tiers * (1 + policy.max_retries_per_tier + policy.transport_retries)
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries_per_tier: 2,
            max_retry_delay_ms: 30_000,
            transport_retries: 1,
        }
    }
}

impl GenerationOptions {
    /// Converts into the provider options struct.
    fn to_stream_options(&self, retry: RetryPolicy) -> LlmStreamOptions {
        LlmStreamOptions {
            temperature: self.temperature,
            top_p: self.top_p,
            top_k: self.top_k,
            max_tokens: self.max_tokens,
            stop: self.stop.clone(),
            seed: self.seed,
            presence_penalty: self.presence_penalty,
            frequency_penalty: self.frequency_penalty,
            tool_choice: self.tool_choice.as_ref().map(to_provider_tool_choice),
            parallel_tool_calls: self.parallel_tool_calls,
            response_format: self
                .response_format
                .as_ref()
                .map(to_provider_response_format),
            metadata: self.metadata.clone(),
            user: self.user.clone(),
            service_tier: self.service_tier.clone(),
            store: self.store,
            prompt_cache_key: self.prompt_cache_key.clone(),
            verbosity: self.verbosity.clone(),
            logprobs: self.logprobs,
            top_logprobs: self.top_logprobs,
            include: self.include.clone(),
            max_status_retries: retry.max_retries_per_tier,
            max_transport_retries: retry.transport_retries,
            max_retry_delay_ms: retry.max_retry_delay_ms,
            thinking_level: parse_thinking_level(self.thinking_level.as_deref()),
            web_search_options: self.web_search_options.clone(),
            ..Default::default()
        }
    }
}

/// Maps the hub's tool choice onto the provider enum.
fn to_provider_tool_choice(choice: &ToolChoice) -> alnair_llm::ToolChoice {
    match choice {
        ToolChoice::Auto => alnair_llm::ToolChoice::Auto,
        ToolChoice::Required => alnair_llm::ToolChoice::Required,
        ToolChoice::None => alnair_llm::ToolChoice::None,
        ToolChoice::Named { name } => alnair_llm::ToolChoice::Named { name: name.clone() },
    }
}

/// Maps the hub's structured-output request onto the provider enum.
fn to_provider_response_format(format: &ResponseFormat) -> alnair_llm::ResponseFormat {
    match format {
        ResponseFormat::Text => alnair_llm::ResponseFormat::Text,
        ResponseFormat::JsonObject => alnair_llm::ResponseFormat::JsonObject,
        ResponseFormat::JsonSchema {
            name,
            schema,
            strict,
        } => alnair_llm::ResponseFormat::JsonSchema {
            name: name.clone(),
            schema: schema.clone(),
            strict: *strict,
        },
    }
}

/// Maps a router provider-type string onto the vendored enum.
///
/// Returns [`Error::UnsupportedProviderType`] for anything else, so an
/// unsupported value can never reach the provider layer.
pub fn provider_type_from_str(value: &str) -> Result<ProviderType> {
    match value {
        "openai-compatible" => Ok(ProviderType::OpenaiCompatible),
        "anthropic-native" => Ok(ProviderType::AnthropicNative),
        "command-code" => Ok(ProviderType::CommandCode),
        "codebuddy-intl" => Ok(ProviderType::CodeBuddyIntl),
        "openai-codex" => Ok(ProviderType::OpenAiCodex),
        other => Err(Error::UnsupportedProviderType(other.to_string())),
    }
}

/// Returns the `type` of the first media part in `messages`, if any.
fn first_media_part_kind(messages: &[RouterMessage]) -> Option<String> {
    messages.iter().find_map(|message| match &message.content {
        MessageContent::Text(_) => None,
        MessageContent::Parts(parts) => parts.iter().find_map(|part| match part {
            ContentPart::Media(media) => Some(media.kind().to_string()),
            ContentPart::Text(_) | ContentPart::ImageUrl(_) => None,
        }),
    })
}

/// Every provider family the router can dispatch to, in validation order.
///
/// Kept here because this module is the only place allowed to name the
/// provider crate; write-time validation and the dashboard read it from here.
pub fn supported_provider_types() -> &'static [&'static str; 5] {
    use std::sync::OnceLock;
    static KEYS: OnceLock<[&'static str; 5]> = OnceLock::new();
    KEYS.get_or_init(|| ProviderType::ALL.map(|provider_type| provider_type.as_key()))
}

/// Builds a provider registry with the supported implementations registered.
pub fn default_registry() -> ProviderRegistry {
    ProviderRegistry::with_defaults()
}

/// A boxed stream of chunks from the provider layer.
pub type ChunkStream = BoxStream<'static, Result<StreamChunk>>;

/// Builds the model config handed to the provider layer.
#[allow(clippy::too_many_arguments)]
fn model_config(
    provider_type: ProviderType,
    base_url: &str,
    model: &str,
    api_key: Option<&str>,
    auth_style: AuthStyle,
    wire_api: WireApi,
    custom_headers: BTreeMap<String, String>,
    price: Option<Price>,
    supports_cache_control: bool,
) -> ModelConfig {
    // Reasoning-capable providers get the effort forwarded. OpenAI-compatible
    // upstreams are included because the subset that implements reasoning (the
    // GPT-5 family, vLLM builds with a reasoning parser, most gateways) reads
    // `reasoning_effort` or `reasoning.effort`; the subset that does not is
    // handled by the provider's retry-without-reasoning path, which strips the
    // field and resends rather than failing the turn.
    let supports_thinking = matches!(
        provider_type,
        ProviderType::OpenaiCompatible
            | ProviderType::CodeBuddyIntl
            | ProviderType::AnthropicNative
            | ProviderType::OpenAiCodex
    );

    // The Codex backend needs the ChatGPT account id, which is a claim of the
    // OAuth bearer. It is derived from the token, not stored on the connection,
    // so a refreshed token always carries its own account.
    let chatgpt_account_id = match provider_type {
        ProviderType::OpenAiCodex => api_key.and_then(chatgpt_account_id_from_jwt),
        _ => None,
    };

    ModelConfig {
        provider_type,
        base_url: base_url.to_string(),
        model_id: model.to_string(),
        api_key: api_key.map(str::to_string),
        auth_style,
        wire_api,
        custom_headers,
        chatgpt_account_id,
        // Capability gates are opt-in: only providers with a known wire shape
        // enable thinking; CodeBuddy expects OpenAI reasoning parameters.
        supports_thinking,
        // Cache breakpoints are opt-in per connection because OpenAI-compatible
        // endpoints may reject the extra field.
        supports_cache_control,
        cost_rates: price.map(cost_rates),
    }
}

/// The `chatgpt_account_id` claim of a ChatGPT/Codex access token.
///
/// The token is a JWT whose `https://api.openai.com/auth` claim namespace
/// carries the account id. The Codex backend refuses a request without the
/// matching `chatgpt-account-id` header, so this doubles as the login gate: a
/// token that does not decode to an account id cannot authorize a turn.
///
/// SECURITY: the token, its payload and the returned id are never logged.
pub fn chatgpt_account_id_from_jwt(token: &str) -> Option<String> {
    use base64::Engine;

    // A JWT is exactly three dot-separated segments; anything else is not a
    // token this can read, so fail closed rather than decode a lookalike.
    let mut segments = token.split('.');
    let (_header, payload, _signature) = (segments.next()?, segments.next()?, segments.next()?);
    if segments.next().is_some() {
        return None;
    }

    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(payload))
        .ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let account_id = claims
        .get("https://api.openai.com/auth")?
        .get("chatgpt_account_id")?
        .as_str()?;

    (!account_id.trim().is_empty()).then(|| account_id.to_string())
}

/// Converts a router price row into the provider layer's rate struct.
fn cost_rates(price: Price) -> ModelCostRates {
    ModelCostRates {
        input_per_million_usd: price.input_per_million_usd,
        output_per_million_usd: price.output_per_million_usd,
        cache_read_per_million_usd: price.cache_read_per_million_usd,
        cache_write_per_million_usd: price.cache_write_per_million_usd,
        reasoning_per_million_usd: price.reasoning_per_million_usd,
        tiers: price
            .tiers
            .into_iter()
            .map(|tier| CostTier {
                context_above: tier.context_above,
                start_minute_utc: tier.start_minute_utc,
                end_minute_utc: tier.end_minute_utc,
                input_per_million_usd: tier.input_per_million_usd,
                output_per_million_usd: tier.output_per_million_usd,
                cache_read_per_million_usd: tier.cache_read_per_million_usd,
                cache_write_per_million_usd: tier.cache_write_per_million_usd,
                reasoning_per_million_usd: tier.reasoning_per_million_usd,
            })
            .collect(),
        request_start_minute_utc: None,
    }
}

/// Streams a completion through the vendored provider registry.
///
/// `streaming` selects the provider's SSE path or its one-shot path; both
/// surface the same chunk stream.
#[allow(clippy::too_many_arguments)]
pub fn stream(
    registry: std::sync::Arc<ProviderRegistry>,
    provider_type: &str,
    base_url: &str,
    model: &str,
    messages: Vec<RouterMessage>,
    api_key: Option<&str>,
    options: Option<&GenerationOptions>,
    retry: RetryPolicy,
    streaming: bool,
    tools: Option<Vec<serde_json::Value>>,
    custom_headers: BTreeMap<String, String>,
    price: Option<Price>,
    cache_retention: CacheRetention,
    auth_style: AuthStyle,
    wire_api: WireApi,
) -> Result<ChunkStream> {
    let provider_type = provider_type_from_str(provider_type)?;

    if registry.get(&provider_type).is_none() {
        return Err(Error::UnsupportedProviderType(format!(
            "no provider registered for {provider_type:?}"
        )));
    }

    // Media parts are only meaningful on an OpenAI-compatible upstream, where
    // they are replayed verbatim. Every other provider stack can only carry
    // text and images, so accepting them would silently drop the media and let
    // the model answer a question it never saw. Reject the request instead.
    if provider_type != ProviderType::OpenaiCompatible
        && let Some(kind) = first_media_part_kind(&messages)
    {
        return Err(Error::BadRequest(format!(
            "content part `{kind}` is only supported on openai-compatible connections; \
             this model is served by a {} connection",
            provider_type.as_key()
        )));
    }

    // Codex translates this option into its hosted Responses web tool.
    if provider_type != ProviderType::OpenaiCompatible
        && !registry
            .get(&provider_type)
            .is_some_and(|provider| provider.capabilities().web_search)
        && options.is_some_and(|options| options.web_search_options.is_some())
    {
        return Err(Error::BadRequest(format!(
            "`web_search_options` requires a provider with hosted web search; \
             this model is served by a {} connection",
            provider_type.as_key()
        )));
    }

    let config = model_config(
        provider_type.clone(),
        base_url,
        model,
        api_key,
        auth_style,
        wire_api,
        custom_headers,
        price,
        !matches!(cache_retention, CacheRetention::None),
    );
    let stream_options = stream_options(options, retry, cache_retention);

    // The provider borrows `config` and `stream_options` for `'a`. Moving them
    // into the generator keeps the borrow valid for the stream's whole life
    // without leaking or requiring unsafe code.
    let chunks: BoxStream<'static, Result<StreamChunk>> = Box::pin(async_stream::stream! {
        let Some(provider) = registry.get(&provider_type) else {
            yield Err(Error::UnsupportedProviderType(format!(
                "no provider registered for {provider_type:?}"
            )));
            return;
        };

        let inner = if streaming {
            provider.stream(&config, messages, &stream_options, tools)
        } else {
            provider.complete(&config, messages, &stream_options, tools)
        };
        futures::pin_mut!(inner);

        while let Some(chunk) = inner.next().await {
            yield to_stream_chunk(chunk);
        }
    });

    Ok(chunks)
}

/// Builds provider options, always applying the router's retry policy even
/// when the request carried no generation options.
fn stream_options(
    options: Option<&GenerationOptions>,
    retry: RetryPolicy,
    cache_retention: CacheRetention,
) -> LlmStreamOptions {
    let mut options = match options {
        Some(options) => options.to_stream_options(retry),
        None => GenerationOptions::default().to_stream_options(retry),
    };
    // Retention is a property of the target connection, not the request.
    options.cache_retention = cache_retention;
    options
}

/// Converts a vendored chunk into a router-owned chunk.
fn to_stream_chunk(
    chunk: std::result::Result<LlmStreamChunk, alnair_llm::ChatError>,
) -> Result<StreamChunk> {
    match chunk {
        Ok(LlmStreamChunk::Text(text)) => Ok(StreamChunk::Text(text)),
        Ok(LlmStreamChunk::Thinking(text)) => Ok(StreamChunk::Thinking(text)),
        Ok(LlmStreamChunk::ThinkingSignature(signature)) => {
            Ok(StreamChunk::ThinkingSignature(signature))
        }
        Ok(LlmStreamChunk::RedactedThinking(data)) => Ok(StreamChunk::RedactedThinking(data)),
        Ok(LlmStreamChunk::ToolCall {
            id,
            name,
            arguments,
        }) => Ok(StreamChunk::ToolCall {
            id,
            name,
            arguments: arguments.to_string(),
        }),
        Ok(LlmStreamChunk::Usage {
            prompt_eval_count,
            eval_count,
            cached_prompt_eval_count,
            reasoning_eval_count,
            cost_input_usd,
            cost_output_usd,
            cost_reasoning_usd,
            cost_cache_usd,
            ..
        }) => Ok(StreamChunk::Usage(TokenUsage {
            prompt_tokens: prompt_eval_count.unwrap_or(0),
            completion_tokens: eval_count.unwrap_or(0),
            cached_tokens: cached_prompt_eval_count.unwrap_or(0),
            reasoning_tokens: reasoning_eval_count.unwrap_or(0),
            cost_input_usd: cost_input_usd.unwrap_or(0.0),
            cost_output_usd: cost_output_usd.unwrap_or(0.0),
            cost_reasoning_usd: cost_reasoning_usd.unwrap_or(0.0),
            cost_cache_usd: cost_cache_usd.unwrap_or(0.0),
            cost_usd: cost_input_usd.unwrap_or(0.0)
                + cost_output_usd.unwrap_or(0.0)
                + cost_reasoning_usd.unwrap_or(0.0),
        })),
        Ok(LlmStreamChunk::Citation(citation)) => Ok(StreamChunk::Citation(Citation {
            url: citation.url,
            title: citation.title,
            start_index: citation.start_index,
            end_index: citation.end_index,
        })),
        Ok(LlmStreamChunk::Done(reason)) => Ok(StreamChunk::Done(reason)),
        // A rejected request is not an upstream fault: an Anthropic connection
        // with no credential, or a body the provider layer refuses to build,
        // must reach the client as a 400 rather than a 502 gateway error.
        Err(alnair_llm::ChatError::BadRequest(message)) => Err(Error::BadRequest(message)),
        Err(error) => Err(Error::Upstream(error.to_string())),
    }
}

/// Drains a stream into a single completion response.
pub async fn collect(stream: ChunkStream) -> Result<CompletionResponse> {
    let mut stream = std::pin::pin!(stream);
    let mut response = CompletionResponse::default();

    while let Some(chunk) = stream.next().await {
        match chunk? {
            StreamChunk::Text(text) => response.content.push_str(&text),
            StreamChunk::Thinking(text) => {
                response
                    .thinking
                    .get_or_insert_with(String::new)
                    .push_str(&text);
            }
            StreamChunk::ThinkingSignature(signature) => {
                response.thinking_signature = Some(signature);
            }
            StreamChunk::RedactedThinking(data) => {
                response.redacted_thinking = Some(data);
            }
            StreamChunk::ToolCall {
                id,
                name,
                arguments,
            } => response.tool_calls.push(ToolCallSpec {
                id,
                name,
                arguments,
            }),
            StreamChunk::Citation(citation) => response.citations.push(citation),
            StreamChunk::Usage(usage) => response.usage = Some(usage),
            StreamChunk::Done(reason) => {
                response.finish_reason = Some(openai_finish_reason(
                    reason.as_deref(),
                    !response.tool_calls.is_empty(),
                ));
                break;
            }
        }
    }

    // A stream that ends without a `Done` chunk still completed: the provider
    // closed the connection. Leaving the reason empty would let a caller's
    // fallback word a truncated answer as if it finished normally, so resolve
    // it here, next to the one function that owns the vocabulary.
    if response.finish_reason.is_none() {
        response.finish_reason = Some(openai_finish_reason(None, !response.tool_calls.is_empty()));
    }

    Ok(response)
}

/// Convenience adapter so callers can treat the backend stream as a `Stream`.
pub fn into_stream(chunks: ChunkStream) -> impl Stream<Item = Result<StreamChunk>> + Send {
    chunks
}

#[cfg(test)]
mod tests;
