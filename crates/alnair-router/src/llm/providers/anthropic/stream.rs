//! Anthropic SSE parsing, usage mapping, and non-streaming assembly.

use std::collections::HashMap;

use crate::llm::model_config::ModelCostRates;
use crate::llm::model_config::apply_custom_headers;
use crate::llm::providers::common::{
    calculate_token_costs, is_retryable_status, parse_tool_arguments_strict, retry_after_delay,
    retry_delay,
};
use crate::llm::providers::sse::parse_data_line;
use crate::llm::types::{ChatError, LlmStreamChunk};

use super::{AnthropicCompletion, AnthropicCompletionBlock, AnthropicRequest, PendingToolUse};

pub(super) fn anthropic_usage_chunk(
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
pub(super) fn anthropic_completion_chunks(
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

pub(super) fn handle_anthropic_sse_line(
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

pub(super) async fn send_anthropic_request_with_retry(
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
