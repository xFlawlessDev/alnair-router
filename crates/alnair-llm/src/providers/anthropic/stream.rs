//! Anthropic SSE parsing, usage mapping, and non-streaming assembly.

use std::collections::HashMap;

use crate::model_config::AuthStyle;
use crate::model_config::ModelCostRates;
use crate::model_config::apply_custom_headers;
use crate::providers::common::{
    calculate_token_costs, is_retryable_status, parse_tool_arguments_strict, retry_after_delay,
    retry_delay,
};
use crate::providers::sse::parse_data_line;
use crate::types::{ChatError, LlmStreamChunk};

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
    // Anthropic bills thinking as output tokens and reports no separate count.
    let costs = calculate_token_costs(
        rates,
        prompt_eval_count,
        cached_prompt_eval_count,
        cached_write_count,
        eval_count,
        None,
    );

    LlmStreamChunk::Usage {
        total_duration: None,
        prompt_eval_count,
        cached_prompt_eval_count,
        eval_count,
        reasoning_eval_count: None,
        cost_input_usd: costs.input_usd,
        cost_output_usd: costs.output_usd,
        cost_reasoning_usd: costs.reasoning_usd,
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
            AnthropicCompletionBlock::Thinking {
                thinking,
                signature,
            } => {
                if !thinking.is_empty() {
                    chunks.push(LlmStreamChunk::Thinking(thinking));
                }
                if let Some(signature) = signature {
                    chunks.push(LlmStreamChunk::ThinkingSignature(signature));
                }
            }
            AnthropicCompletionBlock::RedactedThinking { data } => {
                chunks.push(LlmStreamChunk::RedactedThinking(data));
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
    usage_totals: &mut serde_json::Map<String, serde_json::Value>,
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
                "signature_delta" => {
                    if let Some(signature) = delta.get("signature").and_then(|value| value.as_str())
                        && !signature.is_empty()
                    {
                        chunks.push(LlmStreamChunk::ThinkingSignature(signature.to_string()));
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
            } else if content_block.get("type").and_then(|value| value.as_str())
                == Some("redacted_thinking")
                && let Some(data) = content_block.get("data").and_then(|value| value.as_str())
            {
                chunks.push(LlmStreamChunk::RedactedThinking(data.to_string()));
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
                merge_usage_fields(usage_totals, usage);
            }
            if !usage_totals.is_empty() {
                chunks.push(anthropic_usage_chunk(
                    &serde_json::Value::Object(usage_totals.clone()),
                    rates,
                ));
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
                merge_usage_fields(usage_totals, usage);
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

/// Accumulates usage fields across stream events.
///
/// Anthropic reports `input_tokens`/cache counts on `message_start` and the
/// cumulative `output_tokens` on `message_delta`, so a later, non-null value
/// wins while earlier fields are preserved.
fn merge_usage_fields(
    totals: &mut serde_json::Map<String, serde_json::Value>,
    update: &serde_json::Value,
) {
    let Some(update) = update.as_object() else {
        return;
    };
    for (key, value) in update {
        if !value.is_null() {
            totals.insert(key.clone(), value.clone());
        }
    }
}

/// Header name and value carrying the Anthropic credential.
///
/// An API key travels in `x-api-key`; a subscription/OAuth session token must
/// travel in `Authorization: Bearer`. Sending the wrong one 401s the request.
pub(super) fn anthropic_auth_header(
    api_key: &str,
    auth_style: AuthStyle,
) -> (&'static str, String) {
    match auth_style {
        AuthStyle::ApiKey => ("x-api-key", api_key.to_string()),
        AuthStyle::Bearer => ("authorization", format!("Bearer {api_key}")),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn send_anthropic_request_with_retry(
    client: &reqwest::Client,
    endpoint: &str,
    api_key: &str,
    auth_style: AuthStyle,
    custom_headers: &std::collections::BTreeMap<String, String>,
    request_body: &AnthropicRequest,
    max_retry_delay_ms: u64,
    max_retries: usize,
) -> Result<reqwest::Response, ChatError> {
    let mut last_error = None;
    let (auth_name, auth_value) = anthropic_auth_header(api_key, auth_style);
    for attempt in 0..=max_retries {
        let mut request = client
            .post(endpoint)
            .header(auth_name, auth_value.clone())
            .header("anthropic-version", "2023-06-01")
            .header(reqwest::header::CONTENT_TYPE, "application/json");
        if request_body.uses_extended_cache_ttl() {
            request = request.header("anthropic-beta", "extended-cache-ttl-2025-04-11");
        }
        let request = request.json(request_body);
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
