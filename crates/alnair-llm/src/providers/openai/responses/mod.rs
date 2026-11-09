//! The Responses-API arm of an openai-compatible upstream.
//!
//! Selected per connection by `wire_api = "responses"`. The upstream is an
//! ordinary OpenAI-shaped endpoint authenticated with a bearer key, so this
//! shares the chat arm's retrying send and its HTML-content-type guard; what
//! differs is the endpoint, the request body and the event sequence.
//!
//! Unlike the Codex provider, which speaks a private dialect of the same API,
//! nothing here is OpenAI-specific: any endpoint that implements the Responses
//! shape (Azure OpenAI's `/openai/v1/responses`, a gateway that exposes it)
//! works through the same path.

mod request;
mod stream;

#[cfg(test)]
mod tests;

use crate::model_config::{LlmStreamOptions, ModelConfig};
use crate::providers::common::{
    RetryBudget, describe_non_json_body, is_html_content_type, normalize_base_url,
    response_content_type,
};
use crate::providers::openai::request::{
    log_openai_request_failure, send_openai_request_with_retry, should_retry_openai_without_tools,
    should_retry_responses_without_reasoning,
};
use crate::providers::openai::truncate_for_log;
use crate::providers::sse::{SseLineBuffer, parse_data_line};
use crate::types::{ChatError, LlmStreamChunk, Message};
use bytes::Bytes;
use futures::StreamExt;
use futures::stream::BoxStream;
use tracing::warn;

