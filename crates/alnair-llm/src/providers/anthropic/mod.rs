//! Anthropic native provider: request bodies, SSE streaming, and one-shot calls.

use std::collections::HashMap;

use futures::StreamExt;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};

use crate::model_config::{LlmStreamOptions, ModelConfig};
use crate::provider::LlmProvider;
use crate::providers::common::{
    RetryBudget, describe_non_json_body, is_html_content_type, normalize_base_url,
    response_content_type,
};
use crate::providers::sse::SseLineBuffer;
use crate::types::{ChatError, LlmStreamChunk, Message};

mod conv;
mod stream;

use conv::*;
use stream::*;

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
        let auth_style = config.auth_style;
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
                auth_style,
                &custom_headers,
                &request,
                options.max_retry_delay_ms,
                RetryBudget::new(options.max_status_retries, options.max_transport_retries),
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

            // A 200 carrying an HTML page is not an event stream. Left alone it
            // parses to zero `data:` lines, so the turn ends empty and reads as
            // the model saying nothing.
            if let Some(content_type) = response_content_type(&response)
                && is_html_content_type(&content_type)
            {
                let body = response.text().await.unwrap_or_default();
                yield Err(ChatError::Provider(format!(
                    "Anthropic streaming request returned {content_type} instead of an event stream: {}",
                    describe_non_json_body(&body)
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
        let auth_style = config.auth_style;
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
                auth_style,
                &custom_headers,
                &request,
                options.max_retry_delay_ms,
                RetryBudget::new(options.max_status_retries, options.max_transport_retries),
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
                Err(_) => {
                    yield Err(ChatError::Provider(format!(
                        "Anthropic returned a non-JSON response: {}",
                        describe_non_json_body(&body)
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
mod tests;
