use std::collections::HashMap;

use bytes::Bytes;
use futures::StreamExt;
use futures::stream::BoxStream;
use tracing::warn;

use crate::llm::model_config::{LlmStreamOptions, ModelConfig, ThinkingLevel, apply_custom_headers};
use crate::llm::provider::LlmProvider;
use crate::llm::providers::common::{
    PROVIDER_MAX_RETRIES, calculate_token_costs, is_retryable_status, normalize_base_url,
    parse_tool_arguments_strict, retry_after_delay, retry_delay,
};
use crate::llm::providers::sse::{SseLineBuffer, parse_data_line};
use crate::llm::types::{ChatError, LlmStreamChunk, Message};

pub struct OpenAiProvider {
    client: reqwest::Client,
}

impl OpenAiProvider {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    pub fn stream_with_mode<'a>(
        &'a self,
        config: &'a ModelConfig,
        messages: Vec<Message>,
        options: &'a LlmStreamOptions,
        tools: Option<Vec<serde_json::Value>>,
        use_streaming: bool,
    ) -> BoxStream<'a, Result<LlmStreamChunk, ChatError>> {
        let base_url = config.base_url.clone();
        let model = config.model_id.clone();
        let api_key = config.api_key.clone();
        let custom_headers = config.custom_headers.clone();
        let capture_thinking = !matches!(options.thinking_level, Some(ThinkingLevel::None));

        Box::pin(async_stream::stream! {
            let endpoint = format!("{}/chat/completions", normalize_base_url(&base_url));

            let openai_messages = crate::llm::types::convert_messages_to_openai_format(&messages);
            if openai_messages.is_empty() {
                yield Err(ChatError::BadRequest("No valid messages to send".to_string()));
                return;
            }

            let mut request_body = serde_json::json!({
                "model": model,
                "messages": openai_messages,
                "stream": use_streaming
            });

            if use_streaming {
                request_body["stream_options"] = serde_json::json!({
                    "include_usage": true
                });
            }

            options.apply_to_openai_compatible_body(
                &mut request_body,
                config.supports_thinking,
                config.supports_cache_control,
            );
            apply_openai_chat_model_compatibility(&model, &mut request_body);

            if let Some(ref tools) = tools
                && !tools.is_empty()
            {
                request_body["tools"] = serde_json::json!(tools);
            }

            let mut response = match send_openai_request_with_retry(
                &self.client,
                &endpoint,
                api_key.as_deref(),
                &custom_headers,
                &request_body,
                Some(options.max_retry_delay_ms),
            )
            .await
            {
                Ok(response) => response,
                Err(error) => {
                    warn!(error = %error, "OpenAI request failed after retries");
                    yield Err(error);
                    return;
                }
            };

            let mut retried_without_tools = false;
            let mut retried_without_reasoning = false;
            loop {
                if response.status().is_success() {
                    break;
                }

                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                log_openai_request_failure(status, &body, &request_body, retried_without_tools);

                if should_retry_openai_without_reasoning(status, &body, &request_body) {
                    warn!(
                        status = %status,
                        error = %truncate_for_log(&body, 2000),
                        "OpenAI-compatible provider rejected reasoning_effort; retrying without reasoning_effort"
                    );
                    if let Some(object) = request_body.as_object_mut() {
                        object.remove("reasoning_effort");
                        retried_without_reasoning = true;
                    }
                    response = match send_openai_request_with_retry(
                        &self.client,
                        &endpoint,
                        api_key.as_deref(),
                        &custom_headers,
                        &request_body,
                        Some(options.max_retry_delay_ms),
                    )
                    .await
                    {
                        Ok(response) => response,
                        Err(error) => {
                            warn!(error = %error, "OpenAI retry without reasoning_effort failed");
                            yield Err(error);
                            return;
                        }
                    };
                    continue;
                }

                if should_retry_openai_without_tools(status, &body, &request_body) {
                    warn!(
                        status = %status,
                        error = %truncate_for_log(&body, 2000),
                        "OpenAI-compatible provider rejected automatic tool choice; retrying without tools"
                    );
                    if let Some(object) = request_body.as_object_mut() {
                        object.remove("tools");
                        object.remove("tool_choice");
                        retried_without_tools = true;
                    }
                    response = match send_openai_request_with_retry(
                        &self.client,
                        &endpoint,
                        api_key.as_deref(),
                        &custom_headers,
                        &request_body,
                        Some(options.max_retry_delay_ms),
                    )
                    .await
                    {
                        Ok(response) => response,
                        Err(error) => {
                            warn!(error = %error, "OpenAI retry without tools failed");
                            yield Err(error);
                            return;
                        }
                    };
                    continue;
                }

                if retried_without_tools {
                    // Already retried without tools — give up gracefully.
                    // Agent can continue without tool calls on next turn.
                    yield Ok(LlmStreamChunk::Text(format!(
                        "[provider error {status}: request failed even without tools]"
                    )));
                    yield Ok(LlmStreamChunk::Done(Some("stop".to_string())));
                    return;
                }

                if retried_without_reasoning {
                    warn!(
                        status = %status,
                        error = %truncate_for_log(&body, 2000),
                        "OpenAI-compatible retry without reasoning_effort still failed"
                    );
                }
                yield Err(ChatError::Provider(format!("OpenAI API error {status}: {body}")));
                return;
            }

            if !use_streaming {
                let text = response.text().await.unwrap_or_default();
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                    let mut finish_reason = None;
                    let mut emitted_tool_call = false;
                    if let Some(choices) = json.get("choices").and_then(|c| c.as_array())
                        && let Some(choice) = choices.first()
                    {
                        finish_reason = choice
                            .get("finish_reason")
                            .and_then(|r| r.as_str())
                            .map(String::from);

                        if let Some(message) = choice.get("message") {
                            if capture_thinking
                                && let Some(reasoning) = extract_reasoning_text(message)
                                && !reasoning.is_empty()
                            {
                                yield Ok(LlmStreamChunk::Thinking(reasoning.to_string()));
                            }

                            if let Some(content) = message.get("content").and_then(|c| c.as_str())
                                && !content.is_empty()
                            {
                                let parsed = split_embedded_thinking(content);
                                if let Some(thinking) = parsed.thinking
                                    && capture_thinking
                                    && !thinking.is_empty()
                                {
                                    yield Ok(LlmStreamChunk::Thinking(thinking));
                                }
                                if !parsed.text.is_empty() {
                                    yield Ok(LlmStreamChunk::Text(parsed.text));
                                }
                            }
                        }
                        if let Some(tool_calls) = choice
                            .get("message")
                            .and_then(|m| m.get("tool_calls"))
                            .and_then(|t| t.as_array())
                        {
                            for tc in tool_calls {
                                let id = tc
                                    .get("id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default()
                                    .to_string();
                                let name = extract_tool_call_name(tc).unwrap_or_default().to_string();
                                let arguments_fragment = extract_tool_call_arguments_fragment(tc)
                                    .unwrap_or_else(|| "{}".to_string());
                                let arguments = arguments_fragment.as_str();
                                let Some(chunk) = (match build_tool_call_chunk(id, name, arguments) {
                                    Ok(chunk) => chunk,
                                    Err(error) => {
                                        yield Err(error);
                                        return;
                                    }
                                }) else {
                                    warn!(
                                        tool_call = %tc,
                                        "dropping OpenAI-compatible tool call without function.name"
                                    );
                                    continue;
                                };
                                yield Ok(chunk);
                                emitted_tool_call = true;
                            }
                        }
                    }
                    if let Some(usage) = extract_usage(&json) {
                        let prompt_eval_count = usage.get("prompt_tokens").and_then(|v| v.as_u64());
                        let cached_prompt_eval_count = usage
                            .get("prompt_tokens_details")
                            .and_then(|details| details.get("cached_tokens"))
                            .and_then(|v| v.as_u64())
                            .or_else(|| usage.get("cached_tokens").and_then(|v| v.as_u64()));
                        let eval_count = usage.get("completion_tokens").and_then(|v| v.as_u64());
                        let rates = config.effective_cost_rates();
                        let costs = calculate_token_costs(
                            rates.as_ref(),
                            prompt_eval_count,
                            cached_prompt_eval_count,
                            None,
                            eval_count,
                        );
                        yield Ok(LlmStreamChunk::Usage {
                            total_duration: None,
                            prompt_eval_count,
                            cached_prompt_eval_count,
                            eval_count,
                            cost_input_usd: costs.input_usd,
                            cost_output_usd: costs.output_usd,
                        });
                    }
                    yield Ok(LlmStreamChunk::Done(finish_reason_for_tool_calls(
                        finish_reason,
                        emitted_tool_call,
                    )));
                } else {
                    warn!(
                        response_preview = %truncate_for_log(&text, 500),
                        "non-streaming OpenAI response is not valid JSON"
                    );
                    yield Err(ChatError::Provider(format!(
                        "OpenAI response is not valid JSON: {}",
                        truncate_for_log(&text, 200)
                    )));
                }
                return;
            }

            // Streaming mode: parse SSE
            let mut stream = response.bytes_stream();
            let mut tool_call_accumulator: HashMap<usize, (String, String, String)> =
                HashMap::new();
            let mut pending_finish_reason: Option<String> = None;
            let mut embedded_thinking = EmbeddedThinkingStreamParser::default();
            let mut sse_line_buffer = SseLineBuffer::default();

            while let Some(chunk) = stream.next().await {
                let bytes: Bytes = match chunk {
                    Ok(b) => b,
                    Err(e) => {
                        warn!(error = %e, "SSE stream disconnected");
                        yield Err(ChatError::Provider(e.to_string()));
                        return;
                    }
                };

                for line in sse_line_buffer.push(&bytes) {
                    let Some(data) = parse_data_line(&line) else {
                        continue;
                    };
                    if data == "[DONE]" {
                        let had_tool_calls = !tool_call_accumulator.is_empty();
                        // Flush any accumulated tool calls
                        let mut indices: Vec<usize> = tool_call_accumulator.keys().copied().collect();
                        indices.sort();
                        for idx in indices {
                            if let Some((id, name, arguments)) = tool_call_accumulator.remove(&idx) {
                                let Some(chunk) = (match build_tool_call_chunk(id.clone(), name, &arguments) {
                                    Ok(chunk) => chunk,
                                    Err(error) => {
                                        yield Err(error);
                                        return;
                                    }
                                }) else {
                                    warn!(
                                        index = idx,
                                        id = %id,
                                        arguments_preview = %truncate_for_log(&arguments, 500),
                                        "dropping OpenAI-compatible tool call without function.name"
                                    );
                                    continue;
                                };
                                yield Ok(chunk);
                            }
                        }
                        yield Ok(LlmStreamChunk::Done(terminal_stream_finish_reason(
                            pending_finish_reason,
                            had_tool_calls,
                        )));
                        return;
                    }

                    let Ok(json) = serde_json::from_str::<serde_json::Value>(data) else {
                        if data.contains("error") {
                            yield Err(ChatError::Provider(format!("OpenAI stream returned invalid error JSON: {data}")));
                            return;
                        }
                        warn!(payload = %truncate_for_log(data, 500), "OpenAI stream returned invalid JSON");
                        continue;
                    };

                    if let Some(choices) = json.get("choices").and_then(|c| c.as_array())
                        && let Some(choice) = choices.first()
                    {
                        if let Some(delta) = choice.get("delta") {
                            if capture_thinking
                                && let Some(reasoning) = extract_reasoning_text(delta)
                                && !reasoning.is_empty()
                            {
                                yield Ok(LlmStreamChunk::Thinking(reasoning.to_string()));
                            }

                            if let Some(content) = delta.get("content").and_then(|c| c.as_str())
                                && !content.is_empty()
                            {
                                for parsed in embedded_thinking.push(content) {
                                    match parsed {
                                        ParsedEmbeddedThinkingChunk::Thinking(thinking) => {
                                            if capture_thinking {
                                                yield Ok(LlmStreamChunk::Thinking(thinking));
                                            }
                                        }
                                        ParsedEmbeddedThinkingChunk::Text(text) => {
                                            yield Ok(LlmStreamChunk::Text(text));
                                        }
                                    }
                                }
                            }

                            // Accumulate tool_calls fragments
                            if let Some(tool_calls) =
                                delta.get("tool_calls").and_then(|t| t.as_array())
                            {
                                for tc in tool_calls {
                                    let index =
                                        tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0)
                                            as usize;
                                    let entry = tool_call_accumulator
                                        .entry(index)
                                        .or_insert_with(|| {
                                            (String::new(), String::new(), String::new())
                                        });

                                    if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
                                        entry.0 = id.to_string();
                                    }
                                    if let Some(name) = extract_tool_call_name(tc) {
                                        entry.1 = name.to_string();
                                    }
                                    if let Some(args) = extract_tool_call_arguments_fragment(tc) {
                                        entry.2.push_str(&args);
                                    }
                                }
                            }
                        }
                        if let Some(reason) = choice.get("finish_reason").and_then(|r| r.as_str())
                            && reason != "null"
                        {
                            pending_finish_reason = Some(reason.to_string());
                        }
                    }

                    if let Some(usage) = extract_usage(&json) {
                        let prompt_eval_count = usage.get("prompt_tokens").and_then(|v| v.as_u64());
                        let cached_prompt_eval_count = usage
                            .get("prompt_tokens_details")
                            .and_then(|details| details.get("cached_tokens"))
                            .and_then(|v| v.as_u64())
                            .or_else(|| usage.get("cached_tokens").and_then(|v| v.as_u64()));
                        let eval_count = usage.get("completion_tokens").and_then(|v| v.as_u64());
                        let rates = config.effective_cost_rates();
                        let costs = calculate_token_costs(
                            rates.as_ref(),
                            prompt_eval_count,
                            cached_prompt_eval_count,
                            None,
                            eval_count,
                        );
                        yield Ok(LlmStreamChunk::Usage {
                            total_duration: None,
                            prompt_eval_count,
                            cached_prompt_eval_count,
                            eval_count,
                            cost_input_usd: costs.input_usd,
                            cost_output_usd: costs.output_usd,
                        });
                    }
                }
            }

            for parsed in embedded_thinking.flush() {
                match parsed {
                    ParsedEmbeddedThinkingChunk::Thinking(thinking) => {
                        if capture_thinking {
                            yield Ok(LlmStreamChunk::Thinking(thinking));
                        }
                    }
                    ParsedEmbeddedThinkingChunk::Text(text) => {
                        yield Ok(LlmStreamChunk::Text(text));
                    }
                }
            }

            let had_tool_calls = !tool_call_accumulator.is_empty();
            let mut indices: Vec<usize> = tool_call_accumulator.keys().copied().collect();
            indices.sort();
            for idx in indices {
                if let Some((id, name, arguments)) = tool_call_accumulator.remove(&idx) {
                    let Some(chunk) = (match build_tool_call_chunk(id.clone(), name, &arguments) {
                        Ok(chunk) => chunk,
                        Err(error) => {
                            yield Err(error);
                            return;
                        }
                    }) else {
                        warn!(
                            index = idx,
                            id = %id,
                            arguments_preview = %truncate_for_log(&arguments, 500),
                            "dropping OpenAI-compatible tool call without function.name"
                        );
                        continue;
                    };
                    yield Ok(chunk);
                }
            }
            yield Ok(LlmStreamChunk::Done(terminal_stream_finish_reason(
                pending_finish_reason,
                had_tool_calls,
            )));
        })
    }
}

