//! Chat types shared across handlers and streaming implementations.

use serde::{Deserialize, Serialize};

use crate::model_config::{Citation, ThinkingLevel};

// =============================================================================
// Generation options
// =============================================================================

/// Generation options for LLM inference.
///
/// Every field here is mapped onto [`crate::model_config::LlmStreamOptions`]
/// and reaches a provider wire body. Options only one provider family reads
/// (`top_k` for Anthropic) stay because they are actually consumed; sampler
/// knobs from the retired Ollama path are gone.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct GenerationOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,
    /// Legacy alias for [`Self::max_tokens`]; kept because the playground and
    /// stored request payloads still carry it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_predict: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<ThinkingLevel>,
}

// =============================================================================
// Messages
// =============================================================================

/// Tool call information for assistant messages
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MessageToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// Text content part for multimodal messages
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TextContentPart {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: String,
}

/// Image URL content part
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ImageUrlContentPart {
    #[serde(rename = "type")]
    pub content_type: String,
    pub image_url: ImageUrl,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ImageUrl {
    pub url: String,
}

/// A content part the provider stack carries without interpreting it.
///
/// OpenAI-compatible upstreams disagree on how they spell audio, video and
/// file input (`input_audio` vs `audio_url`, `file` vs `input_file`), so the
/// router validates the part and then replays it verbatim instead of
/// translating it. The whole wire object is kept so the upstream sees exactly
/// what the client sent.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(transparent)]
pub struct MediaContentPart {
    pub raw: serde_json::Value,
}

impl MediaContentPart {
    /// The part's `type` discriminator, e.g. `input_audio` or `video_url`.
    pub fn kind(&self) -> &str {
        self.raw
            .get("type")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
    }
}

/// Content part - text, image, or an opaque media part
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ContentPart {
    Text(TextContentPart),
    ImageUrl(ImageUrlContentPart),
    Media(MediaContentPart),
}

impl ContentPart {
    /// Renders the part as its wire object.
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            ContentPart::Text(t) => serde_json::json!({
                "type": "text",
                "text": t.text
            }),
            ContentPart::ImageUrl(img) => serde_json::json!({
                "type": "image_url",
                "image_url": { "url": img.image_url.url }
            }),
            ContentPart::Media(media) => media.raw.clone(),
        }
    }
}

/// Message content - string or multimodal array
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

impl Default for MessageContent {
    fn default() -> Self {
        MessageContent::Text(String::new())
    }
}

impl MessageContent {
    pub fn as_text(&self) -> String {
        match self {
            MessageContent::Text(s) => s.clone(),
            MessageContent::Parts(parts) => parts
                .iter()
                .filter_map(|p| match p {
                    ContentPart::Text(t) => Some(t.text.clone()),
                    ContentPart::ImageUrl(_) | ContentPart::Media(_) => None,
                })
                .collect::<Vec<_>>()
                .join(" "),
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            MessageContent::Text(s) => s.trim().is_empty(),
            MessageContent::Parts(parts) => parts.is_empty(),
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        match self {
            MessageContent::Text(s) => serde_json::json!(s),
            MessageContent::Parts(parts) => {
                let json_parts: Vec<serde_json::Value> =
                    parts.iter().map(ContentPart::to_json).collect();
                serde_json::json!(json_parts)
            }
        }
    }
}

/// Chat message (OpenAI-compatible)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Message {
    pub role: String,
    #[serde(default)]
    pub content: MessageContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    /// Anthropic's cryptographic signature for `thinking`, required when
    /// replaying a thinking block back during tool use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_signature: Option<String>,
    /// Base64 payload of an Anthropic `redacted_thinking` block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redacted_thinking: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<MessageToolCall>>,
    #[serde(default)]
    pub cache_control: bool,
}

impl Message {
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: MessageContent::Text(content.into()),
            thinking: None,
            thinking_signature: None,
            redacted_thinking: None,
            tool_call_id: None,
            tool_calls: None,
            cache_control: false,
        }
    }
}

/// Convert messages to OpenAI API format.
///
/// This is the OpenAI chat-completions wire shape used by the provider stack;
/// it lives here because [`Message`] is the shared conversation type.
pub fn convert_messages_to_openai_format(messages: &[Message]) -> Vec<serde_json::Value> {
    messages
        .iter()
        .filter(|m| {
            !m.content.is_empty()
                || m.role == "tool"
                || (m.role == "assistant" && m.tool_calls.is_some())
        })
        .map(|m| {
            if m.role == "tool" {
                serde_json::json!({
                    "role": "tool",
                    "content": m.content.as_text(),
                    "tool_call_id": m.tool_call_id.clone().unwrap_or_else(|| "unknown".to_string())
                })
            } else if m.role == "assistant"
                && let Some(tool_calls) = &m.tool_calls
            {
                let tool_calls: Vec<serde_json::Value> = tool_calls
                    .iter()
                    .map(|tc| {
                        serde_json::json!({
                            "id": tc.id,
                            "type": "function",
                            "function": {
                                "name": tc.name,
                                "arguments": tc.arguments
                            }
                        })
                    })
                    .collect();
                let content = if m.content.is_empty() {
                    serde_json::Value::Null
                } else {
                    m.content.to_json()
                };
                serde_json::json!({
                    "role": "assistant",
                    "content": content,
                    "tool_calls": tool_calls
                })
            } else {
                serde_json::json!({
                    "role": m.role,
                    "content": m.content.to_json()
                })
            }
        })
        .collect()
}

