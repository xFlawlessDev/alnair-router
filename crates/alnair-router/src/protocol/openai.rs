//! Conversion between the OpenAI wire format and router-owned types.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::upstream::chat_backend::{
    self, GenerationOptions, MessagePart, RouterMessage, ToolCallSpec,
};

/// OpenAI `POST /v1/chat/completions` request body.
#[derive(Debug, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<OpenAiMessage>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub top_p: Option<f64>,
    #[serde(default)]
    pub max_tokens: Option<i32>,
    #[serde(default)]
    pub max_completion_tokens: Option<i32>,
    #[serde(default)]
    pub stop: Option<StopSequence>,
    #[serde(default)]
    pub seed: Option<i64>,
    #[serde(default)]
    pub presence_penalty: Option<f64>,
    #[serde(default)]
    pub frequency_penalty: Option<f64>,
    #[serde(default)]
    pub tools: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    pub stream_options: Option<StreamOptions>,
    /// OpenAI reasoning effort, e.g. `low`, `medium`, `high`.
    #[serde(default)]
    pub reasoning_effort: Option<String>,
}

/// `stream_options` on a streaming request.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct StreamOptions {
    /// Ask for a final SSE chunk carrying token usage, as OpenAI does.
    #[serde(default)]
    pub include_usage: bool,
}

/// `stop` accepts either a string or an array of strings.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum StopSequence {
    One(String),
    Many(Vec<String>),
}

impl StopSequence {
    fn into_vec(self) -> Vec<String> {
        match self {
            StopSequence::One(value) => vec![value],
            StopSequence::Many(values) => values,
        }
    }
}

/// An OpenAI message. `content` may be a string or a multimodal part array.
#[derive(Debug, Deserialize)]
pub struct OpenAiMessage {
    pub role: String,
    #[serde(default)]
    pub content: Option<OpenAiContent>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<OpenAiToolCall>>,
    /// Assistant reasoning text, as DeepSeek/OpenRouter expose it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    /// Anthropic signature for `reasoning_content`, replayed during tool use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_signature: Option<String>,
    /// Anthropic `redacted_thinking` payload, replayed verbatim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redacted_thinking: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum OpenAiContent {
    Text(String),
    Parts(Vec<OpenAiContentPart>),
    Null,
}

#[derive(Debug, Deserialize)]
pub struct OpenAiContentPart {
    #[serde(rename = "type")]
    pub part_type: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub image_url: Option<OpenAiImageUrl>,
}

#[derive(Debug, Deserialize)]
pub struct OpenAiImageUrl {
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct OpenAiToolCall {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub function: Option<OpenAiFunctionCall>,
}

#[derive(Debug, Deserialize)]
pub struct OpenAiFunctionCall {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arguments: Option<String>,
}

impl ChatCompletionRequest {
    /// Converts generation parameters into router-owned options.
    pub fn generation_options(&self) -> GenerationOptions {
        GenerationOptions {
            temperature: self.temperature,
            top_p: self.top_p,
            max_tokens: self.max_completion_tokens.or(self.max_tokens),
            stop: self.stop.clone().map(StopSequence::into_vec),
            seed: self.seed,
            presence_penalty: self.presence_penalty,
            frequency_penalty: self.frequency_penalty,
            thinking_level: self.reasoning_effort.clone(),
        }
    }

