//! Anthropic Messages API translation.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::protocol::openai::{
    ChatCompletionRequest, OpenAiContent, OpenAiContentPart, OpenAiFunctionCall, OpenAiImageUrl,
    OpenAiMessage, OpenAiToolCall, StopSequence,
};
use crate::upstream::chat_backend::RouterMessage;

/// `POST /v1/messages` request body.
#[derive(Debug, Deserialize)]
pub struct MessagesRequest {
    pub model: String,
    pub messages: Vec<AnthropicMessage>,
    #[serde(default)]
    pub system: Option<SystemPrompt>,
    #[serde(default)]
    pub max_tokens: Option<i32>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub top_p: Option<f64>,
    #[serde(default)]
    pub stop_sequences: Option<Vec<String>>,
    #[serde(default)]
    pub tools: Option<Vec<serde_json::Value>>,
    /// Extended-thinking configuration; `budget_tokens` maps to an effort level.
    #[serde(default)]
    pub thinking: Option<AnthropicThinkingRequest>,
}

#[derive(Debug, Deserialize)]
pub struct AnthropicThinkingRequest {
    #[serde(rename = "type", default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub budget_tokens: Option<u64>,
}

/// `system` accepts either a plain string or content blocks.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum SystemPrompt {
    Text(String),
    Blocks(Vec<AnthropicContentBlock>),
}