// =============================================================================
// Provider types
// =============================================================================

/// Provider type for routing requests.
///
/// OpenAI-compatible, Anthropic-native, Command Code, CodeBuddy Intl and
/// ChatGPT/Codex upstreams are supported.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderType {
    #[default]
    OpenaiCompatible,
    AnthropicNative,
    /// Command Code: the Provider API, falling back to the CLI transport.
    CommandCode,
    /// CodeBuddy International's stream-only OpenAI-compatible API.
    #[serde(rename = "codebuddy-intl")]
    CodeBuddyIntl,
    /// ChatGPT/Codex: the Codex backend's Responses dialect, authorized by a
    /// ChatGPT subscription OAuth bearer rather than an API key.
    #[serde(rename = "openai-codex")]
    OpenAiCodex,
}

impl ProviderType {
    /// Every supported provider, in the order the router's write-time
    /// validation and error messages list them.
    pub const ALL: [ProviderType; 5] = [
        ProviderType::OpenaiCompatible,
        ProviderType::AnthropicNative,
        ProviderType::CommandCode,
        ProviderType::CodeBuddyIntl,
        ProviderType::OpenAiCodex,
    ];

    /// Stable key used for registry lookup and diagnostics.
    ///
    /// This is the single source of truth for the wire names: the registry, the
    /// router's write-time validation, and the messages that list supported
    /// types all read it from here.
    pub fn as_key(&self) -> &'static str {
        match self {
            ProviderType::OpenaiCompatible => "openai-compatible",
            ProviderType::AnthropicNative => "anthropic-native",
            ProviderType::CommandCode => "command-code",
            ProviderType::CodeBuddyIntl => "codebuddy-intl",
            ProviderType::OpenAiCodex => "openai-codex",
        }
    }
}

// =============================================================================
// Stream chunks
// =============================================================================

/// One streamed chunk from a provider.
#[derive(Debug, Clone)]
pub enum LlmStreamChunk {
    Text(String),
    Thinking(String),
    /// Anthropic signature for the thinking block that just streamed; replay it
    /// verbatim alongside the thinking block on the next turn.
    ThinkingSignature(String),
    /// Anthropic `redacted_thinking` block payload, replayed verbatim.
    RedactedThinking(String),
    ToolCall {
        id: String,
        name: String,
        arguments: serde_json::Value,
    },
    /// A citation the model attached to a span of the text it has emitted.
    ///
    /// Output-side only: an upstream that produces none sends none, and that is
    /// a normal response rather than an error.
    Citation(Citation),
    Usage {
        total_duration: Option<u64>,
        prompt_eval_count: Option<u64>,
        cached_prompt_eval_count: Option<u64>,
        eval_count: Option<u64>,
        /// Reasoning tokens included in `eval_count`, when reported.
        reasoning_eval_count: Option<u64>,
        /// USD cost for input tokens. `None` when the router supplied no rates
        /// for this model, so the turn is unpriced.
        cost_input_usd: Option<f64>,
        /// USD cost for output tokens. `None` when the model is unpriced.
        cost_output_usd: Option<f64>,
        /// Reasoning premium over the output rate, reported separately.
        cost_reasoning_usd: Option<f64>,
        /// The part of `cost_input_usd` billed at the cache rate. A nearly
        /// cache-only prompt is almost entirely this figure, so it has to be
        /// visible for the dashboard's cost breakdown to explain the total.
        cost_cache_usd: Option<f64>,
    },
    Done(Option<String>),
}

// =============================================================================
// Error
// =============================================================================

/// Chat error type.
///
/// The provider layer reports what went wrong; mapping it onto an HTTP status
/// belongs to the server, so there is deliberately no `IntoResponse` impl here.
#[derive(Debug, thiserror::Error)]
pub enum ChatError {
    #[error("provider error: {0}")]
    Provider(String),
    #[error("RAG error: {0}")]
    Rag(String),
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("internal error: {0}")]
    Internal(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_config::LlmStreamOptions;

    #[test]
    fn num_predict_is_only_used_when_max_tokens_is_absent() {
        let with_max = GenerationOptions {
            max_tokens: Some(256),
            num_predict: Some(128),
            ..Default::default()
        };
        assert_eq!(LlmStreamOptions::from(&with_max).max_tokens, Some(256));

        let legacy_only = GenerationOptions {
            num_predict: Some(128),
            ..Default::default()
        };
        assert_eq!(
            LlmStreamOptions::from(&legacy_only).max_tokens,
            Some(128),
            "the legacy alias fills the gap rather than overriding max_tokens"
        );
    }

    #[test]
    fn a_non_positive_limit_never_reaches_the_wire_body() {
        let options = LlmStreamOptions::from(&GenerationOptions {
            max_tokens: Some(0),
            ..Default::default()
        });
        let mut body = serde_json::json!({});

        options.apply_to_openai_compatible_body(&mut body, false, false);

        assert!(
            body.get("max_tokens").is_none(),
            "a zero limit must be omitted, not sent as 0"
        );
    }
}