    /// Converts the OpenAI message list into provider-layer messages.
    pub fn into_messages(self) -> Result<Vec<RouterMessage>> {
        if self.messages.is_empty() {
            return Err(Error::BadRequest("messages must not be empty".to_string()));
        }
        self.messages
            .into_iter()
            .map(OpenAiMessage::into_router_message)
            .collect()
    }
}

impl OpenAiMessage {
    /// Converts one OpenAI message into a provider-layer message.
    pub fn into_router_message(self) -> Result<RouterMessage> {
        let role = self.role.trim().to_ascii_lowercase();
        if role.is_empty() {
            return Err(Error::BadRequest("message role is required".to_string()));
        }

        let reasoning_content = self.reasoning_content;
        let thinking_signature = self.thinking_signature;
        let redacted_thinking = self.redacted_thinking;

        // Assistant messages carrying tool calls are rebuilt as such, so the
        // provider layer can pair them with the following tool results.
        if let Some(calls) = self.tool_calls.filter(|calls| !calls.is_empty()) {
            let specs = calls
                .into_iter()
                .filter_map(|call| {
                    let function = call.function?;
                    Some(ToolCallSpec {
                        id: call.id.unwrap_or_else(|| "call_0".to_string()),
                        name: function.name.unwrap_or_default(),
                        arguments: function.arguments.unwrap_or_else(|| "{}".to_string()),
                    })
                })
                .collect();

            let mut message = chat_backend::message_assistant_tool_calls(
                self.content.map(content_to_text).unwrap_or_default(),
                specs,
            );
            message.thinking = reasoning_content;
            message.thinking_signature = thinking_signature;
            message.redacted_thinking = redacted_thinking;
            return Ok(message);
        }

        if role == "tool" {
            let tool_call_id = self.tool_call_id.ok_or_else(|| {
                Error::BadRequest("tool message requires tool_call_id".to_string())
            })?;
            return Ok(chat_backend::message_tool_result(
                self.content.map(content_to_text).unwrap_or_default(),
                tool_call_id,
            ));
        }

        let mut message = match self.content {
            Some(OpenAiContent::Parts(parts)) => {
                let parts: Vec<MessagePart> = parts
                    .into_iter()
                    .filter_map(|part| match part.part_type.as_str() {
                        "text" => part.text.map(MessagePart::Text),
                        "image_url" => part.image_url.map(|image| MessagePart::ImageUrl(image.url)),
                        _ => None,
                    })
                    .collect();

                if parts.is_empty() {
                    chat_backend::message_text(&role, String::new())
                } else {
                    chat_backend::message_parts(&role, parts)
                }
            }
            other => {
                chat_backend::message_text(&role, other.map(content_to_text).unwrap_or_default())
            }
        };
        if role == "assistant" {
            message.thinking = reasoning_content;
            message.thinking_signature = thinking_signature;
            message.redacted_thinking = redacted_thinking;
        }
        Ok(message)
    }
}

fn content_to_text(content: OpenAiContent) -> String {
    match content {
        OpenAiContent::Text(text) => text,
        OpenAiContent::Null => String::new(),
        OpenAiContent::Parts(parts) => parts
            .into_iter()
            .filter(|part| part.part_type == "text")
            .filter_map(|part| part.text)
            .collect::<Vec<_>>()
            .join(" "),
    }
}

/// Non-streaming JSON response in OpenAI shape.
#[derive(Debug, Serialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: &'static str,
    pub created: i64,
    pub model: String,
    pub choices: Vec<ChatChoice>,
    pub usage: UsagePayload,
}

#[derive(Debug, Serialize)]
pub struct ChatChoice {
    pub index: u32,
    pub message: ChatChoiceMessage,
    pub finish_reason: String,
}

#[derive(Debug, Serialize)]
pub struct ChatChoiceMessage {
    pub role: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OpenAiToolCallOut>>,
}

/// Tool call in OpenAI `message.tool_calls` shape.
#[derive(Debug, Serialize)]
pub struct OpenAiToolCallOut {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: &'static str,
    pub function: OpenAiFunctionOut,
}

#[derive(Debug, Serialize)]
pub struct OpenAiFunctionOut {
    pub name: String,
    pub arguments: String,
}

/// Builds the `tool_calls` field, omitting it entirely when there are none.
pub fn tool_calls_payload(specs: &[ToolCallSpec]) -> Option<Vec<OpenAiToolCallOut>> {
    if specs.is_empty() {
        return None;
    }
    Some(
        specs
            .iter()
            .map(|spec| OpenAiToolCallOut {
                id: spec.id.clone(),
                call_type: "function",
                function: OpenAiFunctionOut {
                    name: spec.name.clone(),
                    arguments: spec.arguments.clone(),
                },
            })
            .collect(),
    )
}

#[derive(Debug, Serialize)]
pub struct UsagePayload {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_tokens_details: Option<PromptTokensDetails>,
}

#[derive(Debug, Serialize)]
pub struct PromptTokensDetails {
    pub cached_tokens: u64,
}

/// SSE `chat.completion.chunk` payload.
#[derive(Debug, Serialize)]
pub struct ChatCompletionChunk {
    pub id: String,
    pub object: &'static str,
    pub created: i64,
    pub model: String,
    pub choices: Vec<ChunkChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsagePayload>,
}

#[derive(Debug, Serialize)]
pub struct ChunkChoice {
    pub index: u32,
    pub delta: ChunkDelta,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Serialize, Default)]
pub struct ChunkDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Reasoning stream, as DeepSeek and OpenRouter expose it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallDelta>>,
}

/// A streamed tool call. Fragments are identified by `index`; the first one for
/// an index carries `id` and `name`, later ones only extend `arguments`.
#[derive(Debug, Serialize)]
pub struct ToolCallDelta {
    pub index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub call_type: Option<&'static str>,
    pub function: FunctionDelta,
}

#[derive(Debug, Serialize, Default)]
pub struct FunctionDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}

/// Builds the terminal usage payload for a completed request.
pub fn usage_payload(usage: Option<chat_backend::TokenUsage>) -> UsagePayload {
    let usage = usage.unwrap_or_default();
    UsagePayload {
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
        total_tokens: usage.prompt_tokens + usage.completion_tokens,
        prompt_tokens_details: (usage.cached_tokens > 0).then_some(PromptTokensDetails {
            cached_tokens: usage.cached_tokens,
        }),
    }
}