impl Default for OpenAiProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl LlmProvider for OpenAiProvider {
    fn stream<'a>(
        &'a self,
        config: &'a ModelConfig,
        messages: Vec<Message>,
        options: &'a LlmStreamOptions,
        tools: Option<Vec<serde_json::Value>>,
    ) -> BoxStream<'a, Result<LlmStreamChunk, ChatError>> {
        self.stream_with_mode(config, messages, options, tools, true)
    }
}

impl LlmStreamOptions {
    pub fn apply_to_openai_body(&self, body: &mut serde_json::Value) {
        macro_rules! set {
            ($field:ident) => {
                if let Some(v) = self.$field {
                    body[stringify!($field)] = serde_json::json!(v);
                }
            };
        }
        set!(temperature);
        set!(top_p);
        set!(presence_penalty);
        set!(frequency_penalty);
        if let Some(v) = self.max_tokens
            && v > 0
        {
            body["max_tokens"] = serde_json::json!(v);
        }
        set!(seed);
        if let Some(ref v) = self.stop {
            body["stop"] = serde_json::json!(v);
        }
        if let Some(reasoning_effort) = openai_reasoning_effort(self.thinking_level) {
            body["reasoning_effort"] = serde_json::json!(reasoning_effort);
        }
    }
}

fn openai_reasoning_effort(level: Option<ThinkingLevel>) -> Option<&'static str> {
    match level.unwrap_or(ThinkingLevel::None) {
        ThinkingLevel::None => None,
        ThinkingLevel::Minimal => Some("minimal"),
        ThinkingLevel::Low => Some("low"),
        ThinkingLevel::Medium => Some("medium"),
        ThinkingLevel::High => Some("high"),
        ThinkingLevel::Max => Some("xhigh"),
    }
}

