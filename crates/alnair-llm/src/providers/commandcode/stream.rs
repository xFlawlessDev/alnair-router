//! `/alpha/generate` event decoding.
//!
//! The CLI transport is newline-delimited JSON: one object per line, optionally
//! wrapped in an SSE `data:` prefix, terminated by a `finish` (or `error`)
//! event. Shapes mirror OmniRoute's `commandCode` executor.

use serde_json::Value;

/// One decoded line from the CLI stream.
#[derive(Debug, PartialEq)]
pub(super) enum CliEvent {
    Text(String),
    Reasoning(String),
    ToolCall {
        id: String,
        name: String,
        input: Value,
    },
    /// Terminal event: carries the upstream finish reason.
    Finish(String),
    Error(String),
    /// Anything the protocol sends that this router does not surface.
    Ignored,
}

/// Decodes one raw line into JSON, skipping keep-alives, comments, `event:`
/// headers and the `[DONE]` sentinel.
pub(super) fn decode_line(line: &str) -> Option<Value> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with(':') || trimmed.starts_with("event:") {
        return None;
    }

    let payload = match trimmed.strip_prefix("data:") {
        Some(rest) => rest.trim_start(),
        None => trimmed,
    };
    if payload.is_empty() || payload == "[DONE]" {
        return None;
    }

    serde_json::from_str(payload).ok()
}

/// Maps one event onto a chunk-level event, folding any usage it carries into
/// `usage` (the CLI reports usage across `finish-step`, `finish` and delta
/// events, always nested under a details object).
pub(super) fn event_from(value: &Value, usage: &mut UsageTotals) -> CliEvent {
    usage.merge(value);

    match value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "text-delta" => CliEvent::Text(string_field(value, "text").unwrap_or_default()),
        "reasoning-delta" => CliEvent::Reasoning(string_field(value, "text").unwrap_or_default()),
        "tool-call" => CliEvent::ToolCall {
            id: string_field(value, "toolCallId")
                .or_else(|| string_field(value, "id"))
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            name: string_field(value, "toolName")
                .or_else(|| string_field(value, "name"))
                .unwrap_or_default(),
            input: value
                .get("input")
                .or_else(|| value.get("args"))
                .or_else(|| value.get("arguments"))
                .filter(|input| input.is_object())
                .cloned()
                .unwrap_or_else(|| serde_json::json!({})),
        },
        "finish" => CliEvent::Finish(finish_reason(value.get("finishReason"))),
        "error" => CliEvent::Error(error_message(value)),
        _ => CliEvent::Ignored,
    }
}

/// Normalizes the upstream finish reason onto OpenAI's vocabulary.
pub(super) fn finish_reason(raw: Option<&Value>) -> String {
    match raw.and_then(Value::as_str).unwrap_or_default() {
        "tool-calls" | "tool_calls" | "toolUse" => "tool_calls".to_string(),
        "length" | "max_tokens" | "max-tokens" | "max_output_tokens" => "length".to_string(),
        _ => "stop".to_string(),
    }
}

fn error_message(value: &Value) -> String {
    value
        .get("error")
        .and_then(|error| {
            error
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| error.as_str())
        })
        .unwrap_or("Command Code stream error")
        .to_string()
}

fn string_field(value: &Value, field: &str) -> Option<String> {
    value.get(field).and_then(Value::as_str).map(str::to_string)
}

/// Token counts accumulated across the events of one response.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct UsageTotals {
    prompt: Option<u64>,
    cached: Option<u64>,
    completion: Option<u64>,
    reasoning: Option<u64>,
}

impl UsageTotals {
    /// True once any count has been seen.
    pub(super) fn is_empty(&self) -> bool {
        self.prompt.is_none()
            && self.cached.is_none()
            && self.completion.is_none()
            && self.reasoning.is_none()
    }

    /// `(prompt, cached, completion, reasoning)`.
    pub(super) fn counts(&self) -> (Option<u64>, Option<u64>, Option<u64>, Option<u64>) {
        (self.prompt, self.cached, self.completion, self.reasoning)
    }

    fn merge(&mut self, value: &Value) {
        let usage = if value.get("type").and_then(Value::as_str) == Some("finish-step") {
            value
                .get("usage")
                .or_else(|| value.get("totalUsage"))
                .cloned()
        } else {
            value
                .get("totalUsage")
                .or_else(|| value.get("usage"))
                .cloned()
        };
        let Some(usage) = usage.filter(Value::is_object) else {
            return;
        };

        let input_details = first_object(&usage, &["inputTokenDetails", "input_tokens_details"]);
        let output_details = first_object(&usage, &["outputTokenDetails", "output_tokens_details"]);
        let reasoning_details = first_object(
            &usage,
            &["reasoningTokenDetails", "reasoning_tokens_details"],
        );

        let cached = first_u64(
            Some(&usage),
            &[
                "cachedInputTokens",
                "cached_input_tokens",
                "cacheReadInputTokens",
                "cache_read_input_tokens",
                "cached_tokens",
            ],
        )
        .or_else(|| first_u64(input_details, &["cachedTokens", "cached_tokens"]));
        let reasoning = first_u64(Some(&usage), &["reasoningTokens", "reasoning_tokens"])
            .or_else(|| first_u64(output_details, &["reasoningTokens", "reasoning_tokens"]))
            .or_else(|| first_u64(reasoning_details, &["reasoningTokens", "reasoning_tokens"]));

        let prompt = first_u64(
            Some(&usage),
            &[
                "inputTokens",
                "input_tokens",
                "promptTokens",
                "prompt_tokens",
            ],
        )
        .or_else(|| {
            let uncached = first_u64(input_details, &["noCacheTokens", "no_cache_tokens"]);
            match (uncached, cached) {
                (None, None) => None,
                (uncached, cached) => Some(uncached.unwrap_or(0) + cached.unwrap_or(0)),
            }
        });
        let completion = first_u64(
            Some(&usage),
            &[
                "outputTokens",
                "output_tokens",
                "completionTokens",
                "completion_tokens",
            ],
        )
        .or_else(|| {
            let text = first_u64(output_details, &["textTokens", "text_tokens"]);
            match (text, reasoning) {
                (None, None) => None,
                (text, reasoning) => Some(text.unwrap_or(0) + reasoning.unwrap_or(0)),
            }
        });

        // Later events report cumulative totals, so the newest count wins.
        self.prompt = prompt.or(self.prompt);
        self.cached = cached.or(self.cached);
        self.completion = completion.or(self.completion);
        self.reasoning = reasoning.or(self.reasoning);
    }
}

fn first_object<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter()
        .find_map(|key| value.get(*key).filter(|entry| entry.is_object()))
}

fn first_u64(value: Option<&Value>, keys: &[&str]) -> Option<u64> {
    let value = value?;
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_u64))
}
