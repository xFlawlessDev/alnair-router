//! Chat types shared across handlers and streaming implementations.

use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};

use crate::model_config::{ModelConfig, ThinkingLevel};

// =============================================================================
// Generation options
// =============================================================================

/// Generation options for LLM inference
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct GenerationOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repeat_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_predict: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mirostat: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mirostat_tau: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mirostat_eta: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_ctx: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tfs_z: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<ThinkingLevel>,
}

impl GenerationOptions {
    pub fn apply_to_openai_body(&self, body: &mut serde_json::Value) {
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
        } else if let Some(v) = self.num_predict
            && v > 0
        {
            body["max_tokens"] = serde_json::json!(v);
        }
        set!(seed);
        if let Some(ref v) = self.stop {
            body["stop"] = serde_json::json!(v);
        }
    }
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

/// Content part - text or image
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ContentPart {
    Text(TextContentPart),
    ImageUrl(ImageUrlContentPart),
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
                    ContentPart::ImageUrl(_) => None,
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
                let json_parts: Vec<serde_json::Value> = parts
                    .iter()
                    .map(|p| match p {
                        ContentPart::Text(t) => serde_json::json!({
                            "type": "text",
                            "text": t.text
                        }),
                        ContentPart::ImageUrl(img) => serde_json::json!({
                            "type": "image_url",
                            "image_url": { "url": img.image_url.url }
                        }),
                    })
                    .collect();
                serde_json::json!(json_parts)
            }
        }
    }

    pub fn to_parts(&self) -> (String, Vec<String>) {
        match self {
            MessageContent::Text(s) => (s.clone(), Vec::new()),
            MessageContent::Parts(parts) => {
                let mut text_parts = Vec::new();
                let mut images = Vec::new();
                for p in parts {
                    match p {
                        ContentPart::Text(t) => text_parts.push(t.text.clone()),
                        ContentPart::ImageUrl(img) => {
                            let base64 = strip_data_url_prefix(&img.image_url.url);
                            images.push(base64);
                        }
                    }
                }
                (text_parts.join(" "), images)
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
            tool_call_id: None,
            tool_calls: None,
            cache_control: false,
        }
    }

    pub fn to_provider_json(&self) -> serde_json::Value {
        if self.role == "tool" {
            return serde_json::json!({
                "role": "tool",
                "content": self.content.as_text(),
                "tool_call_id": self.tool_call_id.clone().unwrap_or_else(|| "unknown".to_string())
            });
        }

        if self.role == "assistant"
            && let Some(tool_calls) = &self.tool_calls
        {
            let tool_calls: Vec<serde_json::Value> = tool_calls
                .iter()
                .map(|tc| {
                    serde_json::json!({
                        "id": tc.id,
                        "type": "function",
                        "function": {
                            "name": tc.name,
                            "arguments": parse_tool_arguments(&tc.arguments)
                        }
                    })
                })
                .collect();
            return serde_json::json!({
                "role": "assistant",
                "content": self.content.as_text(),
                "tool_calls": tool_calls
            });
        }

        let (text, images) = self.content.to_parts();
        let mut msg = serde_json::json!({
            "role": self.role,
            "content": text
        });
        if !images.is_empty() {
            msg["images"] = serde_json::json!(images);
        }
        msg
    }
}

fn parse_tool_arguments(arguments: &str) -> serde_json::Value {
    serde_json::from_str(arguments).unwrap_or_else(|_| serde_json::json!({}))
}

/// Convert messages to OpenAI API format
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
/// OpenAI-compatible, Anthropic-native and Command Code upstreams are
/// supported.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderType {
    #[default]
    OpenaiCompatible,
    AnthropicNative,
    /// Command Code: the Provider API, falling back to the CLI transport.
    CommandCode,
}

impl ProviderType {
    /// Stable key used for registry lookup and diagnostics.
    pub fn as_key(&self) -> &'static str {
        match self {
            ProviderType::OpenaiCompatible => "openai-compatible",
            ProviderType::AnthropicNative => "anthropic-native",
            ProviderType::CommandCode => "command-code",
        }
    }
}

// =============================================================================
// Request / Response
// =============================================================================