fn apply_openai_chat_model_compatibility(model: &str, body: &mut serde_json::Value) {
    if !uses_gpt5_chat_parameter_shape(model) {
        return;
    }

    if let Some(max_tokens) = body
        .as_object_mut()
        .and_then(|object| object.remove("max_tokens"))
    {
        body["max_completion_tokens"] = max_tokens;
    }

    if let Some(object) = body.as_object_mut() {
        object.remove("temperature");
        object.remove("top_p");
        object.remove("presence_penalty");
        object.remove("frequency_penalty");
        object.remove("seed");
    }
}

fn uses_gpt5_chat_parameter_shape(model: &str) -> bool {
    let model = model
        .rsplit(['/', ':'])
        .next()
        .unwrap_or(model)
        .to_ascii_lowercase();

    model == "gpt-5" || model.starts_with("gpt-5-") || model.starts_with("gpt-5.")
}

fn finish_reason_for_tool_calls(reason: Option<String>, has_tool_calls: bool) -> Option<String> {
    if has_tool_calls {
        Some("tool_calls".to_string())
    } else {
        reason
    }
}

fn terminal_stream_finish_reason(reason: Option<String>, has_tool_calls: bool) -> Option<String> {
    finish_reason_for_tool_calls(reason.or_else(|| Some("stop".to_string())), has_tool_calls)
}

#[cfg(test)]
fn provider_network_error_text(error: &ChatError) -> String {
    format!(
        "[provider request failed after retry: {}]",
        truncate_for_log(&error.to_string(), 200)
    )
}

fn log_openai_request_failure(
    status: reqwest::StatusCode,
    body: &str,
    request_body: &serde_json::Value,
    retrying_without_tools: bool,
) {
    let model = request_body
        .get("model")
        .and_then(|value| value.as_str())
        .unwrap_or("<unknown>");
    let request_keys = summarize_request_keys(request_body);
    let tool_count = request_body
        .get("tools")
        .and_then(|tools| tools.as_array())
        .map_or(0, Vec::len);
    let tool_names = summarize_tool_names(request_body.get("tools"));

    warn!(
        status = %status,
        model,
        request_keys = %request_keys,
        tool_count,
        tool_names = %tool_names,
        retrying_without_tools,
        error = %truncate_for_log(body, 2000),
        "OpenAI-compatible request rejected; diagnostic summary"
    );
}

fn summarize_request_keys(request_body: &serde_json::Value) -> String {
    let Some(object) = request_body.as_object() else {
        return "<non-object>".to_string();
    };

    let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
    keys.sort_unstable();
    keys.join(",")
}

fn summarize_tool_names(tools: Option<&serde_json::Value>) -> String {
    let Some(tools) = tools.and_then(|value| value.as_array()) else {
        return "<none>".to_string();
    };

    let names: Vec<&str> = tools
        .iter()
        .filter_map(|tool| {
            tool.get("function")
                .and_then(|function| function.get("name"))
                .and_then(|name| name.as_str())
        })
        .take(40)
        .collect();

    if tools.len() > names.len() {
        format!(
            "{} (+{} unnamed)",
            names.join(","),
            tools.len() - names.len()
        )
    } else {
        names.join(",")
    }
}

#[cfg(test)]
fn drain_complete_sse_lines(buffer: &mut String, chunk: &str) -> Vec<String> {
    buffer.push_str(chunk);

    let mut lines = Vec::new();
    while let Some(newline_index) = buffer.find('\n') {
        let mut line: String = buffer.drain(..=newline_index).collect();
        if line.ends_with('\n') {
            line.pop();
        }
        if line.ends_with('\r') {
            line.pop();
        }
        lines.push(line);
    }

    lines
}

fn extract_tool_call_name(tool_call: &serde_json::Value) -> Option<&str> {
    tool_call
        .get("function")
        .and_then(|function| function.get("name"))
        .and_then(|name| name.as_str())
        .or_else(|| tool_call.get("name").and_then(|name| name.as_str()))
        .or_else(|| {
            tool_call
                .get("function_name")
                .and_then(|name| name.as_str())
        })
        .or_else(|| tool_call.get("tool_name").and_then(|name| name.as_str()))
        .filter(|name| !name.trim().is_empty())
}

fn extract_tool_call_arguments_fragment(tool_call: &serde_json::Value) -> Option<String> {
    let arguments = tool_call
        .get("function")
        .and_then(|function| function.get("arguments"))
        .or_else(|| tool_call.get("arguments"))?;

    if let Some(arguments) = arguments.as_str() {
        return Some(arguments.to_string());
    }

    Some(arguments.to_string())
}

