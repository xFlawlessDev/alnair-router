use std::collections::HashMap;

use futures::StreamExt;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};

use crate::model_config::{CacheRetention, LlmStreamOptions, ModelConfig, ThinkingLevel};
use crate::provider::LlmProvider;
use crate::providers::common::normalize_base_url;
use crate::providers::sse::SseLineBuffer;
use crate::types::{ChatError, ContentPart, LlmStreamChunk, Message, MessageContent};

#[derive(Debug, Clone, Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_k: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop_sequences: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<Vec<AnthropicSystemBlock>>,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<AnthropicTool>>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<AnthropicThinking>,
}

impl AnthropicRequest {
    /// Whether any cache breakpoint requests the 1-hour TTL, which needs the
    /// `extended-cache-ttl-2025-04-11` beta header.
    fn uses_extended_cache_ttl(&self) -> bool {
        let system = self.system.iter().flatten().any(|block| {
            block
                .cache_control
                .as_ref()
                .is_some_and(AnthropicCacheControl::is_one_hour)
        });
        let messages = self
            .messages
            .iter()
            .flat_map(|message| &message.content)
            .any(|block| match block {
                AnthropicContentBlock::Text { cache_control, .. }
                | AnthropicContentBlock::ToolResult { cache_control, .. }
                | AnthropicContentBlock::Image { cache_control, .. } => cache_control
                    .as_ref()
                    .is_some_and(AnthropicCacheControl::is_one_hour),
                AnthropicContentBlock::Thinking { .. }
                | AnthropicContentBlock::RedactedThinking { .. }
                | AnthropicContentBlock::ToolUse { .. } => false,
            });
        let tools = self.tools.iter().flatten().any(|tool| {
            tool.cache_control
                .as_ref()
                .is_some_and(AnthropicCacheControl::is_one_hour)
        });
        system || messages || tools
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct AnthropicSystemBlock {
    #[serde(rename = "type")]
    kind: String,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<AnthropicCacheControl>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct AnthropicCacheControl {
    #[serde(rename = "type")]
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    ttl: Option<String>,
}

impl AnthropicCacheControl {
    fn is_one_hour(&self) -> bool {
        self.ttl.as_deref() == Some("1h")
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct AnthropicThinking {
    #[serde(rename = "type")]
    kind: String,
    budget_tokens: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
struct AnthropicMessage {
    role: String,
    content: Vec<AnthropicContentBlock>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicContentBlock {
    Text {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<AnthropicCacheControl>,
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
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<AnthropicCacheControl>,
    },
    Image {
        source: AnthropicImageSource,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<AnthropicCacheControl>,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicImageSource {
    Base64 { media_type: String, data: String },
    Url { url: String },
}

#[derive(Debug, Clone, Serialize)]
struct AnthropicTool {
    name: String,
    description: String,
    input_schema: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<AnthropicCacheControl>,
}

#[derive(Debug, Clone, Default)]
struct PendingToolUse {
    id: String,
    name: String,
    input_json: String,
}

#[derive(Debug, Clone, Deserialize)]
struct AnthropicErrorEnvelope {
    error: Option<AnthropicErrorObject>,
}

#[derive(Debug, Clone, Deserialize)]
struct AnthropicErrorObject {
    message: Option<String>,
}

/// Non-streaming Messages response body.
#[derive(Debug, Deserialize)]
struct AnthropicCompletion {
    #[serde(default)]
    content: Vec<AnthropicCompletionBlock>,
    #[serde(default)]
    usage: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicCompletionBlock {
    Text {
        text: String,
    },
    Thinking {
        thinking: String,
        #[serde(default)]
        signature: Option<String>,
    },
    RedactedThinking {
        data: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(other)]
    Other,
}

pub struct AnthropicNativeProvider {
    client: reqwest::Client,
}

impl AnthropicNativeProvider {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

impl Default for AnthropicNativeProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(private_interfaces)]
pub(crate) fn convert_messages_to_anthropic(
    messages: &[Message],
    tools: Option<&[serde_json::Value]>,
    options: &LlmStreamOptions,
    supports_thinking: bool,
    supports_cache_control: bool,
) -> AnthropicRequest {
    let system_parts: Vec<String> = messages
        .iter()
        .filter(|message| message.role == "system")
        .map(|message| message.content.as_text())
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .collect();

    // Thinking blocks may only be replayed while thinking stays enabled; the
    // API rejects them outright otherwise.
    let thinking = supports_thinking
        .then(|| anthropic_thinking_from_level(options.thinking_level))
        .flatten();
    let replay_thinking = thinking.is_some();

    let mut anthropic_messages = Vec::new();

    for message in messages {
        match message.role.as_str() {
            "system" => continue,
            "tool" => {
                anthropic_messages.push(AnthropicMessage {
                    role: "user".to_string(),
                    content: vec![AnthropicContentBlock::ToolResult {
                        tool_use_id: message
                            .tool_call_id
                            .clone()
                            .unwrap_or_else(|| "unknown".to_string()),
                        content: message.content.as_text(),
                        cache_control: None,
                    }],
                });
            }
            "user" => {
                let content = content_blocks_from_message_content(&message.content);
                if !content.is_empty() {
                    anthropic_messages.push(AnthropicMessage {
                        role: "user".to_string(),
                        content,
                    });
                }
            }
            "assistant" => {
                let mut content = Vec::new();

                // Thinking blocks from a prior turn (signed, unmodified) must be
                // replayed first; Anthropic rejects a final assistant tool-use
                // message that does not start with thinking when thinking is on.
                if replay_thinking {
                    if let Some(redacted) = &message.redacted_thinking {
                        content.push(AnthropicContentBlock::RedactedThinking {
                            data: redacted.clone(),
                        });
                    }
                    if let (Some(thinking), Some(signature)) = (
                        message.thinking.as_ref(),
                        message.thinking_signature.as_ref(),
                    ) && !thinking.trim().is_empty()
                    {
                        content.push(AnthropicContentBlock::Thinking {
                            thinking: thinking.clone(),
                            signature: signature.clone(),
                        });
                    }
                }

                content.extend(content_blocks_from_message_content(&message.content));

                if let Some(tool_calls) = &message.tool_calls {
                    for tool_call in tool_calls {
                        let input = serde_json::from_str::<serde_json::Value>(&tool_call.arguments)
                            .unwrap_or_else(|_| serde_json::json!({}));
                        content.push(AnthropicContentBlock::ToolUse {
                            id: tool_call.id.clone(),
                            name: tool_call.name.clone(),
                            input,
                        });
                    }
                }

                if !content.is_empty() {
                    anthropic_messages.push(AnthropicMessage {
                        role: "assistant".to_string(),
                        content,
                    });
                }
            }
            _ => {}
        }
    }

    let mark_cache =
        supports_cache_control && !matches!(options.cache_retention, CacheRetention::None);
    let mark_final_tool = mark_cache && system_parts.is_empty() && anthropic_messages.is_empty();
    let final_tool_marker = mark_final_tool
        .then(|| anthropic_cache_control(supports_cache_control, options.cache_retention))
        .flatten();
    let mut max_tokens = options.max_tokens.unwrap_or(8192).max(1) as u32;
    if let Some(thinking) = &thinking
        && max_tokens <= thinking.budget_tokens
    {
        max_tokens = thinking.budget_tokens + 1024;
    }

    let anthropic_messages = merge_consecutive_anthropic_messages(anthropic_messages);
    let system = anthropic_system_blocks(
        system_parts,
        supports_cache_control,
        options.cache_retention,
    );
    let messages = if system.is_none() {
        apply_anthropic_cache_marker(
            anthropic_messages,
            supports_cache_control,
            options.cache_retention,
        )
    } else {
        anthropic_messages
    };

    // Extended thinking constrains sampling: Anthropic rejects `temperature`,
    // `top_p`, and `top_k` alongside a thinking budget.
    let (temperature, top_p, top_k) = if thinking.is_some() {
        (None, None, None)
    } else {
        (options.temperature, options.top_p, options.top_k)
    };

    AnthropicRequest {
        // Filled by provider stream() from ModelConfig.
        model: String::new(),
        max_tokens,
        temperature,
        top_p,
        top_k,
        stop_sequences: options.stop.clone(),
        system,
        messages,
        tools: convert_tools_to_anthropic(tools, final_tool_marker),
        stream: true,
        thinking,
    }
}

/// Collapses consecutive same-role turns into one, concatenating their blocks.
///
/// Anthropic's API combines consecutive `user`/`assistant` turns, but
/// Anthropic-compatible and Bedrock endpoints still reject them, and a `tool`
/// message mapped to a `user` turn can land next to a real user turn.
fn merge_consecutive_anthropic_messages(messages: Vec<AnthropicMessage>) -> Vec<AnthropicMessage> {
    let mut merged: Vec<AnthropicMessage> = Vec::with_capacity(messages.len());
    for message in messages {
        match merged.last_mut() {
            Some(last) if last.role == message.role => last.content.extend(message.content),
            _ => merged.push(message),
        }
    }
    merged
}

fn anthropic_thinking_from_level(level: Option<ThinkingLevel>) -> Option<AnthropicThinking> {
    let budget_tokens = match level.unwrap_or(ThinkingLevel::None) {
        ThinkingLevel::None => return None,
        ThinkingLevel::Minimal => 1024,
        ThinkingLevel::Low => 4096,
        ThinkingLevel::Medium => 8192,
        ThinkingLevel::High => 16384,
        ThinkingLevel::Max => 32768,
    };

    Some(AnthropicThinking {
        kind: "enabled".to_string(),
        budget_tokens,
    })
}

fn anthropic_cache_control(
    supports_cache_control: bool,
    cache_retention: CacheRetention,
) -> Option<AnthropicCacheControl> {
    if !supports_cache_control || matches!(cache_retention, CacheRetention::None) {
        return None;
    }
    Some(AnthropicCacheControl {
        kind: "ephemeral".to_string(),
        // Long retention uses Anthropic's 1-hour cache TTL, which requires the
        // `extended-cache-ttl-2025-04-11` beta header (sent when retention is Long).
        ttl: matches!(cache_retention, CacheRetention::Long).then(|| "1h".to_string()),
    })
}

fn anthropic_system_blocks(
    system_parts: Vec<String>,
    supports_cache_control: bool,
    cache_retention: CacheRetention,
) -> Option<Vec<AnthropicSystemBlock>> {
    if system_parts.is_empty() {
        return None;
    }
    let marker = anthropic_cache_control(supports_cache_control, cache_retention);
    let last_index = system_parts.len().saturating_sub(1);
    Some(
        system_parts
            .into_iter()
            .enumerate()
            .map(|(index, text)| AnthropicSystemBlock {
                kind: "text".to_string(),
                text,
                cache_control: (index == last_index).then(|| marker.clone()).flatten(),
            })
            .collect(),
    )
}

fn apply_anthropic_cache_marker(
    mut messages: Vec<AnthropicMessage>,
    supports_cache_control: bool,
    cache_retention: CacheRetention,
) -> Vec<AnthropicMessage> {
    let Some(marker) = anthropic_cache_control(supports_cache_control, cache_retention) else {
        return messages;
    };
    if messages.is_empty() {
        return messages;
    }
    if let Some(block) = messages
        .last_mut()
        .and_then(|message| message.content.last_mut())
    {
        match block {
            AnthropicContentBlock::Text { cache_control, .. }
            | AnthropicContentBlock::ToolResult { cache_control, .. }
            | AnthropicContentBlock::Image { cache_control, .. } => {
                *cache_control = Some(marker);
            }
            AnthropicContentBlock::Thinking { .. }
            | AnthropicContentBlock::RedactedThinking { .. }
            | AnthropicContentBlock::ToolUse { .. } => {}
        }
    }
    messages
}

fn content_blocks_from_message_content(content: &MessageContent) -> Vec<AnthropicContentBlock> {
    match content {
        MessageContent::Text(text) => {
            if text.trim().is_empty() {
                Vec::new()
            } else {
                vec![AnthropicContentBlock::Text {
                    text: text.clone(),
                    cache_control: None,
                }]
            }
        }
        MessageContent::Parts(parts) => {
            let mut blocks = Vec::new();
            for part in parts {
                match part {
                    ContentPart::Text(text_part) => {
                        if !text_part.text.trim().is_empty() {
                            blocks.push(AnthropicContentBlock::Text {
                                text: text_part.text.clone(),
                                cache_control: None,
                            });
                        }
                    }
                    ContentPart::ImageUrl(image_part) => {
                        let url = image_part.image_url.url.as_str();
                        if let Some((media_type, data)) = parse_base64_data_url(url) {
                            blocks.push(AnthropicContentBlock::Image {
                                source: AnthropicImageSource::Base64 { media_type, data },
                                cache_control: None,
                            });
                        } else {
                            blocks.push(AnthropicContentBlock::Image {
                                source: AnthropicImageSource::Url {
                                    url: url.to_string(),
                                },
                                cache_control: None,
                            });
                        }
                    }
                }
            }
            blocks
        }
    }
}

fn parse_base64_data_url(value: &str) -> Option<(String, String)> {
    let prefix = "data:";
    if !value.starts_with(prefix) {
        return None;
    }

    let payload = &value[prefix.len()..];
    let sep = payload.find(";base64,")?;
    let media_type = payload[..sep].trim();
    let data = payload[(sep + ";base64,".len())..].trim();

    if media_type.is_empty() || data.is_empty() {
        return None;
    }

    Some((media_type.to_string(), data.to_string()))
}

fn convert_tools_to_anthropic(
    tools: Option<&[serde_json::Value]>,
    final_tool_marker: Option<AnthropicCacheControl>,
) -> Option<Vec<AnthropicTool>> {
    let tools = tools?;
    if tools.is_empty() {
        return None;
    }

    let mut anthropic_tools = Vec::new();
    for tool in tools {
        let function = tool.get("function").unwrap_or(tool);
        let name = function
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
        if name.is_empty() {
            continue;
        }

        let description = function
            .get("description")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();

        let mut input_schema = function
            .get("parameters")
            .or_else(|| function.get("input_schema"))
            .cloned()
            .unwrap_or_else(|| serde_json::json!({ "type": "object", "properties": {} }));
        if !matches!(
            input_schema.get("type").and_then(|value| value.as_str()),
            Some("object")
        ) {
            input_schema = serde_json::json!({ "type": "object", "properties": {} });
        }

        anthropic_tools.push(AnthropicTool {
            name,
            description,
            input_schema,
            cache_control: None,
        });
    }

    if let Some(marker) = final_tool_marker
        && let Some(tool) = anthropic_tools.last_mut()
    {
        tool.cache_control = Some(marker);
    }

    if anthropic_tools.is_empty() {
        None
    } else {
        Some(anthropic_tools)
    }
}

impl LlmProvider for AnthropicNativeProvider {
    fn stream<'a>(
        &'a self,
        config: &'a ModelConfig,
        messages: Vec<Message>,
        options: &'a LlmStreamOptions,
        tools: Option<Vec<serde_json::Value>>,
    ) -> BoxStream<'a, Result<LlmStreamChunk, ChatError>> {
        let mut request = convert_messages_to_anthropic(
            &messages,
            tools.as_deref(),
            options,
            config.supports_thinking,
            config.supports_cache_control,
        );
        request.model = config.model_id.clone();
        request.stream = true;

        let endpoint = format!("{}/messages", normalize_base_url(&config.base_url));
        let api_key = config.api_key.clone();
        let custom_headers = config.custom_headers.clone();

        Box::pin(async_stream::stream! {
            // TODO E4: non-streaming path
            let Some(api_key) = api_key.as_deref().filter(|key| !key.trim().is_empty()) else {
                yield Err(ChatError::BadRequest("Anthropic API key is required".to_string()));
                return;
            };
            let response = match send_anthropic_request_with_retry(
                &self.client,
                &endpoint,
                api_key,
                &custom_headers,
                &request,
                options.max_retry_delay_ms,
                options.max_retries,
            )
            .await
            {
                Ok(response) => response,
                Err(error) => {
                    yield Err(error);
                    return;
                }
            };

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                let provider_message = serde_json::from_str::<AnthropicErrorEnvelope>(&body)
                    .ok()
                    .and_then(|payload| payload.error.and_then(|error| error.message))
                    .unwrap_or(body);
                yield Err(ChatError::Provider(format!(
                    "Anthropic API error {status}: {provider_message}"
                )));
                return;
            }

            let rates = config.effective_cost_rates();
            let mut stream = response.bytes_stream();
            let mut line_buffer = SseLineBuffer::default();
            let mut pending_tool_uses: HashMap<usize, PendingToolUse> = HashMap::new();
            let mut usage_totals = serde_json::Map::new();
            while let Some(chunk) = stream.next().await {
                let bytes = match chunk {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        yield Err(ChatError::Provider(error.to_string()));
                        return;
                    }
                };

                for line in line_buffer.push(&bytes) {
                    match handle_anthropic_sse_line(
                        &line,
                        &mut pending_tool_uses,
                        &mut usage_totals,
                        rates.as_ref(),
                    ) {
                        Ok((chunks, is_terminal)) => {
                            for chunk in chunks {
                                yield Ok(chunk);
                            }
                            if is_terminal {
                                return;
                            }
                        }
                        Err(error) => {
                            yield Err(error);
                            return;
                        }
                    }
                }
            }

            if let Some(line) = line_buffer.finish() {
                match handle_anthropic_sse_line(
                    &line,
                    &mut pending_tool_uses,
                    &mut usage_totals,
                    rates.as_ref(),
                ) {
                    Ok((chunks, _)) => {
                        for chunk in chunks {
                            yield Ok(chunk);
                        }
                    }
                    Err(error) => yield Err(error),
                }
            }
        })
    }

    /// One-shot `/messages` call with `stream: false`.
    fn complete<'a>(
        &'a self,
        config: &'a ModelConfig,
        messages: Vec<Message>,
        options: &'a LlmStreamOptions,
        tools: Option<Vec<serde_json::Value>>,
    ) -> BoxStream<'a, Result<LlmStreamChunk, ChatError>> {
        let mut request = convert_messages_to_anthropic(
            &messages,
            tools.as_deref(),
            options,
            config.supports_thinking,
            config.supports_cache_control,
        );
        request.model = config.model_id.clone();
        request.stream = false;

        let endpoint = format!("{}/messages", normalize_base_url(&config.base_url));
        let api_key = config.api_key.clone();
        let custom_headers = config.custom_headers.clone();
        let rates = config.effective_cost_rates();

        Box::pin(async_stream::stream! {
            let Some(api_key) = api_key.as_deref().filter(|key| !key.trim().is_empty()) else {
                yield Err(ChatError::BadRequest("Anthropic API key is required".to_string()));
                return;
            };

            let response = match send_anthropic_request_with_retry(
                &self.client,
                &endpoint,
                api_key,
                &custom_headers,
                &request,
                options.max_retry_delay_ms,
                options.max_retries,
            )
            .await
            {
                Ok(response) => response,
                Err(error) => {
                    yield Err(error);
                    return;
                }
            };

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                let provider_message = serde_json::from_str::<AnthropicErrorEnvelope>(&body)
                    .ok()
                    .and_then(|payload| payload.error.and_then(|error| error.message))
                    .unwrap_or(body);
                yield Err(ChatError::Provider(format!(
                    "Anthropic API error {status}: {provider_message}"
                )));
                return;
            }

            let body = match response.text().await {
                Ok(body) => body,
                Err(error) => {
                    yield Err(ChatError::Provider(error.to_string()));
                    return;
                }
            };
            let parsed: AnthropicCompletion = match serde_json::from_str(&body) {
                Ok(parsed) => parsed,
                Err(error) => {
                    yield Err(ChatError::Provider(format!(
                        "Anthropic returned malformed JSON: {error}"
                    )));
                    return;
                }
            };

            for chunk in anthropic_completion_chunks(parsed, rates.as_ref()) {
                yield Ok(chunk);
            }
        })
    }
}

mod stream;

use stream::*;

#[cfg(test)]
mod tests;
