//! Anthropic `POST /v1/messages` request body and its mapping onto the hub.

use serde::Deserialize;

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
    /// `{"type":"auto"|"any"|"tool"|"none","name":…}`. Mapped onto the hub's
    /// [`ToolChoice`](crate::upstream::chat_backend::ToolChoice).
    #[serde(default)]
    pub tool_choice: Option<serde_json::Value>,
    /// Include-at-most-N sampling; Anthropic's own knob.
    #[serde(default)]
    pub top_k: Option<i32>,
    /// `{"user_id": "…"}`. Becomes the hub's `metadata` and `user`.
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
    /// Anthropic output effort and optional structured-output format.
    #[serde(default)]
    pub output_config: Option<serde_json::Value>,
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

        let (metadata, user) = split_anthropic_metadata(self.metadata.as_ref());

        Ok(ChatCompletionRequest {
            model: self.model,
            messages,
            stream: self.stream,
            temperature: self.temperature,
            top_p: self.top_p,
            top_k: self.top_k,
            max_tokens: self.max_tokens,
            max_completion_tokens: None,
            stop: self.stop_sequences.map(StopSequence::Many),
            seed: None,
            presence_penalty: None,
            frequency_penalty: None,
            tools: self.tools.map(|tools| {
                tools
                    .into_iter()
                    .map(|mut tool| {
                        if let Some(object) = tool.as_object_mut()
                            && object.get("input_schema").is_some()
                            && matches!(
                                object.get("type").and_then(serde_json::Value::as_str),
                                None | Some("custom")
                            )
                        {
                            object.remove("type");
                            if let Some(schema) = object.remove("input_schema") {
                                object.insert("parameters".to_string(), schema);
                            }
                            return serde_json::json!({ "type": "function", "function": tool });
                        }
                        tool
                    })
                    .collect()
            }),
            // Anthropic clients read usage from `message_delta` instead.
            stream_options: None,
            reasoning_effort: self
                .output_config
                .as_ref()
                .and_then(|config| config.get("effort"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
                .or_else(|| {
                    self.thinking.and_then(|thinking| {
                        // `type: "disabled"` (or any non-enabled type) keeps thinking off.
                        if thinking
                            .kind
                            .as_deref()
                            .is_some_and(|kind| kind != "enabled")
                        {
                            return None;
                        }
                        let budget = thinking.budget_tokens.unwrap_or(0);
                        Some(
                            crate::upstream::chat_backend::thinking_level_from_budget(budget)
                                .to_string(),
                        )
                    })
                }),
            // Anthropic's own web search is a `tools` entry, not this field.
            web_search_options: None,
            tool_choice: anthropic_tool_choice_to_openai(self.tool_choice.as_ref())?,
            parallel_tool_calls: None,
            response_format: anthropic_output_config_to_openai(self.output_config.as_ref())?,
            metadata,
            user,
            service_tier: None,
            store: None,
            prompt_cache_key: None,
            verbosity: None,
            logprobs: None,
            top_logprobs: None,
        })
    }
}

/// Splits Anthropic's `metadata` into the hub's two fields.
fn split_anthropic_metadata(
    metadata: Option<&serde_json::Value>,
) -> (Option<serde_json::Value>, Option<String>) {
    let Some(metadata) = metadata else {
        return (None, None);
    };
    let user = metadata
        .get("user_id")
        .and_then(serde_json::Value::as_str)
        .filter(|user| !user.trim().is_empty())
        .map(str::to_string);
    (Some(metadata.clone()), user)
}

/// Translates Anthropic's `tool_choice` into the OpenAI spelling the shared
/// executor understands.
///
/// Anthropic spells its modes differently (`any` for required, `tool` with a
/// `name` for a named function), so the translation belongs here rather than in
/// the executor: one place per protocol, and the hub stays protocol-neutral.
fn anthropic_tool_choice_to_openai(
    value: Option<&serde_json::Value>,
) -> Result<Option<serde_json::Value>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let object = value
        .as_object()
        .ok_or_else(|| Error::BadRequest("tool_choice must be a JSON object".to_string()))?;
    let kind = object.get("type").and_then(serde_json::Value::as_str);

    match kind {
        Some("auto") => Ok(Some(serde_json::json!("auto"))),
        Some("any") => Ok(Some(serde_json::json!("required"))),
        Some("none") => Ok(Some(serde_json::json!("none"))),
        Some("tool") => {
            let name = object
                .get("name")
                .and_then(serde_json::Value::as_str)
                .filter(|name| !name.trim().is_empty())
                .ok_or_else(|| {
                    Error::BadRequest("tool_choice of type `tool` requires a `name`".to_string())
                })?;
            Ok(Some(serde_json::json!({
                "type": "function",
                "function": { "name": name }
            })))
        }
        Some(other) => Err(Error::BadRequest(format!(
            "unsupported tool_choice type `{other}`; use `auto`, `any`, `tool` or `none`"
        ))),
        None => Err(Error::BadRequest(
            "tool_choice requires a `type`".to_string(),
        )),
    }
}

/// Translates `output_config.format` into the OpenAI `response_format` shape.
fn anthropic_output_config_to_openai(
    value: Option<&serde_json::Value>,
) -> Result<Option<serde_json::Value>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let object = value
        .as_object()
        .ok_or_else(|| Error::BadRequest("output_config must be a JSON object".to_string()))?;
    if let Some(effort) = object.get("effort")
        && !matches!(effort.as_str(), Some("low" | "medium" | "high" | "max"))
    {
        return Err(Error::BadRequest(
            "output_config.effort must be low, medium, high, or max".to_string(),
        ));
    }
    let Some(format) = object.get("format") else {
        return Ok(None);
    };
    let kind = format.get("type").and_then(serde_json::Value::as_str);

    match kind {
        Some("json_object") => Ok(Some(serde_json::json!({ "type": "json_object" }))),
        Some("json_schema") => {
            let schema = format.get("schema").ok_or_else(|| {
                Error::BadRequest("output_config.format requires a `schema`".to_string())
            })?;
            Ok(Some(serde_json::json!({
                "type": "json_schema",
                "json_schema": {
                    // Anthropic does not send a name; the OpenAI shape requires
                    // one, and the executor only ever forwards it.
                    "name": format
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("output"),
                    "schema": schema,
                    "strict": format
                        .get("strict")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false),
                }
            })))
        }
        Some(other) => Err(Error::BadRequest(format!(
            "unsupported output_config.format type `{other}`; use `json_object` or `json_schema`"
        ))),
        None => Err(Error::BadRequest(
            "output_config.format requires a `type`".to_string(),
        )),
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
            "system" => "system",
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
                            extra: serde_json::Map::new(),
                        });
                    }
                }
                "image" => {
                    if let Some(source) = block.source.and_then(image_data_url) {
                        parts.push(OpenAiContentPart {
                            part_type: "image_url".to_string(),
                            text: None,
                            image_url: Some(OpenAiImageUrl { url: source }),
                            extra: serde_json::Map::new(),
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
                role: role.to_string(),
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
