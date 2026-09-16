//! Anthropic Messages API translation.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::protocol::openai::{
    ChatCompletionRequest, OpenAiContent, OpenAiContentPart, OpenAiImageUrl, OpenAiMessage,
    StopSequence,
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
                });
            }
        }

        for message in self.messages {
            messages.push(message.into_openai()?);
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
        })
    }
}

impl AnthropicMessage {
    fn into_openai(self) -> Result<OpenAiMessage> {
        let role = match self.role.trim().to_ascii_lowercase().as_str() {
            "assistant" => "assistant",
            "user" => "user",
            other => {
                return Err(Error::BadRequest(format!(
                    "unsupported Anthropic message role: {other}"
                )));
            }
        };

        let content = match self.content {
            AnthropicContent::Text(text) => OpenAiContent::Text(text),
            AnthropicContent::Blocks(blocks) => {
                let mut parts = Vec::new();
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
                        _ => {}
                    }
                }
                OpenAiContent::Parts(parts)
            }
        };

        Ok(OpenAiMessage {
            role: role.to_string(),
            content: Some(content),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        })
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