/// Streams a completion from `{base_url}/responses`.
///
/// `transform` is the same last-minute body hook the chat arm exposes; a
/// provider that wraps this arm may rewrite the request just before it is sent.
pub(super) fn stream<'a>(
    client: &'a reqwest::Client,
    config: &'a ModelConfig,
    messages: Vec<Message>,
    options: &'a LlmStreamOptions,
    tools: Option<Vec<serde_json::Value>>,
    use_streaming: bool,
    transform: fn(&mut serde_json::Value),
) -> BoxStream<'a, Result<LlmStreamChunk, ChatError>> {
    let base_url = config.base_url.clone();
    let api_key = config.api_key.clone();
    let custom_headers = config.custom_headers.clone();
    let rates = config.effective_cost_rates();

    Box::pin(async_stream::stream! {
        let endpoint = format!("{}/responses", normalize_base_url(&base_url));

        let mut request_body =
            request::build_request_body(config, &messages, options, tools.as_deref(), use_streaming);
        if request_body
            .get("input")
            .and_then(|input| input.as_array())
            .is_none_or(Vec::is_empty)
        {
            yield Err(ChatError::BadRequest("No valid messages to send".to_string()));
            return;
        }
        transform(&mut request_body);

        let mut response = match send_openai_request_with_retry(
            client,
            &endpoint,
            api_key.as_deref(),
            &custom_headers,
            &request_body,
            options.max_retry_delay_ms,
            RetryBudget::new(options.max_status_retries, options.max_transport_retries),
        )
        .await
        {
            Ok(response) => response,
            Err(error) => {
                warn!(error = %error, "OpenAI Responses request failed after retries");
                yield Err(error);
                return;
            }
        };

        let mut retried_without_tools = false;
        let mut retried_without_reasoning = false;
        let mut streamed_error: Option<ChatError> = None;
        loop {
            if response.status().is_success() {
                break;
            }

            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            log_openai_request_failure(status, &body, &request_body, retried_without_tools);

            if should_retry_responses_without_reasoning(status, &body, &request_body) {
                warn!(
                    status = %status,
                    error = %truncate_for_log(&body, 2000),
                    "Responses endpoint rejected the reasoning parameter; retrying without it"
                );
                if let Some(object) = request_body.as_object_mut() {
                    object.remove("reasoning");
                    retried_without_reasoning = true;
                }
                response = match send_openai_request_with_retry(
                    client,
                    &endpoint,
                    api_key.as_deref(),
                    &custom_headers,
                    &request_body,
                    options.max_retry_delay_ms,
                    RetryBudget::new(options.max_status_retries, options.max_transport_retries),
                )
                .await
                {
                    Ok(response) => response,
                    Err(error) => {
                        warn!(error = %error, "Responses retry without reasoning failed");
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
                    "Responses endpoint rejected automatic tool choice; retrying without tools"
                );
                if let Some(object) = request_body.as_object_mut() {
                    object.remove("tools");
                    object.remove("tool_choice");
                    retried_without_tools = true;
                }
                response = match send_openai_request_with_retry(
                    client,
                    &endpoint,
                    api_key.as_deref(),
                    &custom_headers,
                    &request_body,
                    options.max_retry_delay_ms,
                    RetryBudget::new(options.max_status_retries, options.max_transport_retries),
                )
                .await
                {
                    Ok(response) => response,
                    Err(error) => {
                        warn!(error = %error, "Responses retry without tools failed");
                        yield Err(error);
                        return;
                    }
                };
                continue;
            }

            if retried_without_tools {
                // Already retried without tools — give up gracefully so the
                // agent can carry on without tool calls on the next turn.
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
                    "Responses retry without the reasoning parameter still failed"
                );
            }

            yield Err(ChatError::Provider(format!("OpenAI Responses API error {status}: {body}")));
            return;
        }

        // A relay may report a failure inside a `200` body. Chat completions
        // simply parse to nothing; here the event carries a message, so surface
        // it rather than ending the turn empty.
        if let Some(error) = streamed_error {
            yield Err(error);
            return;
        }

        if !use_streaming {
            let text = response.text().await.unwrap_or_default();
            match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(json) => {
                    for chunk in stream::completion_chunks(&json, rates.as_ref()) {
                        yield Ok(chunk);
                    }
                }
                Err(_) => {
                    warn!(
                        response_preview = %truncate_for_log(&text, 500),
                        "non-streaming Responses body is not valid JSON"
                    );
                    yield Err(ChatError::Provider(format!(
                        "the Responses endpoint returned a non-JSON response: {}",
                        describe_non_json_body(&text)
                    )));
                }
            }
            return;
        }

        // A 200 carrying an HTML page is not an event stream; without this
        // guard it parses to zero events and the turn ends empty.
        if let Some(content_type) = response_content_type(&response)
            && is_html_content_type(&content_type)
        {
            let body = response.text().await.unwrap_or_default();
            yield Err(ChatError::Provider(format!(
                "the Responses endpoint returned {content_type} instead of an event stream: {}",
                describe_non_json_body(&body)
            )));
            return;
        }

        let mut bytes_stream = response.bytes_stream();
        let mut sse_line_buffer = SseLineBuffer::default();
        let mut pending_usage: Option<LlmStreamChunk> = None;
        let mut terminal: Option<Option<String>> = None;

        'stream: loop {
            let chunk = match bytes_stream.next().await {
                Some(chunk) => chunk,
                None => break,
            };
            let bytes: Bytes = match chunk {
                Ok(bytes) => bytes,
                Err(error) => {
                    warn!(error = %error, "Responses SSE stream disconnected");
                    yield Err(ChatError::Provider(error.to_string()));
                    return;
                }
            };

            for line in sse_line_buffer.push(&bytes) {
                let Some(data) = parse_data_line(&line) else {
                    continue;
                };
                if data == "[DONE]" {
                    terminal = Some(None);
                    break 'stream;
                }

                let Ok(event) = serde_json::from_str::<serde_json::Value>(data) else {
                    // A malformed frame is skipped: the next terminal event still
                    // closes the turn, so one bad line must not lose it.
                    warn!(payload = %truncate_for_log(data, 500), "Responses stream returned invalid JSON");
                    continue;
                };

                // An error event closes the stream; remember it and let the
                // flush below report it, so buffered text is not lost with it.
                if let Some(message) = error_event_message(&event) {
                    warn!(error = %truncate_for_log(&message, 2000), "Responses stream reported an error");
                    streamed_error.get_or_insert(ChatError::Provider(format!(
                        "OpenAI Responses API error: {message}"
                    )));
                    terminal = Some(None);
                    break 'stream;
                }

                match stream::handle_event(&event, rates.as_ref()) {
                    Ok(Some(chunk)) => yield Ok(chunk),
                    Ok(None) => {}
                    Err(error) => {
                        yield Err(error);
                        return;
                    }
                }

                if stream::is_terminal(&event) {
                    let response = event.get("response").unwrap_or(&event);
                    pending_usage = stream::response_usage(response, rates.as_ref());
                    terminal = Some(stream::terminal_reason_for_event(&event));
                    break 'stream;
                }
            }
        }

        if let Some(usage) = pending_usage {
            yield Ok(usage);
        }
        if let Some(error) = streamed_error {
            yield Err(error);
            return;
        }
        // Only a turn that reached a terminal event closes; a socket that dies
        // without one is a truncated answer, and ending it as `Done` would let
        // the router serve the fragment as if it were complete.
        if let Some(reason) = terminal {
            yield Ok(LlmStreamChunk::Done(reason));
        } else {
            yield Err(ChatError::Provider(
                "the Responses stream ended before a terminal event".to_string(),
            ));
        }
    })
}

/// The message of an error frame: a `type: error` event or a top-level
/// `{"error": {...}}` body a relay sent with a `200`.
fn error_event_message(event: &serde_json::Value) -> Option<String> {
    match event.get("type").and_then(|value| value.as_str()) {
        Some("error") => stream::response_error_message(event),
        Some("response.failed") => None,
        _ if event.get("error").is_some() => stream::response_error_message(event),
        _ => None,
    }
}