fn build_tool_call_chunk(
    id: String,
    name: String,
    arguments: &str,
) -> Result<Option<LlmStreamChunk>, ChatError> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Ok(None);
    }

    Ok(Some(LlmStreamChunk::ToolCall {
        id,
        arguments: parse_tool_call_arguments_lossy(arguments, &name)?,
        name,
    }))
}

fn parse_tool_call_arguments_lossy(
    raw: &str,
    tool_name: &str,
) -> Result<serde_json::Value, ChatError> {
    match parse_tool_arguments_strict(raw) {
        Ok(arguments) => Ok(arguments),
        Err(error) => {
            let escaped = escape_invalid_json_backslashes(raw);
            if escaped != raw
                && let Ok(arguments) = parse_tool_arguments_strict(&escaped)
            {
                warn!(
                    tool_name,
                    raw_preview = %truncate_for_log(raw, 240),
                    "repaired provider tool call JSON arguments with invalid backslash escapes"
                );
                return Ok(arguments);
            }

            let repaired = repair_incomplete_json(raw);
            if repaired != raw
                && let Ok(arguments) = parse_tool_arguments_strict(&repaired)
            {
                warn!(
                    tool_name,
                    raw_preview = %truncate_for_log(raw, 240),
                    "repaired incomplete provider tool call JSON arguments"
                );
                return Ok(arguments);
            }

            let escaped_repaired = escape_invalid_json_backslashes(&repaired);
            if escaped_repaired != repaired
                && let Ok(arguments) = parse_tool_arguments_strict(&escaped_repaired)
            {
                warn!(
                    tool_name,
                    raw_preview = %truncate_for_log(raw, 240),
                    "repaired provider tool call JSON arguments with incomplete JSON and invalid backslash escapes"
                );
                return Ok(arguments);
            }

            warn!(
                tool_name,
                error = %error,
                raw_preview = %truncate_for_log(raw, 240),
                "provider returned malformed tool call JSON arguments"
            );
            Err(error)
        }
    }
}

fn escape_invalid_json_backslashes(raw: &str) -> String {
    let mut repaired = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    let mut changed = false;

    while let Some(ch) = chars.next() {
        if ch != '\\' {
            repaired.push(ch);
            continue;
        }

        match chars.peek().copied() {
            Some('"' | '\\' | '/' | 'b' | 'f' | 'n' | 'r' | 't' | 'u') => {
                repaired.push(ch);
            }
            Some(_) | None => {
                repaired.push('\\');
                repaired.push('\\');
                changed = true;
            }
        }
    }

    if changed { repaired } else { raw.to_string() }
}

fn repair_incomplete_json(raw: &str) -> String {
    let mut repaired = trim_to_complete_json_prefix(raw.trim());
    if repaired.is_empty() {
        repaired = raw.trim().to_string();
    }

    let mut stack = Vec::new();
    let mut in_string = false;
    let mut escaped = false;

    for ch in repaired.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => stack.push('}'),
            '[' => stack.push(']'),
            '}' | ']' if stack.last().copied() == Some(ch) => {
                stack.pop();
            }
            _ => {}
        }
    }

    if in_string {
        repaired.push('"');
    }

    while matches!(repaired.chars().last(), Some(',' | ':')) {
        repaired.pop();
    }

    while let Some(ch) = stack.pop() {
        repaired.push(ch);
    }

    repaired
}

fn trim_to_complete_json_prefix(raw: &str) -> String {
    let mut last_boundary = None;
    let mut stack = Vec::new();
    let mut in_string = false;
    let mut escaped = false;

    for (idx, ch) in raw.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
                if stack.last().copied() == Some(':') {
                    stack.pop();
                }
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => stack.push('}'),
            '[' => stack.push(']'),
            ':' => stack.push(':'),
            ',' => {
                while stack.last().copied() == Some(':') {
                    stack.pop();
                }
                last_boundary = Some(idx);
            }
            '}' | ']' => {
                while stack.last().copied() == Some(':') {
                    stack.pop();
                }
                if stack.last().copied() == Some(ch) {
                    stack.pop();
                    last_boundary = Some(idx + ch.len_utf8());
                }
            }
            _ => {}
        }
    }

    if stack.is_empty() && !in_string {
        return raw.to_string();
    }

    last_boundary
        .map(|idx| raw[..idx].trim_end_matches(',').trim().to_string())
        .unwrap_or_default()
}

fn truncate_for_log(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn extract_usage(value: &serde_json::Value) -> Option<&serde_json::Value> {
    value.get("usage").or_else(|| {
        value
            .get("choices")
            .and_then(|choices| choices.as_array())
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("usage"))
    })
}