#[derive(Debug, Deserialize)]
pub struct AnthropicMessage {
    pub role: String,
    pub content: AnthropicContent,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum AnthropicContent {
    Text(String),
    Blocks(Vec<AnthropicContentBlock>),
}

#[derive(Debug, Deserialize)]
pub struct AnthropicContentBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub source: Option<AnthropicImageSource>,
    /// `tool_use` id / `tool_result` target id.
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub input: Option<serde_json::Value>,
    #[serde(default)]
    pub tool_use_id: Option<String>,
    #[serde(default)]
    pub content: Option<serde_json::Value>,
    /// `thinking` text and its `signature`.
    #[serde(default)]
    pub thinking: Option<String>,
    #[serde(default)]
    pub signature: Option<String>,
    /// `redacted_thinking` payload.
    #[serde(default)]
    pub data: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AnthropicImageSource {
    #[serde(rename = "type")]
    pub source_type: String,
    #[serde(default)]
    pub media_type: Option<String>,
    #[serde(default)]
    pub data: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

impl SystemPrompt {
    fn into_text(self) -> String {
        match self {
            SystemPrompt::Text(text) => text,
            SystemPrompt::Blocks(blocks) => blocks
                .into_iter()
                .filter(|block| block.block_type == "text")
                .filter_map(|block| block.text)
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

fn image_data_url(source: AnthropicImageSource) -> Option<String> {
    if let Some(url) = source.url {
        return Some(url);
    }
    let data = source.data?;
    let media_type = source.media_type.unwrap_or_else(|| "image/png".to_string());
    Some(format!("data:{media_type};base64,{data}"))
}

impl MessagesRequest {
    /// Reuses the OpenAI request shape so both endpoints share one executor path.
    pub fn into_chat_request(self) -> Result<ChatCompletionRequest> {
        if self.messages.is_empty() {
            return Err(Error::BadRequest("messages must not be empty".to_string()));
        }

        let mut messages = Vec::with_capacity(self.messages.len() + 1);

        if let Some(system) = self.system {
            let text = system.into_text();
            if !text.trim().is_empty() {
                messages.push(OpenAiMessage {
                    role: "system".to_string(),
                    content: Some(OpenAiContent::Text(text)),
                    name: None,
                    tool_call_id: None,
                    tool_calls: None,
                    reasoning_content: None,
                    thinking_signature: None,
                    redacted_thinking: None,
                });
            }
        }

        for message in self.messages {
            messages.extend(message.into_openai()?);
        }

        Ok(ChatCompletionRequest {
            model: self.model,
            messages,
            stream: self.stream,
            temperature: self.temperature,
            top_p: self.top_p,
            max_tokens: self.max_tokens,
            max_completion_tokens: None,
            stop: self.stop_sequences.map(StopSequence::Many),
            seed: None,
            presence_penalty: None,
            frequency_penalty: None,
            tools: self.tools,
            // Anthropic clients read usage from `message_delta` instead.
            stream_options: None,
            reasoning_effort: self.thinking.and_then(|thinking| {
                // `type: "disabled"` (or any non-enabled type) keeps thinking off.
                if thinking
                    .kind
                    .as_deref()
                    .is_some_and(|kind| kind != "enabled")
                {
                    return None;
                }
                let budget = thinking.budget_tokens.unwrap_or(0);
                Some(crate::upstream::chat_backend::thinking_level_from_budget(budget).to_string())
            }),
        })
    }
}

impl AnthropicMessage {
    /// Converts one Anthropic message into one or more OpenAI-shaped messages.
    ///
    /// `tool_result` blocks become standalone `tool` messages, while `text`,
    /// `image`, `tool_use` and thinking blocks stay on a single user/assistant
    /// message so signatures and call pairings survive the round trip.
    fn into_openai(self) -> Result<Vec<OpenAiMessage>> {
        let role = match self.role.trim().to_ascii_lowercase().as_str() {
            "assistant" => "assistant",
            "user" => "user",
            other => {
                return Err(Error::BadRequest(format!(
                    "unsupported Anthropic message role: {other}"
                )));
            }
        };

        let blocks = match self.content {
            AnthropicContent::Text(text) => {
                return Ok(vec![plain_message(role, OpenAiContent::Text(text))]);
            }
            AnthropicContent::Blocks(blocks) => blocks,
        };

        let mut out = Vec::new();
        let mut parts = Vec::new();
        // Assistant-only fields.
        let mut tool_calls = Vec::new();
        let mut thinking = None;
        let mut signature = None;
        let mut redacted = None;

        for block in blocks {
            match block.block_type.as_str() {
                "text" => {
                    if let Some(text) = block.text {
                        parts.push(OpenAiContentPart {
                            part_type: "text".to_string(),
                            text: Some(text),
                            image_url: None,
                        });
                    }
                }
                "image" => {
                    if let Some(source) = block.source.and_then(image_data_url) {
                        parts.push(OpenAiContentPart {
                            part_type: "image_url".to_string(),
                            text: None,
                            image_url: Some(OpenAiImageUrl { url: source }),
                        });
                    }
                }
                "tool_use" => {
                    if let Some(id) = block.id {
                        let arguments = block
                            .input
                            .map(|input| input.to_string())
                            .unwrap_or_else(|| "{}".to_string());
                        tool_calls.push(OpenAiToolCall {
                            id: Some(id),
                            function: Some(OpenAiFunctionCall {
                                name: block.name,
                                arguments: Some(arguments),
                            }),
                        });
                    }
                }
                "tool_result" => {
                    // Each tool result is its own `tool` role message.
                    if let Some(tool_use_id) = block.tool_use_id {
                        out.push(OpenAiMessage {
                            role: "tool".to_string(),
                            content: Some(block_result_content(block.content)),
                            name: None,
                            tool_call_id: Some(tool_use_id),
                            tool_calls: None,
                            reasoning_content: None,
                            thinking_signature: None,
                            redacted_thinking: None,
                        });
                    }
                }
                "thinking" => {
                    thinking = block.thinking;
                    signature = block.signature;
                }
                "redacted_thinking" => {
                    redacted = block.data;
                }
                _ => {}
            }
        }

        let content = if parts.is_empty() {
            None
        } else {
            Some(OpenAiContent::Parts(parts))
        };

        if role == "assistant" {
            // Keep an assistant turn when it carries thinking or tool calls even
            // with no text, so the next `tool_result` still has its pair.
            if content.is_some()
                || !tool_calls.is_empty()
                || thinking.is_some()
                || redacted.is_some()
            {
                out.push(OpenAiMessage {
                    role: "assistant".to_string(),
                    content,
                    name: None,
                    tool_call_id: None,
                    tool_calls: if tool_calls.is_empty() {
                        None
                    } else {
                        Some(tool_calls)
                    },
                    reasoning_content: thinking,
                    thinking_signature: signature,
                    redacted_thinking: redacted,
                });
            }
        } else if content.is_some() {
            out.push(OpenAiMessage {
                role: "user".to_string(),
                content,
                name: None,
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
                thinking_signature: None,
                redacted_thinking: None,
            });
        }

        Ok(out)
    }
}

fn plain_message(role: &str, content: OpenAiContent) -> OpenAiMessage {
    OpenAiMessage {
        role: role.to_string(),
        content: Some(content),
        name: None,
        tool_call_id: None,
        tool_calls: None,
        reasoning_content: None,
        thinking_signature: None,
        redacted_thinking: None,
    }
}

/// Flattens a `tool_result` content value (string or text-block array).
fn block_result_content(content: Option<serde_json::Value>) -> OpenAiContent {
    match content {
        Some(serde_json::Value::String(text)) => OpenAiContent::Text(text),
        Some(serde_json::Value::Array(blocks)) => {
            let text = blocks
                .into_iter()
                .filter_map(|block| {
                    block
                        .get("text")
                        .and_then(|text| text.as_str())
                        .map(str::to_string)
                })
                .collect::<Vec<_>>()
                .join("\n");
            OpenAiContent::Text(text)
        }
        _ => OpenAiContent::Text(String::new()),
    }
}

/// Anthropic-shaped non-streaming response.
#[derive(Debug, Serialize)]
pub struct MessagesResponse {
    pub id: String,
    #[serde(rename = "type")]
    pub response_type: &'static str,
    pub role: &'static str,
    pub model: String,
    pub content: Vec<AnthropicResponseBlock>,
    pub stop_reason: String,
    pub usage: AnthropicUsage,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnthropicResponseBlock {
    Text {
        text: String,
    },
    Thinking {
        thinking: String,
        signature: String,
    },
    RedactedThinking {
        data: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

#[derive(Debug, Serialize)]
pub struct AnthropicUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Anthropic token-count response.
#[derive(Debug, Serialize)]
pub struct CountTokensResponse {
    pub input_tokens: u64,
}

/// Heuristic token estimate: roughly 4 characters per token.
pub fn estimate_tokens(text: &str) -> u64 {
    let chars = text.chars().count() as u64;
    chars.div_ceil(4).max(1)
}

/// Sums a heuristic estimate across all message content.
pub fn estimate_messages(messages: &[RouterMessage]) -> u64 {
    messages
        .iter()
        .map(|message| estimate_tokens(&message.content.as_text()))
        .sum::<u64>()
        .max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(body: serde_json::Value) -> ChatCompletionRequest {
        let request: MessagesRequest = serde_json::from_value(body).expect("valid request");
        request.into_chat_request().expect("converts")
    }

    #[test]
    fn thinking_budget_maps_to_reasoning_effort() {
        let chat = request(serde_json::json!({
            "model": "claude",
            "max_tokens": 1024,
            "thinking": { "type": "enabled", "budget_tokens": 8192 },
            "messages": [{ "role": "user", "content": "hi" }]
        }));

        assert_eq!(
            chat.generation_options().thinking_level.as_deref(),
            Some("medium")
        );
    }

    #[test]
    fn disabled_thinking_produces_no_reasoning_effort() {
        let chat = request(serde_json::json!({
            "model": "claude",
            "max_tokens": 1024,
            "thinking": { "type": "disabled" },
            "messages": [{ "role": "user", "content": "hi" }]
        }));

        assert!(chat.generation_options().thinking_level.is_none());
    }

    #[test]
    fn signed_thinking_and_tool_blocks_round_trip() {
        let chat = request(serde_json::json!({
            "model": "claude",
            "max_tokens": 1024,
            "messages": [
                { "role": "user", "content": "hi" },
                { "role": "assistant", "content": [
                    { "type": "redacted_thinking", "data": "opaque" },
                    { "type": "thinking", "thinking": "reasoning", "signature": "sig-1" },
                    { "type": "tool_use", "id": "tc1", "name": "read", "input": { "path": "/tmp" } }
                ]},
                { "role": "user", "content": [
                    { "type": "tool_result", "tool_use_id": "tc1", "content": "file" }
                ]}
            ]
        }));

        let messages = chat.into_messages().expect("messages");

        let assistant = messages
            .iter()
            .find(|message| message.role == "assistant")
            .expect("assistant turn");
        assert_eq!(assistant.thinking.as_deref(), Some("reasoning"));
        assert_eq!(assistant.thinking_signature.as_deref(), Some("sig-1"));
        assert_eq!(assistant.redacted_thinking.as_deref(), Some("opaque"));
        assert_eq!(
            assistant
                .tool_calls
                .as_ref()
                .and_then(|calls| calls.first())
                .map(|call| call.id.as_str()),
            Some("tc1")
        );

        let tool = messages
            .iter()
            .find(|message| message.role == "tool")
            .expect("tool result turn");
        assert_eq!(tool.tool_call_id.as_deref(), Some("tc1"));
        assert_eq!(tool.content.as_text(), "file");
    }

    #[test]
    fn tool_result_without_text_preserves_the_pairing() {
        let chat = request(serde_json::json!({
            "model": "claude",
            "max_tokens": 1024,
            "messages": [
                { "role": "assistant", "content": [
                    { "type": "tool_use", "id": "tc1", "name": "read", "input": {} }
                ]},
                { "role": "user", "content": [
                    { "type": "tool_result", "tool_use_id": "tc1" }
                ]}
            ]
        }));

        let messages = chat.into_messages().expect("messages");

        // The empty assistant turn must survive so the tool_result still pairs.
        assert!(messages.iter().any(|message| message.role == "assistant"));
        assert!(messages.iter().any(
            |message| message.role == "tool" && message.tool_call_id.as_deref() == Some("tc1")
        ));
    }
}