/// Chat request payload
#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub messages: Vec<Message>,
    pub model: String,
    #[serde(default)]
    pub model_config: Option<ModelConfig>,
    pub provider_url: String,
    #[serde(default)]
    pub provider_type: ProviderType,
    pub api_key: Option<String>,
    pub system_prompt: Option<String>,

    // RAG
    pub workspace_id: Option<String>,
    pub chat_id: Option<String>,
    pub doc_filter: Option<Vec<String>>,
    pub kb_ids: Option<Vec<String>>,
    #[serde(default)]
    pub use_rag: bool,

    // Streaming
    pub stream: Option<bool>,
    #[serde(default = "default_true")]
    pub use_streaming: bool,

    // Generation
    #[serde(default)]
    pub options: Option<GenerationOptions>,
}

fn default_true() -> bool {
    true
}

/// Non-streaming chat response
#[derive(Debug, Serialize)]
pub struct ChatResponse {
    pub id: String,
    pub content: String,
    pub model: String,
    pub finish_reason: Option<String>,
    pub citations: Option<Vec<Citation>>,
}

/// Citation from RAG
#[derive(Debug, Clone, Serialize)]
pub struct Citation {
    pub source_number: i32,
    pub doc_id: String,
    pub filename: String,
    pub text: String,
    pub score: f32,
}

// =============================================================================
// SSE Stream Events
// =============================================================================

/// SSE stream events for Vercel AI SDK compatibility
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum StreamEvent {
    #[serde(rename = "text")]
    Text { text: String },

    #[serde(rename = "thinking")]
    Thinking { thinking: String },

    #[serde(rename = "tool_call")]
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },

    #[serde(rename = "tool_result")]
    ToolResult {
        id: String,
        result: String,
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        cached: bool,
    },

    #[serde(rename = "source")]
    Source {
        source_number: i32,
        doc_id: String,
        filename: String,
        text: String,
        score: f32,
    },

    #[serde(rename = "search_meta")]
    SearchMeta {
        intent: Option<String>,
        retrieval_mode: String,
        source_count: usize,
    },

    #[serde(rename = "context_status")]
    ContextStatus {
        estimated_input_tokens: u32,
        context_window: u32,
        usable_input_tokens: u32,
        compact_at_tokens: u32,
        hard_limit_tokens: u32,
        compacted: bool,
        summary_turns: u32,
        source: String,
    },

    #[serde(rename = "finish")]
    Finish { finish_reason: String },

    #[serde(rename = "usage")]
    Usage {
        total_duration: Option<u64>,
        prompt_eval_count: Option<u64>,
        cached_prompt_eval_count: Option<u64>,
        eval_count: Option<u64>,
        tokens_per_second: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cost_input_usd: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cost_output_usd: Option<f64>,
    },

    #[serde(rename = "error")]
    Error { message: String },
}

impl StreamEvent {
    pub fn to_sse_data(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// LLM stream chunk (internal)
#[derive(Debug, Clone)]
pub enum LlmStreamChunk {
    Text(String),
    Thinking(String),
    ToolCall {
        id: String,
        name: String,
        arguments: serde_json::Value,
    },
    Usage {
        total_duration: Option<u64>,
        prompt_eval_count: Option<u64>,
        cached_prompt_eval_count: Option<u64>,
        eval_count: Option<u64>,
        /// Reasoning tokens included in `eval_count`, when reported.
        reasoning_eval_count: Option<u64>,
        /// USD cost for input tokens. None for local/unmetered (Ollama) or unknown model.
        cost_input_usd: Option<f64>,
        /// USD cost for output tokens. None for local/unmetered or unknown model.
        cost_output_usd: Option<f64>,
        /// Reasoning premium over the output rate, reported separately.
        cost_reasoning_usd: Option<f64>,
    },
    Done(Option<String>),
}

// =============================================================================
// Error
// =============================================================================

/// Chat error type
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

impl IntoResponse for ChatError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            ChatError::BadRequest(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            ChatError::Provider(_) => (StatusCode::BAD_GATEWAY, self.to_string()),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };
        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}

fn strip_data_url_prefix(data_url: &str) -> String {
    if let Some(idx) = data_url.find(";base64,") {
        data_url[idx + ";base64,".len()..].to_string()
    } else {
        data_url.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn openai_options_skip_non_positive_max_tokens() {
        let mut body = json!({});
        GenerationOptions {
            max_tokens: Some(-12),
            num_predict: Some(128),
            ..Default::default()
        }
        .apply_to_openai_body(&mut body);

        assert_eq!(body["max_tokens"], json!(128));
    }

    #[test]
    fn openai_options_skip_non_positive_generation_limit() {
        let mut body = json!({});
        GenerationOptions {
            max_tokens: Some(0),
            num_predict: Some(0),
            ..Default::default()
        }
        .apply_to_openai_body(&mut body);

        assert!(body.get("max_tokens").is_none());
    }
}
