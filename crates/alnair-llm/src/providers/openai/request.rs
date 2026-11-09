//! Request dispatch, compatibility retries, and retry decisions.

use tracing::warn;

use super::chunks::truncate_for_log;
use crate::model_config::apply_custom_headers;
use crate::providers::common::{RetryBudget, is_retryable_status, retry_after_delay, retry_delay};
use crate::types::ChatError;

pub(super) fn apply_openai_chat_model_compatibility(model: &str, body: &mut serde_json::Value) {
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

pub(super) fn uses_gpt5_chat_parameter_shape(model: &str) -> bool {
    let model = model
        .rsplit(['/', ':'])
        .next()
        .unwrap_or(model)
        .to_ascii_lowercase();

    model == "gpt-5" || model.starts_with("gpt-5-") || model.starts_with("gpt-5.")
}

pub(super) fn finish_reason_for_tool_calls(
    reason: Option<String>,
    has_tool_calls: bool,
) -> Option<String> {
    if has_tool_calls {
        Some("tool_calls".to_string())
    } else {
        reason
    }
}

pub(super) fn terminal_stream_finish_reason(
    reason: Option<String>,
    has_tool_calls: bool,
) -> Option<String> {
    finish_reason_for_tool_calls(reason.or_else(|| Some("stop".to_string())), has_tool_calls)
}

#[cfg(test)]
pub(super) fn provider_network_error_text(error: &ChatError) -> String {
    format!(
        "[provider request failed after retry: {}]",
        truncate_for_log(&error.to_string(), 200)
    )
}

pub(super) fn log_openai_request_failure(
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

pub(super) fn summarize_request_keys(request_body: &serde_json::Value) -> String {
    let Some(object) = request_body.as_object() else {
        return "<non-object>".to_string();
    };

    let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
    keys.sort_unstable();
    keys.join(",")
}

pub(super) fn summarize_tool_names(tools: Option<&serde_json::Value>) -> String {
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

pub(super) async fn send_openai_request(
    client: &reqwest::Client,
    endpoint: &str,
    api_key: Option<&str>,
    custom_headers: &std::collections::BTreeMap<String, String>,
    request_body: &serde_json::Value,
) -> Result<reqwest::Response, ChatError> {
    let mut request = client.post(endpoint).json(request_body);
    if request_body.get("stream").and_then(|value| value.as_bool()) == Some(true) {
        request = request.header(reqwest::header::ACCEPT, "text/event-stream");
    }
    if let Some(key) = api_key {
        request = request.bearer_auth(key);
    }
    request = apply_custom_headers(request, custom_headers)?;

    request
        .send()
        .await
        .map_err(|error| ChatError::Provider(error.to_string()))
}

/// Sends a request, retrying transport failures and retryable statuses on
/// separate budgets.
///
/// The split matters for the router: one client request already fans out
/// across tiers and credentials, so a single number for both would multiply.
/// [`RetryBudget::status`] is the per-tier budget the router configures;
/// [`RetryBudget::transport`] stays small because a refused connection is
/// rarely transient.
pub(super) async fn send_openai_request_with_retry(
    client: &reqwest::Client,
    endpoint: &str,
    api_key: Option<&str>,
    custom_headers: &std::collections::BTreeMap<String, String>,
    request_body: &serde_json::Value,
    max_retry_delay_ms: u64,
    budget: RetryBudget,
) -> Result<reqwest::Response, ChatError> {
    let mut last_error = None;
    let mut status_retries = 0;
    let mut transport_retries = 0;
    for attempt in 0..budget.attempts() {
        match send_openai_request(client, endpoint, api_key, custom_headers, request_body).await {
            Ok(response)
                if is_retryable_status(response.status()) && status_retries < budget.status =>
            {
                status_retries += 1;
                let status = response.status();
                let delay = retry_after_delay(response.headers())
                    .unwrap_or_else(|| retry_delay(attempt, max_retry_delay_ms));
                warn!(
                    status = %status,
                    retry = status_retries,
                    max_status_retries = budget.status,
                    ?delay,
                    "OpenAI request returned retryable status; retrying"
                );
                tokio::time::sleep(delay).await;
            }
            Ok(response) => return Ok(response),
            Err(error) if transport_retries < budget.transport => {
                transport_retries += 1;
                let delay = retry_delay(attempt, max_retry_delay_ms);
                warn!(
                    error = %error,
                    retry = transport_retries,
                    max_transport_retries = budget.transport,
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

pub(super) fn should_retry_openai_without_tools(
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

pub(super) fn should_retry_openai_without_reasoning(
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

/// The Responses counterpart of [`should_retry_openai_without_reasoning`].
///
/// The Responses body nests the effort under `reasoning`, so the chat check
/// never matches it: it looks for a top-level `reasoning_effort` that this wire
/// format does not have. An upstream that implements the endpoint but not the
/// reasoning parameter would otherwise fail the whole turn.
pub(super) fn should_retry_responses_without_reasoning(
    status: reqwest::StatusCode,
    body: &str,
    request_body: &serde_json::Value,
) -> bool {
    if !status.is_client_error() || request_body.get("reasoning").is_none() {
        return false;
    }

    let body = body.to_ascii_lowercase();
    if is_non_retryable_provider_rejection(&body) {
        return false;
    }

    body.contains("reasoning")
        && (body.contains("unsupported")
            || body.contains("unrecognized")
            || body.contains("unknown")
            || body.contains("not supported")
            || body.contains("invalid"))
}

pub(super) fn is_non_retryable_provider_rejection(body: &str) -> bool {
    body.contains("api key")
        || body.contains("authentication")
        || body.contains("authorization")
        || body.contains("rate limit")
        || body.contains("rate_limit")
}
