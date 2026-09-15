use std::collections::HashMap;

use futures::StreamExt;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};

use crate::llm::model_config::{
    CacheRetention, LlmStreamOptions, ModelConfig, ModelCostRates, ThinkingLevel,
    apply_custom_headers,
};
use crate::llm::provider::LlmProvider;
use crate::llm::providers::common::{
    calculate_token_costs, is_retryable_status, normalize_base_url, parse_tool_arguments_strict,
    retry_after_delay, retry_delay,
};
use crate::llm::providers::sse::{SseLineBuffer, parse_data_line};
use crate::llm::types::{ChatError, ContentPart, LlmStreamChunk, Message, MessageContent};

#[derive(Debug, Clone, Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<Vec<AnthropicSystemBlock>>,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<AnthropicTool>>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<AnthropicThinking>,
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

                if message
                    .thinking
                    .as_ref()
                    .is_some_and(|thinking| !thinking.trim().is_empty())
                {
                    // ponytail: prior Anthropic thinking replay needs signed thinking-block persistence;
                    // strip history thinking until Message can store provider signatures.
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
    let thinking = supports_thinking
        .then(|| anthropic_thinking_from_level(options.thinking_level))
        .flatten();
    let mut max_tokens = options.max_tokens.unwrap_or(8192).max(1) as u32;
    if let Some(thinking) = &thinking
        && max_tokens <= thinking.budget_tokens
    {
        max_tokens = thinking.budget_tokens + 1024;
    }

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

    AnthropicRequest {
        // Filled by provider stream() from ModelConfig.
        model: String::new(),
        max_tokens,
        system,
        messages,
        tools: convert_tools_to_anthropic(tools, mark_final_tool),
        stream: true,
        thinking,
    }
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
            AnthropicContentBlock::ToolUse { .. } => {}
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
    mark_final_tool: bool,
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

    if mark_final_tool && let Some(tool) = anthropic_tools.last_mut() {
        tool.cache_control = Some(AnthropicCacheControl {
            kind: "ephemeral".to_string(),
        });
    }

    if anthropic_tools.is_empty() {
        None
    } else {
        Some(anthropic_tools)
    }
}

fn anthropic_usage_chunk(
    usage: &serde_json::Value,
    rates: Option<&ModelCostRates>,
) -> LlmStreamChunk {
    let prompt_eval_count = usage.get("input_tokens").and_then(|value| value.as_u64());
    let cached_prompt_eval_count = usage
        .get("cache_read_input_tokens")
        .and_then(|value| value.as_u64());
    let cached_write_count = usage
        .get("cache_creation_input_tokens")
        .and_then(|value| value.as_u64());
    let eval_count = usage.get("output_tokens").and_then(|value| value.as_u64());
    let costs = calculate_token_costs(
        rates,
        prompt_eval_count,
        cached_prompt_eval_count,
        cached_write_count,
        eval_count,
    );

    LlmStreamChunk::Usage {
        total_duration: None,
        prompt_eval_count,
        cached_prompt_eval_count,
        eval_count,
        cost_input_usd: costs.input_usd,
        cost_output_usd: costs.output_usd,
    }
}

/// Maps a non-streaming Messages response body into chunks.
fn anthropic_completion_chunks(
    completion: AnthropicCompletion,
    rates: Option<&ModelCostRates>,
) -> Vec<LlmStreamChunk> {
    let mut chunks = Vec::new();

    for block in completion.content {
        match block {
            AnthropicCompletionBlock::Text { text } if !text.is_empty() => {
                chunks.push(LlmStreamChunk::Text(text));
            }
            AnthropicCompletionBlock::Thinking { thinking } if !thinking.is_empty() => {
                chunks.push(LlmStreamChunk::Thinking(thinking));
            }
            AnthropicCompletionBlock::ToolUse { id, name, input } => {
                chunks.push(LlmStreamChunk::ToolCall {
                    id,
                    name,
                    arguments: input,
                });
            }
            _ => {}
        }
    }

    if let Some(usage) = completion.usage {
        chunks.push(anthropic_usage_chunk(&usage, rates));
    }
    chunks.push(LlmStreamChunk::Done(None));
    chunks
}