fn extract_reasoning_text(value: &serde_json::Value) -> Option<&str> {
    value
        .get("reasoning_content")
        .or_else(|| value.get("reasoning"))
        .or_else(|| value.get("reasoning_text"))
        .or_else(|| value.get("thinking"))
        .and_then(|v| v.as_str())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedEmbeddedThinking {
    thinking: Option<String>,
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ParsedEmbeddedThinkingChunk {
    Thinking(String),
    Text(String),
}

#[derive(Default)]
struct EmbeddedThinkingStreamParser {
    buffer: String,
    buffering: bool,
    passthrough: bool,
}

impl EmbeddedThinkingStreamParser {
    fn push(&mut self, chunk: &str) -> Vec<ParsedEmbeddedThinkingChunk> {
        if self.passthrough {
            return vec![ParsedEmbeddedThinkingChunk::Text(chunk.to_string())];
        }

        self.buffer.push_str(chunk);
        if !self.buffering {
            if starts_like_embedded_thinking(&self.buffer) {
                self.buffering = true;
            } else if could_be_embedded_thinking_prefix(&self.buffer) {
                return Vec::new();
            } else {
                self.passthrough = true;
                return vec![ParsedEmbeddedThinkingChunk::Text(std::mem::take(
                    &mut self.buffer,
                ))];
            }
        }

        if !self.buffer.to_ascii_lowercase().contains("</think>") {
            return Vec::new();
        }

        let parsed = split_embedded_thinking(&self.buffer);
        self.buffer.clear();
        self.buffering = false;
        self.passthrough = true;
        parsed.into_chunks()
    }

    fn flush(&mut self) -> Vec<ParsedEmbeddedThinkingChunk> {
        if self.buffer.is_empty() {
            return Vec::new();
        }

        if self.buffering {
            let buffer = std::mem::take(&mut self.buffer);
            if buffer.to_ascii_lowercase().contains("</think>")
                || starts_like_untagged_reasoning(&buffer)
            {
                return split_embedded_thinking(&buffer).into_chunks();
            }
            return vec![ParsedEmbeddedThinkingChunk::Text(buffer)];
        }

        vec![ParsedEmbeddedThinkingChunk::Text(std::mem::take(
            &mut self.buffer,
        ))]
    }
}

impl ParsedEmbeddedThinking {
    fn into_chunks(self) -> Vec<ParsedEmbeddedThinkingChunk> {
        let mut chunks = Vec::new();
        if let Some(thinking) = self.thinking
            && !thinking.is_empty()
        {
            chunks.push(ParsedEmbeddedThinkingChunk::Thinking(thinking));
        }
        if !self.text.is_empty() {
            chunks.push(ParsedEmbeddedThinkingChunk::Text(self.text));
        }
        chunks
    }
}

fn split_embedded_thinking(content: &str) -> ParsedEmbeddedThinking {
    let lower = content.to_ascii_lowercase();
    let Some(close_start) = lower.find("</think>") else {
        if starts_like_untagged_reasoning(content) {
            return split_untagged_reasoning(content);
        }

        return ParsedEmbeddedThinking {
            thinking: None,
            text: content.to_string(),
        };
    };

    let close_end = close_start + "</think>".len();
    let thinking = clean_embedded_thinking(&content[..close_start]);
    let text = content[close_end..].trim_start().to_string();
    ParsedEmbeddedThinking {
        thinking: if thinking.is_empty() {
            None
        } else {
            Some(thinking)
        },
        text,
    }
}

fn clean_embedded_thinking(raw: &str) -> String {
    let mut value = raw.trim().to_string();
    let lower = value.to_ascii_lowercase();
    if let Some(open_start) = lower.find("<think>") {
        value = value[open_start + "<think>".len()..].trim().to_string();
    }

    for prefix in [
        "here's a thinking process:",
        "here is a thinking process:",
        "thinking process:",
    ] {
        if value.to_ascii_lowercase().starts_with(prefix) {
            value = value[prefix.len()..].trim_start().to_string();
            break;
        }
    }

    value
}

fn starts_like_embedded_thinking(content: &str) -> bool {
    let lower = content.trim_start().to_ascii_lowercase();
    lower.starts_with("<think")
        || lower.starts_with("here's a thinking process")
        || lower.starts_with("here is a thinking process")
        || lower.starts_with("thinking process")
        || starts_like_untagged_reasoning(content)
}

fn could_be_embedded_thinking_prefix(content: &str) -> bool {
    let lower = content.trim_start().to_ascii_lowercase();
    if lower.len() >= "here is a thinking process".len() {
        return false;
    }
    [
        "<think",
        "here's a thinking process",
        "here is a thinking process",
        "thinking process",
    ]
    .iter()
    .any(|marker| marker.starts_with(lower.as_str()))
}

fn starts_like_untagged_reasoning(content: &str) -> bool {
    let lower = content.trim_start().to_ascii_lowercase();
    [
        "the user is ",
        "the user asks ",
        "user asks ",
        "we need to ",
        "i need to ",
        "i will ",
        "let me ",
    ]
    .iter()
    .any(|marker| lower.starts_with(marker))
}

fn split_untagged_reasoning(content: &str) -> ParsedEmbeddedThinking {
    let trimmed = content.trim();
    let Some((thinking, answer)) = split_on_blank_line_before_final_answer(trimmed) else {
        return ParsedEmbeddedThinking {
            thinking: None,
            text: content.to_string(),
        };
    };

    ParsedEmbeddedThinking {
        thinking: Some(thinking.trim().to_string()),
        text: answer.trim_start().to_string(),
    }
}

fn split_on_blank_line_before_final_answer(content: &str) -> Option<(&str, &str)> {
    let split_markers = ["\r\n\r\n", "\n\n"];
    let mut candidates = Vec::new();
    for marker in split_markers {
        let mut offset = 0;
        while let Some(position) = content[offset..].find(marker) {
            let split_at = offset + position + marker.len();
            let answer = &content[split_at..];
            if looks_like_final_answer(answer) {
                candidates.push((offset + position, split_at));
            }
            offset = split_at;
        }
    }

    candidates
        .into_iter()
        .max_by_key(|(_, split_at)| *split_at)
        .map(|(marker_start, split_at)| (&content[..marker_start], &content[split_at..]))
}

fn looks_like_final_answer(value: &str) -> bool {
    let answer = value.trim_start();
    if answer.is_empty() || answer.len() > 600 {
        return false;
    }

    let lower = answer.to_ascii_lowercase();
    let starts_like_answer = [
        "hello", "hi", "the ", "a ", "an ", "yes", "no", "sure", "i can ", "i can't ", "here ",
    ]
    .iter()
    .any(|marker| lower.starts_with(marker));
    starts_like_answer && !starts_like_untagged_reasoning(answer)
}

async fn send_openai_request(
    client: &reqwest::Client,
    endpoint: &str,
    api_key: Option<&str>,
    custom_headers: &std::collections::BTreeMap<String, String>,
    request_body: &serde_json::Value,
) -> Result<reqwest::Response, ChatError> {
    let mut request = client.post(endpoint).json(request_body);
    if let Some(key) = api_key {
        request = request.bearer_auth(key);
    }
    request = apply_custom_headers(request, custom_headers)?;

    request
        .send()
        .await
        .map_err(|error| ChatError::Provider(error.to_string()))
}

async fn send_openai_request_with_retry(
    client: &reqwest::Client,
    endpoint: &str,
    api_key: Option<&str>,
    custom_headers: &std::collections::BTreeMap<String, String>,
    request_body: &serde_json::Value,
    max_retry_delay_ms: Option<u64>,
) -> Result<reqwest::Response, ChatError> {
    let mut last_error = None;
    for attempt in 0..=PROVIDER_MAX_RETRIES {
        match send_openai_request(client, endpoint, api_key, custom_headers, request_body).await {
            Ok(response)
                if is_retryable_status(response.status()) && attempt < PROVIDER_MAX_RETRIES =>
            {
                let status = response.status();
                let delay = retry_after_delay(response.headers())
                    .unwrap_or_else(|| retry_delay(max_retry_delay_ms));
                warn!(
                    status = %status,
                    retry = attempt + 1,
                    max_retries = PROVIDER_MAX_RETRIES,
                    ?delay,
                    "OpenAI request returned retryable status; retrying"
                );
                tokio::time::sleep(delay).await;
            }
            Ok(response) => return Ok(response),
            Err(error) if attempt < PROVIDER_MAX_RETRIES => {
                let delay = retry_delay(max_retry_delay_ms);
                warn!(
                    error = %error,
                    retry = attempt + 1,
                    max_retries = PROVIDER_MAX_RETRIES,
                    ?delay,
                    "OpenAI request failed; retrying"
                );
                last_error = Some(error);
                tokio::time::sleep(delay).await;
            }
            Err(error) => return Err(error),
        }
    }

    Err(last_error.unwrap_or_else(|| ChatError::Provider("OpenAI request failed".to_string())))
}

fn should_retry_openai_without_tools(
    status: reqwest::StatusCode,
    body: &str,
    request_body: &serde_json::Value,
) -> bool {
    if !status.is_client_error() || request_body.get("tools").is_none() {
        return false;
    }

    let body = body.to_ascii_lowercase();

    // Never retry for obviously-non-tool errors — they'd mask real issues
    if is_non_retryable_provider_rejection(&body) {
        return false;
    }

    // vLLM-specific patterns (proven in production)
    if body.contains("enable-auto-tool-choice")
        || body.contains("auto tool choice")
        || body.contains("tool_choice")
    {
        return true;
    }

    let unsupported_tool_patterns = [
        "unsupported parameter: 'tools'",
        "unsupported parameter: tools",
        "tools is not supported",
        "tools are not supported",
        "tools not supported",
        "does not support tools",
        "doesn't support tools",
        "model does not support tool",
        "model doesn't support tool",
        "tool calls are not supported",
        "tool calling is not supported",
        "tool use is not supported",
        "function calling is not available",
        "function calling is not supported",
        "function_call is not supported",
        "function calls are not supported",
    ];

    unsupported_tool_patterns
        .iter()
        .any(|pattern| body.contains(pattern))
}

fn should_retry_openai_without_reasoning(
    status: reqwest::StatusCode,
    body: &str,
    request_body: &serde_json::Value,
) -> bool {
    if !status.is_client_error() || request_body.get("reasoning_effort").is_none() {
        return false;
    }

    let body = body.to_ascii_lowercase();
    if is_non_retryable_provider_rejection(&body) {
        return false;
    }

    body.contains("reasoning_effort")
        && (body.contains("unsupported")
            || body.contains("unrecognized")
            || body.contains("unknown")
            || body.contains("not supported")
            || body.contains("invalid"))
}

fn is_non_retryable_provider_rejection(body: &str) -> bool {
    body.contains("api key")
        || body.contains("authentication")
        || body.contains("authorization")
        || body.contains("rate limit")
        || body.contains("rate_limit")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::model_config::ThinkingLevel;
    use crate::llm::types::GenerationOptions;

    #[test]
    fn retries_openai_compatible_auto_tool_choice_errors_without_tools() {
        let body = serde_json::json!({
            "model": "Qwen/Qwen3.6-35B-A3B",
            "messages": [],
            "stream": true,
            "tools": [{"type": "function"}]
        });

        assert!(should_retry_openai_without_tools(
            reqwest::StatusCode::BAD_REQUEST,
            r#"{"error":{"message":"\"auto\" tool choice requires --enable-auto-tool-choice and --tool-call-parser to be set"}}"#,
            &body,
        ));
    }

    #[test]
    fn does_not_retry_openai_errors_when_tools_were_not_sent() {
        let body = serde_json::json!({
            "model": "Qwen/Qwen3.6-35B-A3B",
            "messages": [],
            "stream": true
        });

        assert!(!should_retry_openai_without_tools(
            reqwest::StatusCode::BAD_REQUEST,
            r#"{"error":{"message":"bad request"}}"#,
            &body,
        ));
    }

    #[test]
    fn repairs_truncated_tool_call_arguments_by_closing_containers() {
        let parsed = parse_tool_call_arguments_lossy(
            r#"{"value":{"documents":[{"file_name":"knowledge-graph-sample.md","description":"Knowledge Graph Sample"},{"file_name":"Screenshot 2026-02-02 155344.png","description":"Screenshot image of..."}]}"#,
            "set_structured_output",
        )
        .expect("repair incomplete JSON");

        assert_eq!(
            parsed["value"]["documents"][0]["file_name"],
            "knowledge-graph-sample.md"
        );
        assert_eq!(
            parsed["value"]["documents"][1]["file_name"],
            "Screenshot 2026-02-02 155344.png"
        );
    }

    #[test]
    fn malformed_tool_call_arguments_returns_error() {
        let parsed =
            parse_tool_call_arguments_lossy(r#"["not", "object"]"#, "set_structured_output");

        assert!(parsed.is_err());
    }

    #[test]
    fn repairs_windows_paths_with_bare_backslashes_in_tool_arguments() {
        let parsed = parse_tool_call_arguments_lossy(
            r#"{"value":{"solution":"C:\Users\<user>\AppData\Local\Pongo\Logs contains the diagnostic files."}}"#,
            "set_structured_output",
        )
        .expect("repair invalid backslash escapes");

        assert_eq!(
            parsed["value"]["solution"],
            r#"C:\Users\<user>\AppData\Local\Pongo\Logs contains the diagnostic files."#
        );
    }

    #[test]
    fn splits_standard_embedded_think_tags() {
        let parsed = split_embedded_thinking("<think>Use known geography.</think>\n\nParis.");
        assert_eq!(parsed.thinking.as_deref(), Some("Use known geography."));
        assert_eq!(parsed.text, "Paris.");
    }

    #[test]
    fn splits_provider_thinking_process_content_without_open_tag() {
        let parsed = split_embedded_thinking(
            "Here's a thinking process:\n\n1. Identify the question.\n2. Verify Paris.\n</think>\n\nThe capital of France is Paris.",
        );
        assert_eq!(
            parsed.thinking.as_deref(),
            Some("1. Identify the question.\n2. Verify Paris.")
        );
        assert_eq!(parsed.text, "The capital of France is Paris.");
    }

    #[test]
    fn splits_qwen_plain_reasoning_without_think_tags() {
        let parsed = split_embedded_thinking(
            "The user is greeting me with \"hello dear\". This is a simple greeting and does not require any specific or skills. I should respond with a friendly greeting back. \n\nI will not call any tools. I will simply reply to the user.\n \n\nHello! How can I help you today? ",
        );

        assert_eq!(
            parsed.thinking.as_deref(),
            Some(
                "The user is greeting me with \"hello dear\". This is a simple greeting and does not require any specific or skills. I should respond with a friendly greeting back. \n\nI will not call any tools. I will simply reply to the user."
            )
        );
        assert_eq!(parsed.text, "Hello! How can I help you today?");
    }

    #[test]
    fn streaming_parser_buffers_embedded_thinking_until_close_tag() {
        let mut parser = EmbeddedThinkingStreamParser::default();
        assert!(parser.push("Here's a thinking ").is_empty());
        assert!(parser.push("process:\n\n1. Check fact.").is_empty());
        let chunks = parser.push("</think>\n\nParis.");

        assert_eq!(
            chunks,
            vec![
                ParsedEmbeddedThinkingChunk::Thinking("1. Check fact.".to_string()),
                ParsedEmbeddedThinkingChunk::Text("Paris.".to_string())
            ]
        );
        assert!(parser.flush().is_empty());
    }

    #[test]
    fn streaming_parser_splits_untagged_reasoning_on_flush() {
        let mut parser = EmbeddedThinkingStreamParser::default();
        assert!(
            parser
                .push("The user is greeting me with \"hello dear\". This is a simple greeting.\n\n")
                .is_empty()
        );
        assert!(
            parser
                .push("I will not call any tools. I will simply reply to the user.\n \n\n")
                .is_empty()
        );
        assert!(parser.push("Hello! How can I help you today? ").is_empty());

        assert_eq!(
            parser.flush(),
            vec![
                ParsedEmbeddedThinkingChunk::Thinking(
                    "The user is greeting me with \"hello dear\". This is a simple greeting.\n\nI will not call any tools. I will simply reply to the user."
                        .to_string()
                ),
                ParsedEmbeddedThinkingChunk::Text("Hello! How can I help you today?".to_string())
            ]
        );
    }

    #[test]
    fn streaming_parser_passes_through_normal_content() {
        let mut parser = EmbeddedThinkingStreamParser::default();
        assert_eq!(
            parser.push("The capital "),
            vec![ParsedEmbeddedThinkingChunk::Text(
                "The capital ".to_string()
            )]
        );
        assert_eq!(
            parser.push("is Paris."),
            vec![ParsedEmbeddedThinkingChunk::Text("is Paris.".to_string())]
        );
    }

    #[test]
    fn llm_stream_options_apply_to_openai_body_includes_penalties() {
        let generation = GenerationOptions {
            presence_penalty: Some(0.45),
            frequency_penalty: Some(0.12),
            max_tokens: Some(128),
            ..GenerationOptions::default()
        };
        let options = LlmStreamOptions::from(&generation);

        let mut body = serde_json::json!({});
        options.apply_to_openai_body(&mut body);

        assert_eq!(body["presence_penalty"], serde_json::json!(0.45));
        assert_eq!(body["frequency_penalty"], serde_json::json!(0.12));
        assert_eq!(body["max_tokens"], serde_json::json!(128));
    }

    #[test]
    fn extract_usage_reads_top_level_usage() {
        let response = serde_json::json!({
            "usage": {"prompt_tokens": 120},
            "choices": [{"usage": {"prompt_tokens": 80}}]
        });

        assert_eq!(
            extract_usage(&response),
            Some(&serde_json::json!({"prompt_tokens": 120}))
        );
    }

    #[test]
    fn extract_usage_reads_first_choice_usage() {
        let response = serde_json::json!({
            "choices": [{"usage": {"prompt_tokens": 120}}]
        });

        assert_eq!(
            extract_usage(&response),
            Some(&serde_json::json!({"prompt_tokens": 120}))
        );
    }

    #[test]
    fn extract_reasoning_text_prioritizes_reasoning_content() {
        let message = serde_json::json!({
            "reasoning_content": "primary",
            "reasoning": "secondary",
            "reasoning_text": "tertiary",
            "thinking": "fallback"
        });

        assert_eq!(extract_reasoning_text(&message), Some("primary"));
    }

    #[test]
    fn extract_reasoning_text_reads_reasoning_text() {
        let message = serde_json::json!({"reasoning_text": "provider reasoning"});

        assert_eq!(extract_reasoning_text(&message), Some("provider reasoning"));
    }

    #[test]
    fn thinking_level_medium_sets_reasoning_effort_medium() {
        let options = LlmStreamOptions {
            thinking_level: Some(ThinkingLevel::Medium),
            ..LlmStreamOptions::default()
        };
        let mut body = serde_json::json!({});

        options.apply_to_openai_body(&mut body);

        assert_eq!(body["reasoning_effort"], serde_json::json!("medium"));
    }

    #[test]
    fn thinking_level_none_omits_reasoning_effort() {
        let options = LlmStreamOptions {
            thinking_level: Some(ThinkingLevel::None),
            ..LlmStreamOptions::default()
        };
        let mut body = serde_json::json!({});

        options.apply_to_openai_body(&mut body);

        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn thinking_levels_map_to_openai_reasoning_effort_values() {
        assert_eq!(
            openai_reasoning_effort(Some(ThinkingLevel::Minimal)),
            Some("minimal")
        );
        assert_eq!(
            openai_reasoning_effort(Some(ThinkingLevel::Low)),
            Some("low")
        );
        assert_eq!(
            openai_reasoning_effort(Some(ThinkingLevel::Max)),
            Some("xhigh")
        );
    }

    #[test]
    fn gpt5_chat_payload_uses_max_completion_tokens_and_omits_unsupported_params() {
        let options = LlmStreamOptions {
            temperature: Some(0.2),
            top_p: Some(0.9),
            presence_penalty: Some(0.3),
            frequency_penalty: Some(0.1),
            max_tokens: Some(256),
            seed: Some(42),
            thinking_level: Some(ThinkingLevel::Medium),
            ..LlmStreamOptions::default()
        };
        let mut body = serde_json::json!({"model": "gpt-5.5", "messages": []});

        options.apply_to_openai_compatible_body(&mut body, true, false);
        apply_openai_chat_model_compatibility("gpt-5.5", &mut body);

        assert_eq!(body["max_completion_tokens"], serde_json::json!(256));
        assert!(body.get("max_tokens").is_none());
        assert!(body.get("temperature").is_none());
        assert!(body.get("top_p").is_none());
        assert!(body.get("presence_penalty").is_none());
        assert!(body.get("frequency_penalty").is_none());
        assert!(body.get("seed").is_none());
        assert_eq!(body["reasoning_effort"], serde_json::json!("medium"));
    }

    #[test]
    fn non_gpt5_chat_payload_keeps_openai_compatible_fields() {
        let options = LlmStreamOptions {
            temperature: Some(0.2),
            max_tokens: Some(128),
            thinking_level: Some(ThinkingLevel::Medium),
            ..LlmStreamOptions::default()
        };
        let mut body = serde_json::json!({"model": "gpt-4o", "messages": []});

        options.apply_to_openai_compatible_body(&mut body, true, false);
        apply_openai_chat_model_compatibility("gpt-4o", &mut body);

        assert_eq!(body["max_tokens"], serde_json::json!(128));
        assert_eq!(body["temperature"], serde_json::json!(0.2));
        assert_eq!(body["reasoning_effort"], serde_json::json!("medium"));
    }

    #[test]
    fn llm_stream_options_apply_to_openai_compatible_body_sets_cache_control() {
        let options = LlmStreamOptions {
            cache_retention: crate::llm::model_config::CacheRetention::Short,
            ..LlmStreamOptions::default()
        };
        let mut body = serde_json::json!({});

        options.apply_to_openai_compatible_body(&mut body, false, true);

        assert_eq!(
            body["cache_control"],
            serde_json::json!({"type": "ephemeral"})
        );
    }

    #[test]
    fn llm_stream_options_apply_to_openai_compatible_body_omits_cache_control_when_disabled() {
        let options = LlmStreamOptions::default();
        let mut body = serde_json::json!({});

        options.apply_to_openai_compatible_body(&mut body, false, false);

        assert!(body.get("cache_control").is_none());
    }

    #[test]
    fn openai_compatible_body_omits_reasoning_without_thinking_support() {
        let options = LlmStreamOptions {
            thinking_level: Some(ThinkingLevel::Medium),
            ..LlmStreamOptions::default()
        };
        let mut body = serde_json::json!({});

        options.apply_to_openai_compatible_body(&mut body, false, false);

        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn request_diagnostics_summarize_keys_and_tool_names() {
        let body = serde_json::json!({
            "stream": true,
            "model": "gpt-5.5",
            "tools": [
                {"type": "function", "function": {"name": "read_file", "parameters": {"type": "object"}}},
                {"type": "function", "function": {"parameters": {"type": "object"}}}
            ],
            "messages": []
        });

        assert_eq!(summarize_request_keys(&body), "messages,model,stream,tools");
        assert_eq!(
            summarize_tool_names(body.get("tools")),
            "read_file (+1 unnamed)"
        );
    }

    #[test]
    fn sse_line_buffer_keeps_json_split_across_chunks() {
        let mut buffer = String::new();

        let first = drain_complete_sse_lines(
            &mut buffer,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"read_"#,
        );
        let second = drain_complete_sse_lines(
            &mut buffer,
            r#"file","arguments":"{\"path\":"}}]}}]}
"#,
        );

        assert!(first.is_empty());
        assert_eq!(second.len(), 1);
        assert!(second[0].contains(r#""name":"read_file""#));
    }

    #[test]
    fn extracts_tool_name_from_alternate_fields() {
        let tool = serde_json::json!({
            "id": "call_1",
            "name": "read_file",
            "arguments": {"path": "/tmp"}
        });

        assert_eq!(extract_tool_call_name(&tool), Some("read_file"));
        assert_eq!(
            extract_tool_call_arguments_fragment(&tool),
            Some(r#"{"path":"/tmp"}"#.to_string())
        );
    }

    #[test]
    fn extract_tool_call_name_rejects_blank_values() {
        let tool = serde_json::json!({"function": {"name": "   "}});

        assert_eq!(extract_tool_call_name(&tool), None);
    }

    #[test]
    fn finish_reason_becomes_tool_calls_when_tool_calls_exist() {
        assert_eq!(
            finish_reason_for_tool_calls(Some("stop".to_string()), true),
            Some("tool_calls".to_string())
        );
    }

    #[test]
    fn finish_reason_preserves_stop_without_tool_calls() {
        assert_eq!(
            finish_reason_for_tool_calls(Some("stop".to_string()), false),
            Some("stop".to_string())
        );
    }

    #[test]
    fn terminal_stream_finish_reason_defaults_to_stop_without_provider_done() {
        assert_eq!(
            terminal_stream_finish_reason(None, false),
            Some("stop".to_string())
        );
    }

    #[test]
    fn terminal_stream_finish_reason_prefers_tool_calls_when_tools_exist() {
        assert_eq!(
            terminal_stream_finish_reason(None, true),
            Some("tool_calls".to_string())
        );
    }

    #[test]
    fn provider_network_error_text_is_user_visible_and_bounded() {
        let error = ChatError::Provider("connection reset by peer".to_string());

        assert_eq!(
            provider_network_error_text(&error),
            "[provider request failed after retry: provider error: connection reset by peer]"
        );
    }

    #[tokio::test]
    async fn request_network_failure_yields_error_after_retries() {
        let provider = OpenAiProvider::new();
        let config = ModelConfig::openai_compatible("http://127.0.0.1:1", "gpt-4o", None);
        let options = LlmStreamOptions {
            max_retry_delay_ms: 1,
            ..LlmStreamOptions::default()
        };
        let mut stream = provider.stream_with_mode(
            &config,
            vec![Message::new("user", "hello")],
            &options,
            None,
            true,
        );

        let first = tokio::time::timeout(std::time::Duration::from_secs(60), stream.next())
            .await
            .expect("network failure should resolve promptly")
            .expect("provider should emit one item");
        assert!(matches!(first, Err(ChatError::Provider(_))));
    }
    #[test]
    fn build_tool_call_chunk_rejects_empty_tool_name() {
        assert!(
            build_tool_call_chunk("tc1".to_string(), "   ".to_string(), "{}")
                .expect("valid empty args")
                .is_none()
        );
    }

    #[test]
    fn retries_on_unsupported_tools_parameter() {
        let body = serde_json::json!({
            "model": "gpt-5.5",
            "messages": [],
            "tools": [{"type": "function"}]
        });
        assert!(should_retry_openai_without_tools(
            reqwest::StatusCode::BAD_REQUEST,
            r#"{"error":{"message":"Unsupported parameter: 'tools' is not supported for this model."}}"#,
            &body,
        ));
    }

    #[test]
    fn retries_on_tool_not_supported_error() {
        let body = serde_json::json!({
            "model": "gpt-5.5",
            "messages": [],
            "tools": [{"type": "function"}]
        });
        assert!(should_retry_openai_without_tools(
            reqwest::StatusCode::BAD_REQUEST,
            r#"{"error":{"message":"tools is not supported"}}"#,
            &body,
        ));
    }

    #[test]
    fn retries_on_function_calling_not_supported() {
        let body = serde_json::json!({
            "model": "gpt-5.5",
            "messages": [],
            "tools": [{"type": "function"}]
        });
        assert!(should_retry_openai_without_tools(
            reqwest::StatusCode::BAD_REQUEST,
            r#"{"error":{"message":"function calling is not available for model gpt-5.5"}}"#,
            &body,
        ));
    }

    #[test]
    fn retries_on_unsupported_reasoning_effort_parameter() {
        let body = serde_json::json!({
            "model": "gpt-5.5",
            "messages": [],
            "reasoning_effort": "minimal"
        });
        assert!(should_retry_openai_without_reasoning(
            reqwest::StatusCode::BAD_REQUEST,
            r#"{"error":{"message":"Unsupported parameter: 'reasoning_effort' is not supported for this model."}}"#,
            &body,
        ));
    }

    #[test]
    fn does_not_retry_reasoning_error_without_reasoning_effort() {
        let body = serde_json::json!({
            "model": "gpt-4o",
            "messages": []
        });
        assert!(!should_retry_openai_without_reasoning(
            reqwest::StatusCode::BAD_REQUEST,
            r#"{"error":{"message":"Unsupported parameter: reasoning_effort"}}"#,
            &body,
        ));
    }

    #[test]
    fn does_not_retry_reasoning_auth_error() {
        let body = serde_json::json!({
            "model": "gpt-5.5",
            "messages": [],
            "reasoning_effort": "medium"
        });
        assert!(!should_retry_openai_without_reasoning(
            reqwest::StatusCode::UNAUTHORIZED,
            r#"{"error":{"message":"Invalid API key for reasoning_effort request"}}"#,
            &body,
        ));
    }

    #[test]
    fn does_not_retry_auth_error_with_tools() {
        let body = serde_json::json!({
            "model": "gpt-5.5",
            "messages": [],
            "tools": [{"type": "function"}]
        });
        assert!(!should_retry_openai_without_tools(
            reqwest::StatusCode::UNAUTHORIZED,
            r#"{"error":{"message":"Invalid API key"}}"#,
            &body,
        ));
    }

    #[test]
    fn does_not_retry_rate_limit_with_tools() {
        let body = serde_json::json!({
            "model": "gpt-5.5",
            "messages": [],
            "tools": [{"type": "function"}]
        });
        assert!(!should_retry_openai_without_tools(
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            r#"{"error":{"message":"Rate limit exceeded"}}"#,
            &body,
        ));
    }

    #[test]
    fn does_not_retry_invalid_tool_schema_error() {
        let body = serde_json::json!({
            "model": "gpt-5.5",
            "messages": [],
            "tools": [{"type": "function"}]
        });
        assert!(!should_retry_openai_without_tools(
            reqwest::StatusCode::BAD_REQUEST,
            r#"{"error":{"message":"Invalid tool schema: missing required property 'parameters'."}}"#,
            &body,
        ));
    }

    #[test]
    fn does_not_retry_invalid_tool_name_error() {
        let body = serde_json::json!({
            "model": "gpt-5.5",
            "messages": [],
            "tools": [{"type": "function"}]
        });
        assert!(!should_retry_openai_without_tools(
            reqwest::StatusCode::BAD_REQUEST,
            r#"{"error":{"message":"Invalid tool name: must match pattern ^[a-zA-Z0-9_-]+$."}}"#,
            &body,
        ));
    }

    #[test]
    fn does_not_retry_without_tools_in_body() {
        let body = serde_json::json!({
            "model": "gpt-5.5",
            "messages": [],
        });
        assert!(!should_retry_openai_without_tools(
            reqwest::StatusCode::BAD_REQUEST,
            r#"{"error":{"message":"Unsupported parameter: tools"}}"#,
            &body,
        ));
    }
}
