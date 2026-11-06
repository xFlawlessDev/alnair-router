//! Response parsing for an openai-compatible upstream's Responses API.
//!
//! The stream is a typed event sequence rather than chat deltas. The events
//! that carry payload are `response.output_text.delta` (answer text),
//! `response.reasoning_summary_text.delta` / `response.reasoning_text.delta`
//! (reasoning) and `response.output_item.done` (a completed `function_call`,
//! with its `name`). Everything else — item lifecycle noise, web-search
//! progress, `response.created`, the argument deltas — is skipped rather than
//! guessed at.
//!
//! The one-shot body is the same `Response` object, so both paths share
//! [`completion_chunks`].

use serde_json::Value;

use crate::model_config::ModelCostRates;
use crate::providers::common::{calculate_token_costs, parse_tool_arguments_strict};
use crate::types::{ChatError, LlmStreamChunk};

/// Applies one SSE event.
///
/// Text and reasoning stream incrementally. A tool call is only reported once
/// it is complete, because `response.function_call_arguments.done` carries the
/// arguments but no `name`; the item lifecycle's final `output_item.done` event
/// carries the whole call, so this is where it is read.
pub(super) fn handle_event(
    event: &Value,
    _rates: Option<&ModelCostRates>,
) -> Result<Option<LlmStreamChunk>, ChatError> {
    match event_type(event) {
        "response.output_text.delta" => Ok(delta(event).map(LlmStreamChunk::Text)),
        "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
            Ok(delta(event).map(LlmStreamChunk::Thinking))
        }
        "response.output_item.done" => {
            let Some(item) = event.get("item") else {
                return Ok(None);
            };
            if item.get("type").and_then(Value::as_str) != Some("function_call") {
                return Ok(None);
            }
            tool_call_from_item(item).map(Some)
        }
        "response.failed" => Err(ChatError::Provider(
            response_error_message(event)
                .unwrap_or_else(|| "the upstream reported a failed response".to_string()),
        )),
        _ => Ok(None),
    }
}

/// True for the events that close a stream.
pub(super) fn is_terminal(event: &Value) -> bool {
    matches!(
        event_type(event),
        "response.completed" | "response.failed" | "response.incomplete"
    )
}

/// The finish reason for a terminal event.
///
/// `incomplete_details.reason` distinguishes a turn that hit the output limit
/// from one that stopped on its own; a failed turn carries no reason and is
/// surfaced as an error by [`handle_event`] instead.
pub(super) fn terminal_reason_for_event(event: &Value) -> Option<String> {
    if event_type(event) == "response.failed" {
        return None;
    }
    let response = event.get("response").unwrap_or(event);
    Some(terminal_reason(response))
}

pub(super) fn terminal_reason(response: &Value) -> String {
    if let Some(reason) = response
        .get("incomplete_details")
        .and_then(|details| details.get("reason"))
        .and_then(Value::as_str)
        && !reason.is_empty()
    {
        // The Responses API spells the output-limit stop `max_output_tokens`.
        return if reason == "max_output_tokens" {
            "length".to_string()
        } else {
            reason.to_string()
        };
    }

    match response.get("status").and_then(Value::as_str) {
        Some("incomplete") => "length".to_string(),
        _ => "stop".to_string(),
    }
}

/// Turns a one-shot `{output: [...], usage: {...}}` body into chunks.
pub(super) fn completion_chunks(
    body: &Value,
    rates: Option<&ModelCostRates>,
) -> Vec<LlmStreamChunk> {
    let mut chunks = Vec::new();
    let empty = Vec::new();
    let output = body
        .get("output")
        .and_then(Value::as_array)
        .unwrap_or(&empty);

    for item in output {
        match item.get("type").and_then(Value::as_str) {
            Some("message") => {
                let parts = item
                    .get("content")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                for part in parts {
                    let text = part.get("text").and_then(Value::as_str).unwrap_or_default();
                    if !text.is_empty() {
                        chunks.push(LlmStreamChunk::Text(text.to_string()));
                    }
                }
            }
            Some("reasoning") => {
                for text in reasoning_summary_texts(item) {
                    chunks.push(LlmStreamChunk::Thinking(text));
                }
            }
            Some("function_call") => match tool_call_from_item(item) {
                Ok(chunk) => chunks.push(chunk),
                Err(error) => chunks.push(LlmStreamChunk::Text(format!(
                    "[upstream returned a tool call this router could not read: {error}]"
                ))),
            },
            _ => {}
        }
    }

    if let Some(usage) = response_usage(body, rates) {
        chunks.push(usage);
    }
    chunks.push(LlmStreamChunk::Done(Some(terminal_reason(body))));
    chunks
}

fn event_type(event: &Value) -> &str {
    event
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn delta(event: &Value) -> Option<String> {
    event
        .get("delta")
        .and_then(Value::as_str)
        .filter(|delta| !delta.is_empty())
        .map(str::to_string)
}

/// Reads the message of a `response.failed` event, or the top-level `error` an
/// upstream may send instead of an event sequence.
pub(super) fn response_error_message(event: &Value) -> Option<String> {
    let error = event
        .get("response")
        .and_then(|response| response.get("error"))
        .or_else(|| event.get("error"))?;

    if let Some(message) = error.get("message").and_then(Value::as_str) {
        return Some(message.to_string());
    }
    (!error.is_null()).then(|| error.to_string())
}

fn tool_call_from_item(item: &Value) -> Result<LlmStreamChunk, ChatError> {
    let name = item
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| {
            ChatError::Provider(
                "an upstream function call arrived without a usable name".to_string(),
            )
        })?;
    // `call_id` is what a `function_call_output` must reference; `id` is the
    // item's own identifier and only a fallback.
    let id = item
        .get("call_id")
        .or_else(|| item.get("id"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let raw = item
        .get("arguments")
        .and_then(Value::as_str)
        .unwrap_or("{}");

    Ok(LlmStreamChunk::ToolCall {
        id,
        name: name.to_string(),
        arguments: parse_tool_arguments_strict(raw)?,
    })
}

/// Pulls the summary texts out of a `reasoning` output item.
fn reasoning_summary_texts(item: &Value) -> Vec<String> {
    let parts = ["summary", "content"];
    let mut texts = Vec::new();

    for key in parts {
        let Some(entries) = item.get(key).and_then(Value::as_array) else {
            continue;
        };
        for entry in entries {
            if let Some(text) = entry.get("text").and_then(Value::as_str)
                && !text.is_empty()
            {
                texts.push(text.to_string());
            }
        }
    }

    texts
}

/// Uploads the token counters onto a usage chunk.
///
/// The Responses shape reports `input_tokens` / `output_tokens` with the cached
/// and reasoning split under `input_tokens_details` / `output_tokens_details`.
pub(super) fn response_usage(
    body: &Value,
    rates: Option<&ModelCostRates>,
) -> Option<LlmStreamChunk> {
    let usage = body.get("usage")?.as_object()?;
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