fn handle_anthropic_sse_line(
    line: &str,
    pending_tool_uses: &mut HashMap<usize, PendingToolUse>,
    rates: Option<&ModelCostRates>,
) -> Result<(Vec<LlmStreamChunk>, bool), ChatError> {
    let Some(data) = parse_data_line(line) else {
        return Ok((Vec::new(), false));
    };
    if data.is_empty() {
        return Ok((Vec::new(), false));
    }

    let payload: serde_json::Value = match serde_json::from_str(data) {
        Ok(payload) => payload,
        Err(_) if data.contains("error") => {
            return Err(ChatError::Provider(format!(
                "Anthropic stream returned invalid error JSON: {data}"
            )));
        }
        Err(_) => {
            tracing::warn!(payload = %data, "Anthropic stream returned invalid JSON");
            return Ok((Vec::new(), false));
        }
    };

    let mut chunks = Vec::new();
    let event_type = payload
        .get("type")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    match event_type {
        "content_block_delta" => {
            let index = payload
                .get("index")
                .and_then(|value| value.as_u64())
                .unwrap_or(0) as usize;
            let Some(delta) = payload.get("delta") else {
                return Ok((chunks, false));
            };
            match delta
                .get("type")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
            {
                "text_delta" => {
                    if let Some(text) = delta.get("text").and_then(|value| value.as_str())
                        && !text.is_empty()
                    {
                        chunks.push(LlmStreamChunk::Text(text.to_string()));
                    }
                }
                "thinking_delta" => {
                    if let Some(thinking) = delta.get("thinking").and_then(|value| value.as_str())
                        && !thinking.is_empty()
                    {
                        chunks.push(LlmStreamChunk::Thinking(thinking.to_string()));
                    }
                }
                "input_json_delta" => {
                    if let Some(partial_json) =
                        delta.get("partial_json").and_then(|value| value.as_str())
                    {
                        pending_tool_uses
                            .entry(index)
                            .or_default()
                            .input_json
                            .push_str(partial_json);
                    }
                }
                _ => {}
            }
        }
        "content_block_start" => {
            let index = payload
                .get("index")
                .and_then(|value| value.as_u64())
                .unwrap_or(0) as usize;
            let Some(content_block) = payload.get("content_block") else {
                return Ok((chunks, false));
            };
            if content_block.get("type").and_then(|value| value.as_str()) == Some("tool_use") {
                pending_tool_uses.insert(
                    index,
                    PendingToolUse {
                        id: content_block
                            .get("id")
                            .and_then(|value| value.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        name: content_block
                            .get("name")
                            .and_then(|value| value.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        input_json: String::new(),
                    },
                );
            }
        }
        "content_block_stop" => {
            let index = payload
                .get("index")
                .and_then(|value| value.as_u64())
                .unwrap_or(0) as usize;
            if let Some(tool_use) = pending_tool_uses.remove(&index) {
                if tool_use.id.trim().is_empty() || tool_use.name.trim().is_empty() {
                    tracing::warn!(index, id = %tool_use.id, name = %tool_use.name, "dropping Anthropic tool use without id/name");
                    return Ok((chunks, false));
                }
                chunks.push(LlmStreamChunk::ToolCall {
                    id: tool_use.id,
                    name: tool_use.name,
                    arguments: parse_tool_arguments_strict(&tool_use.input_json)?,
                });
            }
        }
        "message_delta" => {
            if let Some(usage) = payload.get("usage") {
                chunks.push(anthropic_usage_chunk(usage, rates));
            }
            if let Some(stop_reason) = payload
                .get("delta")
                .and_then(|value| value.get("stop_reason"))
                .and_then(|value| value.as_str())
            {
                chunks.push(LlmStreamChunk::Done(Some(stop_reason.to_string())));
                return Ok((chunks, true));
            }
        }
        "message_start" => {
            if let Some(usage) = payload.get("message").and_then(|value| value.get("usage")) {
                chunks.push(anthropic_usage_chunk(usage, rates));
            }
        }
        "error" => {
            let message = payload
                .get("error")
                .and_then(|value| value.get("message"))
                .and_then(|value| value.as_str())
                .unwrap_or("Anthropic streaming error")
                .to_string();
            return Err(ChatError::Provider(message));
        }
        _ => {}
    }

    Ok((chunks, false))
}

async fn send_anthropic_request_with_retry(
    client: &reqwest::Client,
    endpoint: &str,
    api_key: &str,
    custom_headers: &std::collections::BTreeMap<String, String>,
    request_body: &AnthropicRequest,
    max_retry_delay_ms: u64,
    max_retries: usize,
) -> Result<reqwest::Response, ChatError> {
    let mut last_error = None;
    for attempt in 0..=max_retries {
        let request = client
            .post(endpoint)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .json(request_body);
        let request = apply_custom_headers(request, custom_headers)?;
        match request.send().await {
            Ok(response) if is_retryable_status(response.status()) && attempt < max_retries => {
                let status = response.status();
                let delay = retry_after_delay(response.headers())
                    .unwrap_or_else(|| retry_delay(attempt, max_retry_delay_ms));
                tracing::warn!(
                    status = %status,
                    retry = attempt + 1,
                    max_retries = max_retries,
                    ?delay,
                    "Anthropic request returned retryable status; retrying"
                );
                tokio::time::sleep(delay).await;
            }
            Ok(response) => return Ok(response),
            Err(error) if attempt < max_retries => {
                let delay = retry_delay(attempt, max_retry_delay_ms);
                tracing::warn!(
                    error = %error,
                    retry = attempt + 1,
                    max_retries = max_retries,
                    ?delay,
                    "Anthropic request failed; retrying"
                );
                last_error = Some(ChatError::Provider(error.to_string()));
                tokio::time::sleep(delay).await;
            }
            Err(error) => return Err(ChatError::Provider(error.to_string())),
        }
    }

    Err(last_error.unwrap_or_else(|| ChatError::Provider("Anthropic request failed".to_string())))
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
            while let Some(chunk) = stream.next().await {
                let bytes = match chunk {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        yield Err(ChatError::Provider(error.to_string()));
                        return;
                    }
                };

                for line in line_buffer.push(&bytes) {
                    match handle_anthropic_sse_line(&line, &mut pending_tool_uses, rates.as_ref()) {
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
                match handle_anthropic_sse_line(&line, &mut pending_tool_uses, rates.as_ref()) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::model_config::ThinkingLevel;
    use crate::llm::types::{Message, MessageToolCall};
    use futures::StreamExt;

    fn convert_for_test(messages: &[Message], options: &LlmStreamOptions) -> AnthropicRequest {
        convert_messages_to_anthropic(messages, None, options, true, true)
    }

    #[test]
    fn completion_json_maps_to_chunks() {
        let parsed: AnthropicCompletion = serde_json::from_str(
            r#"{
                "content": [
                    { "type": "thinking", "thinking": "pondering" },
                    { "type": "text", "text": "hello" },
                    { "type": "tool_use", "id": "tc1", "name": "read", "input": { "path": "/tmp" } },
                    { "type": "redacted_thinking", "data": "opaque" }
                ],
                "usage": { "input_tokens": 10, "output_tokens": 2 }
            }"#,
        )
        .expect("valid completion");

        let chunks = anthropic_completion_chunks(parsed, None);

        assert!(matches!(&chunks[0], LlmStreamChunk::Thinking(text) if text == "pondering"));
        assert!(matches!(&chunks[1], LlmStreamChunk::Text(text) if text == "hello"));
        assert!(matches!(
            &chunks[2],
            LlmStreamChunk::ToolCall { id, name, .. } if id == "tc1" && name == "read"
        ));
        assert!(matches!(
            &chunks[3],
            LlmStreamChunk::Usage {
                prompt_eval_count: Some(10),
                eval_count: Some(2),
                ..
            }
        ));
        assert!(matches!(&chunks[4], LlmStreamChunk::Done(_)));
        assert_eq!(chunks.len(), 5, "unknown block types are skipped");
    }

    #[test]
    fn convert_messages_extracts_system_to_top_level() {
        let messages = vec![
            Message::new("system", "You are helpful"),
            Message::new("user", "hello"),
        ];
        let request = convert_for_test(&messages, &LlmStreamOptions::default());

        assert_eq!(
            request.system,
            Some(vec![AnthropicSystemBlock {
                kind: "text".to_string(),
                text: "You are helpful".to_string(),
                cache_control: None,
            }])
        );
        assert_eq!(request.messages.len(), 1);
        assert_eq!(request.messages[0].role, "user");
    }

    #[test]
    fn convert_messages_maps_tool_calls_to_tool_use_blocks() {
        let mut assistant = Message::new("assistant", "");
        assistant.tool_calls = Some(vec![MessageToolCall {
            id: "tc1".to_string(),
            name: "read_file".to_string(),
            arguments: r#"{"path":"/tmp"}"#.to_string(),
        }]);

        let request = convert_for_test(&[assistant], &LlmStreamOptions::default());
        assert_eq!(request.messages.len(), 1);

        let has_tool_use = request.messages[0].content.iter().any(|block| {
            matches!(
                block,
                AnthropicContentBlock::ToolUse { id, name, input }
                    if id == "tc1"
                        && name == "read_file"
                        && *input == serde_json::json!({"path": "/tmp"})
            )
        });
        assert!(has_tool_use);
    }

    #[test]
    fn convert_messages_maps_tool_results_to_tool_result_blocks() {
        let mut tool_message = Message::new("tool", "file content");
        tool_message.tool_call_id = Some("tc1".to_string());

        let request = convert_for_test(&[tool_message], &LlmStreamOptions::default());

        assert_eq!(request.messages.len(), 1);
        assert_eq!(request.messages[0].role, "user");

        let has_tool_result = request.messages[0].content.iter().any(|block| {
            matches!(
                block,
                AnthropicContentBlock::ToolResult { tool_use_id, content, .. }
                    if tool_use_id == "tc1" && content == "file content"
            )
        });
        assert!(has_tool_result);
    }

    #[test]
    fn thinking_level_minimal_sets_budget_1024() {
        let options = LlmStreamOptions {
            thinking_level: Some(ThinkingLevel::Minimal),
            ..LlmStreamOptions::default()
        };

        let request = convert_for_test(&[Message::new("user", "hello")], &options);

        let thinking = request.thinking.expect("thinking config");
        assert_eq!(thinking.kind, "enabled");
        assert_eq!(thinking.budget_tokens, 1024);
        assert_eq!(request.max_tokens, 8192);
    }

    #[test]
    fn thinking_level_max_bumps_max_tokens_when_needed() {
        let options = LlmStreamOptions {
            max_tokens: Some(1024),
            thinking_level: Some(ThinkingLevel::Max),
            ..LlmStreamOptions::default()
        };

        let request = convert_for_test(&[Message::new("user", "hello")], &options);

        let thinking = request.thinking.expect("thinking config");
        assert_eq!(thinking.budget_tokens, 32768);
        assert_eq!(request.max_tokens, 33792);
    }

    #[test]
    fn thinking_level_ignored_when_model_lacks_capability() {
        let options = LlmStreamOptions {
            thinking_level: Some(ThinkingLevel::Minimal),
            ..LlmStreamOptions::default()
        };

        let request = convert_messages_to_anthropic(
            &[Message::new("user", "hello")],
            None,
            &options,
            false,
            true,
        );

        assert_eq!(request.thinking, None);
    }

    #[test]
    fn cache_marker_applies_to_last_system_block_only() {
        let options = LlmStreamOptions {
            cache_retention: CacheRetention::Short,
            ..LlmStreamOptions::default()
        };
        let request = convert_for_test(
            &[
                Message::new("system", "one"),
                Message::new("system", "two"),
                Message::new("user", "hello"),
            ],
            &options,
        );

        let system = request.system.expect("system blocks");
        assert_eq!(
            system
                .iter()
                .filter(|block| block.cache_control.is_some())
                .count(),
            1
        );
        assert_eq!(
            system[1]
                .cache_control
                .as_ref()
                .map(|cache| cache.kind.as_str()),
            Some("ephemeral")
        );
    }

    #[test]
    fn tool_schema_preserves_nested_fields() {
        let tools = vec![serde_json::json!({
            "type": "function",
            "function": {
                "name": "search",
                "description": "Search docs",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {"type": "string", "description": "Search text"}
                    },
                    "required": ["query"],
                    "additionalProperties": false
                }
            }
        })];

        let converted = convert_tools_to_anthropic(Some(&tools), false).expect("tools");

        assert_eq!(converted[0].input_schema["additionalProperties"], false);
    }

    #[tokio::test]
    async fn missing_api_key_fails_before_http_request() {
        let provider = AnthropicNativeProvider::new();
        let config = ModelConfig::anthropic("http://127.0.0.1:1", "claude-test", None);
        let options = LlmStreamOptions::default();
        let mut stream =
            provider.stream(&config, vec![Message::new("user", "hello")], &options, None);

        let error = stream
            .next()
            .await
            .expect("first stream item")
            .expect_err("missing key should fail");

        assert!(error.to_string().contains("Anthropic API key is required"));
    }

    #[test]
    fn cache_marker_omitted_when_capability_disabled() {
        let options = LlmStreamOptions {
            cache_retention: CacheRetention::Short,
            ..LlmStreamOptions::default()
        };
        let request = convert_messages_to_anthropic(
            &[Message::new("user", "hello")],
            None,
            &options,
            true,
            false,
        );

        let has_marker = request.messages.iter().any(|message| {
            message.content.iter().any(|block| match block {
                AnthropicContentBlock::Text { cache_control, .. }
                | AnthropicContentBlock::ToolResult { cache_control, .. }
                | AnthropicContentBlock::Image { cache_control, .. } => cache_control.is_some(),
                AnthropicContentBlock::ToolUse { .. } => false,
            })
        });
        assert!(!has_marker);
    }

    #[test]
    fn prior_unsigned_thinking_is_stripped_from_history() {
        let mut assistant = Message::new("assistant", "answer");
        assistant.thinking = Some("private reasoning".to_string());

        let request = convert_for_test(&[assistant], &LlmStreamOptions::default());

        assert_eq!(request.messages[0].content.len(), 1);
    }

    #[test]
    fn stream_parser_reads_message_delta_usage() {
        let mut pending = HashMap::new();
        let (chunks, terminal) = handle_anthropic_sse_line(
            r#"data: {"type":"message_delta","usage":{"input_tokens":10,"cache_read_input_tokens":4,"output_tokens":2}}"#,
            &mut pending,
            None,
        )
        .expect("usage event");

        assert!(!terminal);
        assert!(matches!(
            chunks.first(),
            Some(LlmStreamChunk::Usage {
                prompt_eval_count: Some(10),
                cached_prompt_eval_count: Some(4),
                eval_count: Some(2),
                ..
            })
        ));
    }

    #[test]
    fn stream_parser_reports_terminal_done() {
        let mut pending = HashMap::new();
        let (chunks, terminal) = handle_anthropic_sse_line(
            r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"}}"#,
            &mut pending,
            None,
        )
        .expect("done event");

        assert!(terminal);
        assert!(
            matches!(chunks.first(), Some(LlmStreamChunk::Done(Some(reason))) if reason == "end_turn")
        );
    }

    #[test]
    fn stream_parser_handles_unterminated_final_sse_line() {
        let mut buffer = SseLineBuffer::default();
        assert!(buffer.push(br#"data:{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}"#).is_empty());
        let line = buffer.finish().expect("leftover line");
        let mut pending = HashMap::new();

        let (chunks, terminal) =
            handle_anthropic_sse_line(&line, &mut pending, None).expect("line");

        assert!(!terminal);
        assert!(matches!(chunks.first(), Some(LlmStreamChunk::Text(text)) if text == "hi"));
    }

    #[test]
    fn stream_parser_rejects_malformed_tool_input() {
        let mut pending = HashMap::new();
        handle_anthropic_sse_line(
            r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"search"}}"#,
            &mut pending,
            None,
        )
        .expect("start");
        handle_anthropic_sse_line(
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"not json"}}"#,
            &mut pending,
            None,
        )
        .expect("delta");

        let error = handle_anthropic_sse_line(
            r#"data: {"type":"content_block_stop","index":0}"#,
            &mut pending,
            None,
        )
        .expect_err("malformed input must fail");

        assert!(
            error
                .to_string()
                .contains("provider returned malformed tool JSON")
        );
    }
}
