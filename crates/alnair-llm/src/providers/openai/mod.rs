use std::collections::HashMap;

use bytes::Bytes;
use futures::StreamExt;
use futures::stream::BoxStream;
use tracing::warn;

use crate::model_config::{LlmStreamOptions, ModelConfig, ThinkingLevel};
use crate::provider::LlmProvider;
use crate::providers::common::{calculate_token_costs, normalize_base_url};
use crate::providers::sse::{SseLineBuffer, parse_data_line};
use crate::types::{ChatError, LlmStreamChunk, Message};

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

            let openai_messages = crate::types::convert_messages_to_openai_format(&messages);
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
                options.max_retry_delay_ms,
                options.max_retries,
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
                        options.max_retry_delay_ms,
                        options.max_retries,
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
                        options.max_retry_delay_ms,
                        options.max_retries,
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
                        let reasoning_eval_count = reasoning_tokens(usage);
                        let rates = config.effective_cost_rates();
                        let costs = calculate_token_costs(
                            rates.as_ref(),
                            prompt_eval_count,
                            cached_prompt_eval_count,
                            None,
                            eval_count,
                            reasoning_eval_count,
                        );
                        yield Ok(LlmStreamChunk::Usage {
                            total_duration: None,
                            prompt_eval_count,
                            cached_prompt_eval_count,
                            eval_count,
                            reasoning_eval_count,
                            cost_input_usd: costs.input_usd,
                            cost_output_usd: costs.output_usd,
                            cost_reasoning_usd: costs.reasoning_usd,
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
                        let reasoning_eval_count = reasoning_tokens(usage);
                        let rates = config.effective_cost_rates();
                        let costs = calculate_token_costs(
                            rates.as_ref(),
                            prompt_eval_count,
                            cached_prompt_eval_count,
                            None,
                            eval_count,
                            reasoning_eval_count,
                        );
                        yield Ok(LlmStreamChunk::Usage {
                            total_duration: None,
                            prompt_eval_count,
                            cached_prompt_eval_count,
                            eval_count,
                            reasoning_eval_count,
                            cost_input_usd: costs.input_usd,
                            cost_output_usd: costs.output_usd,
                            cost_reasoning_usd: costs.reasoning_usd,
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

mod chunks;
mod request;

use chunks::*;
use request::*;

#[cfg(test)]
mod tests;
