//! Response parsing for the Codex Responses SSE event sequence.
//!
//! The events that matter are `response.output_text.delta`,
//! `response.reasoning_summary_text.delta`, `response.output_item.done`
//! (a completed `function_call`) and `response.completed` (usage). Anything
//! else — item lifecycle noise, `response.created`, web-search progress — is
//! skipped rather than guessed at.

use std::collections::BTreeMap;

use futures::Stream;
use futures::StreamExt;
use serde_json::Value;

use crate::model_config::ModelCostRates;
use crate::providers::common::calculate_token_costs;
use crate::providers::sse::{SseLineBuffer, parse_data_line};
use crate::types::{ChatError, LlmStreamChunk};

/// Maps a raw upstream response onto router chunks.
pub(super) fn read_sse(
    response: reqwest::Response,
    rates: Option<ModelCostRates>,
) -> impl Stream<Item = Result<LlmStreamChunk, ChatError>> {
    async_stream::stream! {
        let mut stream = response.bytes_stream();
        let mut line_buffer = SseLineBuffer::default();
        let mut tool_calls: BTreeMap<String, LlmStreamChunk> = BTreeMap::new();
        let mut usage: Option<LlmStreamChunk> = None;
        let mut terminal: Option<String> = None;
        let mut finished = false;

        while let Some(chunk) = stream.next().await {
            let bytes = match chunk {
                Ok(bytes) => bytes,
                Err(error) => {
                    yield Err(ChatError::Provider(error.to_string()));
                    return;
                }
            };

            for line in line_buffer.push(&bytes) {
                let Some(data) = parse_data_line(&line) else {
                    continue;
                };
                if data == "[DONE]" {
                    finished = true;
                    break;
                }
                let Ok(event) = serde_json::from_str::<Value>(data) else {
                    // A malformed frame is skipped: the next `response.completed`
                    // still closes the turn, so one bad line must not lose it.
                    tracing::warn!(payload = %data, "Codex stream returned invalid JSON");
                    continue;
                };
                match handle_event(&event, rates.as_ref(), &mut tool_calls, &mut usage) {
                    Ok(Some(chunk)) => yield Ok(chunk),
                    Ok(None) => {}
                    Err(error) => {
                        yield Err(error);
                        return;
                    }
                }
                if is_terminal(&event) {
                    terminal = terminal_reason_for_event(&event);
                    finished = true;
                    break;
                }
            }
            if finished {
                break;
            }
        }

        // Tool calls arrive before the terminal event, so they are buffered and
        // flushed here in arrival order.
        for chunk in tool_calls.into_values() {
            yield Ok(chunk);
        }
        if let Some(usage) = usage {
            yield Ok(usage);
        }
        yield Ok(LlmStreamChunk::Done(terminal));
    }
}

/// The event type, or the item type of a `response.output_item.done` event.
fn event_type(event: &Value) -> &str {
    event
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn is_terminal(event: &Value) -> bool {
    matches!(event_type(event), "response.completed" | "response.failed")
}

/// Applies one SSE event. Reasoning and text stream incrementally; a finished
/// function call is buffered because its arguments only exist as a whole.
fn handle_event(
    event: &Value,
    rates: Option<&ModelCostRates>,
    tool_calls: &mut BTreeMap<String, LlmStreamChunk>,
    usage: &mut Option<LlmStreamChunk>,
) -> Result<Option<LlmStreamChunk>, ChatError> {
    match event_type(event) {
        "response.output_text.delta" => Ok(event
            .get("delta")
            .and_then(Value::as_str)
            .filter(|delta| !delta.is_empty())
            .map(|delta| LlmStreamChunk::Text(delta.to_string()))),
        "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => Ok(event
            .get("delta")
            .and_then(Value::as_str)
            .filter(|delta| !delta.is_empty())
            .map(|delta| LlmStreamChunk::Thinking(delta.to_string()))),
        "response.output_item.done" => {
            if let Some(item) = event.get("item") {
                collect_function_call(item, tool_calls)?;
            }
            Ok(None)
        }
        "response.completed" => {
            if let Some(response) = event.get("response") {
                *usage = response_usage(response, rates);
            }
            Ok(None)
        }
        "response.failed" => {
            let message = event
                .get("response")
                .and_then(|response| response.get("error"))
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("the Codex backend reported a failed response");
            Err(ChatError::Provider(format!("Codex API error: {message}")))
        }
        _ => Ok(None),
    }
}

/// Buffers a `function_call` item once the backend has finished emitting it.
fn collect_function_call(
    item: &Value,
    tool_calls: &mut BTreeMap<String, LlmStreamChunk>,
) -> Result<(), ChatError> {
    if item.get("type").and_then(Value::as_str) != Some("function_call") {
        return Ok(());
    }
    let Some(name) = item.get("name").and_then(Value::as_str) else {
        return Ok(());
    };
    let call_id = item
        .get("call_id")
        .or_else(|| item.get("id"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let raw = item
        .get("arguments")
        .and_then(Value::as_str)
        .unwrap_or("{}")
        .to_string();
    let arguments = crate::providers::common::parse_tool_arguments_strict(&raw)?;
    let key = if call_id.is_empty() {
        name.to_string()
    } else {
        call_id.clone()
    };
    tool_calls.insert(
        key,
        LlmStreamChunk::ToolCall {
            id: call_id,
            name: name.to_string(),
            arguments,
        },
    );
    Ok(())
}

/// Uploads the token counters onto a usage chunk.
///
/// The Responses shape reports `input_tokens` / `output_tokens` with the cached
/// and reasoning split under `input_tokens_details` / `output_tokens_details`.
fn response_usage(response: &Value, rates: Option<&ModelCostRates>) -> Option<LlmStreamChunk> {
    let usage = response.get("usage")?.as_object()?;
    let count = |value: Option<&Value>| value.and_then(Value::as_u64);
    let prompt = count(usage.get("input_tokens"));
    let completion = count(usage.get("output_tokens"));
    if prompt.is_none() && completion.is_none() {
        return None;
    }
    let cached = count(
        usage
            .get("input_tokens_details")
            .and_then(|details| details.get("cached_tokens")),
    );
    let reasoning = count(
        usage
            .get("output_tokens_details")
            .and_then(|details| details.get("reasoning_tokens")),
    );
    let costs = calculate_token_costs(rates, prompt, cached, None, completion, reasoning);

    Some(LlmStreamChunk::Usage {
        total_duration: None,
        prompt_eval_count: prompt,
        cached_prompt_eval_count: cached,
        eval_count: completion,
        reasoning_eval_count: reasoning,
        cost_input_usd: costs.input_usd,
        cost_output_usd: costs.output_usd,
        cost_reasoning_usd: costs.reasoning_usd,
        cost_cache_usd: costs.cache_usd,
    })
}

fn terminal_reason(body: &Value) -> Option<String> {
    match body
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("completed")
    {
        "incomplete" => Some("length".to_string()),
        _ => Some("stop".to_string()),
    }
}

/// Finish reason for a terminal SSE event. A turn that carried tool calls is
/// reported as `tool_calls` downstream; `incomplete_details` is what the
/// backend sets when it stopped on the output limit rather than by itself.
fn terminal_reason_for_event(event: &Value) -> Option<String> {
    if event_type(event) == "response.failed" {
        return None;
    }
    match event.get("response") {
        Some(response) => terminal_reason(response),
        None => Some("stop".to_string()),
    }
}
