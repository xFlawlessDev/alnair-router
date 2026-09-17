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
    ContentPart, ImageUrlContentPart, LlmStreamChunk, LlmStreamOptions, Message, MessageContent,
    MessageToolCall, ModelConfig, ModelCostRates, ProviderType, TextContentPart,
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
        })
        .collect();

    RouterMessage {
        role: role.into(),
        content: MessageContent::Parts(parts),
        thinking: None,
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
    /// Total cost in USD: input + output + reasoning.
    pub cost_usd: f64,
}

/// One streamed chunk from an upstream provider.
#[derive(Debug, Clone)]
pub enum StreamChunk {
    Text(String),
    Thinking(String),
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
        other => Err(Error::UnsupportedProviderType(other.to_string())),
    }
}

/// Builds a provider registry with the supported implementations registered.
pub fn default_registry() -> ProviderRegistry {
    ProviderRegistry::with_defaults()
}

/// A boxed stream of chunks from the provider layer.
pub type ChunkStream = BoxStream<'static, Result<StreamChunk>>;

/// Builds the model config handed to the provider layer.
fn model_config(
    provider_type: ProviderType,
    base_url: &str,
    model: &str,
    api_key: Option<&str>,
    custom_headers: BTreeMap<String, String>,
    price: Option<Price>,
) -> ModelConfig {
    ModelConfig {
        provider_type,
        base_url: base_url.to_string(),
        model_id: model.to_string(),
        api_key: api_key.map(str::to_string),
        custom_headers,
        supports_vision: false,
        // Capability gates are opt-in: the router does not assume a model
        // supports thinking or prompt caching unless configured.
        supports_thinking: false,
        supports_cache_control: false,
        context_window: 0,
        cost_rates: price.map(cost_rates),
    }
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
) -> Result<ChunkStream> {
    let provider_type = provider_type_from_str(provider_type)?;

    if registry.get(&provider_type).is_none() {
        return Err(Error::UnsupportedProviderType(format!(
            "no provider registered for {provider_type:?}"
        )));
    }

    let config = model_config(
        provider_type.clone(),
        base_url,
        model,
        api_key,
        custom_headers,
        price,
    );
    let stream_options = stream_options(options, retry);

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
fn stream_options(options: Option<&GenerationOptions>, retry: RetryPolicy) -> LlmStreamOptions {
    match options {
        Some(options) => options.to_stream_options(retry),
        None => GenerationOptions::default().to_stream_options(retry),
    }
}

/// Converts a vendored chunk into a router-owned chunk.
fn to_stream_chunk(
    chunk: std::result::Result<LlmStreamChunk, alnair_llm::ChatError>,
) -> Result<StreamChunk> {
    match chunk {
        Ok(LlmStreamChunk::Text(text)) => Ok(StreamChunk::Text(text)),
        Ok(LlmStreamChunk::Thinking(text)) => Ok(StreamChunk::Thinking(text)),
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
            ..
        }) => Ok(StreamChunk::Usage(TokenUsage {
            prompt_tokens: prompt_eval_count.unwrap_or(0),
            completion_tokens: eval_count.unwrap_or(0),
            cached_tokens: cached_prompt_eval_count.unwrap_or(0),
            reasoning_tokens: reasoning_eval_count.unwrap_or(0),
            cost_input_usd: cost_input_usd.unwrap_or(0.0),
            cost_output_usd: cost_output_usd.unwrap_or(0.0),
            cost_reasoning_usd: cost_reasoning_usd.unwrap_or(0.0),
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
            StreamChunk::Thinking(_) => {}
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
mod tests {
    use super::*;

    #[test]
    fn retry_policy_maps_into_provider_options() {
        let policy = RetryPolicy {
            max_retries_per_tier: 7,
            max_retry_delay_ms: 1_234,
        };

        let options = GenerationOptions::default().to_stream_options(policy);

        assert_eq!(options.max_retries, 7);
        assert_eq!(options.max_retry_delay_ms, 1_234);
    }

    #[test]
    fn retry_policy_default_is_conservative() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.max_retries_per_tier, 2);
        assert_eq!(policy.max_retry_delay_ms, 30_000);
    }

    #[test]
    fn retry_policy_applies_even_without_generation_options() {
        let policy = RetryPolicy {
            max_retries_per_tier: 3,
            max_retry_delay_ms: 456,
        };

        let options = stream_options(None, policy);

        assert_eq!(options.max_retries, 3);
        assert_eq!(options.max_retry_delay_ms, 456);
    }

    #[tokio::test]
    async fn collect_aggregates_tool_calls_instead_of_failing() {
        let chunks: ChunkStream = futures::stream::iter(vec![
            Ok(StreamChunk::Text("calling".to_string())),
            Ok(StreamChunk::ToolCall {
                id: "tc1".to_string(),
                name: "read_file".to_string(),
                arguments: r#"{"path":"/tmp"}"#.to_string(),
            }),
            Ok(StreamChunk::Usage(TokenUsage {
                prompt_tokens: 5,
                completion_tokens: 1,
                ..Default::default()
            })),
            Ok(StreamChunk::Done(None)),
        ])
        .boxed();

        let completion = collect(chunks).await.expect("collect");

        assert_eq!(completion.content, "calling");
        assert_eq!(completion.tool_calls.len(), 1);
        assert_eq!(completion.tool_calls[0].name, "read_file");
        assert_eq!(completion.finish_reason.as_deref(), Some("tool_calls"));
        assert_eq!(completion.usage.expect("usage").prompt_tokens, 5);
    }

    #[tokio::test]
    async fn collect_marks_plain_completions_as_stopped() {
        let chunks: ChunkStream = futures::stream::iter(vec![
            Ok(StreamChunk::Text("hello".to_string())),
            Ok(StreamChunk::Done(None)),
        ])
        .boxed();

        let completion = collect(chunks).await.expect("collect");

        assert_eq!(completion.finish_reason.as_deref(), Some("stop"));
        assert!(completion.tool_calls.is_empty());
    }
}
