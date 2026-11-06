//! The single seam to the vendored provider layer.
//!
//! **This is the only module in the crate permitted to reach into
//! [`alnair_llm`].** The provider stack is vendored so the router builds
//! standalone; funnelling every coupling point through here keeps that boundary
//! honest. Verify with:
//!
//! ```text
//! grep -rl "alnair_llm" src | grep -v "src/upstream/chat_backend.rs"   # expect no output
//! ```

use std::collections::BTreeMap;

use futures::StreamExt;
use futures::stream::{BoxStream, Stream};

use crate::error::{Error, Result};
use crate::pricing::Price;
use alnair_llm::{
    AuthStyle, CacheRetention, ContentPart, ImageUrlContentPart, LlmStreamChunk, LlmStreamOptions,
    Message, MessageContent, MessageToolCall, ModelConfig, ModelCostRates, ProviderType,
    TextContentPart, ThinkingLevel, WireApi,
};

/// Re-exported provider registry, so callers never name `alnair_llm` directly.
pub use alnair_llm::ProviderRegistry;

/// Re-exported conversation message, so callers never name `alnair_llm` directly.
pub type RouterMessage = Message;

/// A single multimodal part of a message, in router-owned form.
#[derive(Debug, Clone)]
pub enum MessagePart {
    Text(String),
    ImageUrl(String),
    /// An audio, video or file part, kept as the exact wire object the client
    /// sent. OpenAI-compatible upstreams disagree on the spelling of these
    /// parts, so the router validates the shape and replays it verbatim rather
    /// than translating it into one canonical form.
    Media(serde_json::Value),
}

/// A tool call an assistant message is asking the model to run.
#[derive(Debug, Clone)]
pub struct ToolCallSpec {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

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

/// Token accounting reported by a provider.
#[derive(Debug, Clone, Default, Copy)]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_tokens: u64,
    /// Reasoning tokens included in `completion_tokens`, when reported.
    pub reasoning_tokens: u64,
    /// Input-token cost in USD.
    pub cost_input_usd: f64,
    /// Output-token cost in USD (reasoning premium excluded).
    pub cost_output_usd: f64,
    /// Reasoning premium over the output rate, in USD.
    pub cost_reasoning_usd: f64,
    /// The part of `cost_input_usd` billed at the cache rate, in USD.
    pub cost_cache_usd: f64,
    /// Total cost in USD: input + output + reasoning.
    pub cost_usd: f64,
}

/// One streamed chunk from an upstream provider.
#[derive(Debug, Clone)]
pub enum StreamChunk {
    Text(String),
    Thinking(String),
    /// Signature for the preceding thinking block, replayed to Anthropic on the
    /// next turn.
    ThinkingSignature(String),
    /// Redacted thinking block payload, replayed verbatim.
    RedactedThinking(String),
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
    Usage(TokenUsage),
    /// Terminal chunk, carrying the provider's finish reason when it reports one.
    Done(Option<String>),
}

/// Normalizes a provider finish reason onto OpenAI's vocabulary.
///
/// Providers disagree on the terminal token (`tool_use` vs `tool_calls`,
/// `end_turn` vs `stop`), so the wire shape is decided here rather than letting
/// an Anthropic-flavoured reason leak to an OpenAI client. A completed tool call
/// always wins over a generic `stop`, because clients drive their tool loop off
/// `finish_reason`.
pub fn openai_finish_reason(reason: Option<&str>, has_tool_calls: bool) -> String {
    match reason.unwrap_or_default() {
        "tool_calls" | "tool-calls" | "tool_use" | "toolUse" => "tool_calls",
        "length" | "max_tokens" | "max-tokens" | "max_output_tokens" => "length",
        _ if has_tool_calls => "tool_calls",
        _ => "stop",
    }
    .to_string()
}

/// Normalizes a provider finish reason onto Anthropic's vocabulary.
///
/// The inverse of [`openai_finish_reason`]: an OpenAI-flavoured `length` becomes
/// `max_tokens`, and any completed tool call becomes `tool_use`.
pub fn anthropic_stop_reason(reason: Option<&str>, has_tool_calls: bool) -> String {
    match reason.unwrap_or_default() {
        "tool_calls" | "tool-calls" | "tool_use" | "toolUse" => "tool_use",
        "length" | "max_tokens" | "max-tokens" | "max_output_tokens" => "max_tokens",
        "stop_sequence" => "stop_sequence",
        _ if has_tool_calls => "tool_use",
        _ => "end_turn",
    }
    .to_string()
}

/// Completion result in router-owned types.
#[derive(Debug, Clone, Default)]
pub struct CompletionResponse {
    pub content: String,
    /// Concatenated reasoning text, when the provider emits any.
    pub thinking: Option<String>,
    /// Anthropic signature for `thinking`, replayed on the next turn.
    pub thinking_signature: Option<String>,
    /// Anthropic `redacted_thinking` payload, replayed verbatim.
    pub redacted_thinking: Option<String>,
    pub finish_reason: Option<String>,
    pub usage: Option<TokenUsage>,
    /// Tool calls the model requested, in arrival order.
    pub tool_calls: Vec<ToolCallSpec>,
}

/// Generation options in router-owned form.
#[derive(Debug, Clone, Default)]
pub struct GenerationOptions {
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub max_tokens: Option<i32>,
    pub stop: Option<Vec<String>>,
    pub seed: Option<i64>,
    pub presence_penalty: Option<f64>,
    pub frequency_penalty: Option<f64>,
    /// Extended-thinking effort, one of `minimal|low|medium|high|max`.
    pub thinking_level: Option<String>,
    /// OpenAI `web_search_options`, carried through to an OpenAI-compatible
    /// upstream untouched. The router validates the shape but never rewrites
    /// it, because each provider exposes its own knobs.
    pub web_search_options: Option<serde_json::Value>,
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

/// Maps an Anthropic `thinking.budget_tokens` value onto an effort level.
pub fn thinking_level_from_budget(budget: u64) -> &'static str {
    match budget {
        0..=1024 => "minimal",
        1025..=4096 => "low",
        4097..=8192 => "medium",
        8193..=16384 => "high",
        _ => "max",
    }
}

/// Maps a connection's stored cache-retention value onto the provider enum.
pub fn cache_retention_from_str(value: &str) -> CacheRetention {
    match value {
        "short" => CacheRetention::Short,
        "long" => CacheRetention::Long,
        _ => CacheRetention::None,
    }
}

/// Maps a connection's stored auth-style value onto the provider enum.
///
/// Anything unrecognised falls back to the provider default (`x-api-key` for
/// Anthropic-native), so a bad value degrades to today's behaviour rather than
/// silently switching the header.
pub fn auth_style_from_str(value: &str) -> AuthStyle {
    match value {
        "bearer" => AuthStyle::Bearer,
        _ => AuthStyle::ApiKey,
    }
}

/// Maps a connection's stored wire-api value onto the provider enum.
///
/// Anything unrecognised falls back to `/chat/completions`, which is what every
/// OpenAI-compatible endpoint serves, so a bad value degrades to today's
/// behaviour rather than failing the request.
pub fn wire_api_from_str(value: &str) -> WireApi {
    match value {
        "responses" => WireApi::Responses,
        _ => WireApi::Chat,
    }
}

/// Provider retry behaviour applied inside a single tier, before the executor
/// fails over to the next one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Retries after the first attempt fails, per tier.
    pub max_retries_per_tier: usize,
    /// Upper bound for the exponential retry delay, in milliseconds.
    pub max_retry_delay_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries_per_tier: 2,
            max_retry_delay_ms: 30_000,
        }
    }
}

impl GenerationOptions {
    /// Converts into the provider options struct.
    fn to_stream_options(&self, retry: RetryPolicy) -> LlmStreamOptions {
        LlmStreamOptions {
            temperature: self.temperature,
            top_p: self.top_p,
            max_tokens: self.max_tokens,
            stop: self.stop.clone(),
            seed: self.seed,
            presence_penalty: self.presence_penalty,
            frequency_penalty: self.frequency_penalty,
            max_retries: retry.max_retries_per_tier,
            max_retry_delay_ms: retry.max_retry_delay_ms,
            thinking_level: parse_thinking_level(self.thinking_level.as_deref()),
            web_search_options: self.web_search_options.clone(),
            ..Default::default()
        }
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

/// Human-readable provider name for error messages.
fn provider_type_label(provider_type: &ProviderType) -> &'static str {
    match provider_type {
        ProviderType::OpenaiCompatible => "openai-compatible",
        ProviderType::AnthropicNative => "anthropic-native",
        ProviderType::CommandCode => "command-code",
        ProviderType::CodeBuddyIntl => "codebuddy-intl",
        ProviderType::OpenAiCodex => "openai-codex",
    }
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
        supports_vision: false,
        // Capability gates are opt-in: only providers with a known wire shape
        // enable thinking; CodeBuddy expects OpenAI reasoning parameters.
        supports_thinking,
        // Cache breakpoints are opt-in per connection because OpenAI-compatible
        // endpoints may reject the extra field.
        supports_cache_control,
        context_window: 0,
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
            provider_type_label(&provider_type)
        )));
    }

    // `web_search_options` is OpenAI's own knob; only an OpenAI-compatible
    // upstream can carry it. Accepting it elsewhere would drop the option and
    // answer without searching.
    if provider_type != ProviderType::OpenaiCompatible
        && options.is_some_and(|options| options.web_search_options.is_some())
    {
        return Err(Error::BadRequest(format!(
            "`web_search_options` is only supported on openai-compatible connections; \
             this model is served by a {} connection",
            provider_type_label(&provider_type)
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
        Ok(LlmStreamChunk::Done(reason)) => Ok(StreamChunk::Done(reason)),
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

    Ok(response)
}

/// Convenience adapter so callers can treat the backend stream as a `Stream`.
pub fn into_stream(chunks: ChunkStream) -> impl Stream<Item = Result<StreamChunk>> + Send {
    chunks
}

#[cfg(test)]
mod tests;
