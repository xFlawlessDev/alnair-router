//! Request shaping for the Codex Responses dialect: body, identity headers,
//! and the retrying send.

use serde_json::{Value, json};

use crate::model_config::{LlmStreamOptions, ModelConfig, ThinkingLevel, apply_custom_headers};
use crate::providers::common::{
    RetryBudget, describe_non_json_body, is_retryable_status, retry_after_delay, retry_delay,
};
use crate::types::{ChatError, Message};

use super::{CODEX_OPENAI_BETA, CODEX_ORIGINATOR, CODEX_USER_AGENT};

/// The backend rejects a turn with no `max_output_tokens`, so a request that
/// carries none gets this placeholder instead.
const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 32_000;

/// Builds the request body.
///
/// The three backend contracts (see the module docs) are applied here: system
/// messages are hoisted into `instructions`, `store` is `false`, and
/// `include: ["reasoning.encrypted_content"]` is always requested. A
/// `thinking_level` becomes `reasoning.effort`, and `max` — a tier the public
/// API does not spell — maps onto `xhigh`.
pub(super) fn build_request_body(
    config: &ModelConfig,
    messages: &[Message],
    options: &LlmStreamOptions,
    tools: Option<&[Value]>,
) -> Value {
    let mut instructions: Vec<String> = Vec::new();
    let mut input: Vec<Value> = Vec::new();

    for message in messages {
        match message.role.as_str() {
            "system" | "developer" => {
                let text = message.content.as_text();
                if !text.trim().is_empty() {
                    instructions.push(text);
                }
            }
            "tool" => {
                let Some(call_id) = message.tool_call_id.as_deref() else {
                    continue;
                };
                input.push(json!({
                    "type": "function_call_output",
                    "call_id": call_id,
                    "output": message.content.as_text(),
                }));
            }
            "assistant" => {
                let text = message.content.as_text();
                if !text.is_empty() {
                    input.push(text_item("assistant", "output_text", text));
                }
                for call in message.tool_calls.iter().flatten() {
                    input.push(json!({
                        "type": "function_call",
                        "call_id": call.id,
                        "name": call.name,
                        "arguments": call.arguments,
                    }));
                }
            }
            _ => {
                if message.content.is_empty() {
                    continue;
                }
                input.push(match &message.content {
                    crate::types::MessageContent::Parts(parts) => {
                        let content: Vec<Value> = parts
                            .iter()
                            .map(|part| match part {
                                crate::types::ContentPart::Text(text) => json!({
                                    "type": "input_text",
                                    "text": text.text,
                                }),
                                crate::types::ContentPart::ImageUrl(image) => json!({
                                    "type": "input_image",
                                    "image_url": image.image_url.url,
                                }),
                                crate::types::ContentPart::Media(media) => media.raw.clone(),
                            })
                            .collect();
                        json!({ "role": "user", "content": content })
                    }
                    other => text_item("user", "input_text", other.as_text()),
                });
            }
        }
    }

    let mut body = json!({
        "model": config.model_id,
        "input": input,
        "tools": convert_tools(tools),
        // `auto` remains the default for a request that says nothing, but an
        // explicit choice must survive: writing these unconditionally is what
        // silently overrode the client's request before.
        "tool_choice": match &options.tool_choice {
            Some(choice) => tool_choice_to_codex(choice),
            None => Value::String("auto".to_string()),
        },
        "parallel_tool_calls": options.parallel_tool_calls.unwrap_or(true),
        // The backend is stateless, so `store` is forced off and a prompt-cache
        // hint would be ignored.
        "store": false,
        "include": include_list(options),
        "prompt_cache_key": Value::Null,
        "stream": true,
    });

    if let Some(search_options) = &options.web_search_options {
        let tools = body["tools"].as_array_mut().expect("tools array");
        if !tools.iter().any(|tool| {
            matches!(
                tool["type"].as_str(),
                Some("web_search" | "web_search_preview")
            )
        }) {
            let mut tool = search_options.as_object().cloned().unwrap_or_default();
            tool.insert("type".to_string(), json!("web_search"));
            tools.push(Value::Object(tool));
        }
    }

    if !instructions.is_empty() {
        body["instructions"] = Value::String(instructions.join("\n\n"));
    }
    if let Some(max_output_tokens) = options.max_tokens.filter(|max| *max > 0) {
        body["max_output_tokens"] = json!(max_output_tokens);
    } else {
        body["max_output_tokens"] = json!(DEFAULT_MAX_OUTPUT_TOKENS);
    }
    if let Some(effort) = reasoning_effort(options.thinking_level) {
        body["reasoning"] = json!({ "effort": effort, "summary": "auto" });
    }

    // A JSON `null` is not the same as an absent field to every backend; drop
    // the placeholder rather than sending it.
    if let Some(object) = body.as_object_mut() {
        object.retain(|_, value| !value.is_null());
    }
    body
}

fn text_item(role: &str, kind: &str, text: String) -> Value {
    json!({ "role": role, "content": [{ "type": kind, "text": text }] })
}

/// The backend always needs `reasoning.encrypted_content` so a multi-turn tool
/// loop can replay the reasoning it produced; any client opt-ins are added to
/// it rather than replacing it.
fn include_list(options: &LlmStreamOptions) -> Value {
    let mut includes = vec![Value::String("reasoning.encrypted_content".to_string())];
    for include in &options.include {
        if include != "reasoning.encrypted_content" {
            includes.push(Value::String(include.clone()));
        }
    }
    Value::Array(includes)
}

/// The Codex dialect spells a named choice the flat Responses way.
fn tool_choice_to_codex(choice: &crate::model_config::ToolChoice) -> Value {
    use crate::model_config::ToolChoice;
    match choice {
        ToolChoice::Auto => Value::String("auto".to_string()),
        ToolChoice::Required => Value::String("required".to_string()),
        ToolChoice::None => Value::String("none".to_string()),
        ToolChoice::Named { name } => json!({ "type": "function", "name": name }),
    }
}

/// Public model tiers are `minimal|low|medium|high`; Codex adds `xhigh` and
/// Alnair's `max` maps onto it.
fn reasoning_effort(level: Option<ThinkingLevel>) -> Option<&'static str> {
    match level? {
        ThinkingLevel::None => None,
        ThinkingLevel::Minimal => Some("minimal"),
        ThinkingLevel::Low => Some("low"),
        ThinkingLevel::Medium => Some("medium"),
        ThinkingLevel::High => Some("high"),
        ThinkingLevel::Max => Some("xhigh"),
    }
}

/// Flattens the OpenAI function-tool shape into the Responses shape, where
/// `name`, `description` and `parameters` sit at the top level.
pub(super) fn convert_tools(tools: Option<&[Value]>) -> Value {
    Value::Array(
        tools
            .unwrap_or_default()
            .iter()
            .filter_map(|tool| {
                if matches!(
                    tool["type"].as_str(),
                    Some("web_search" | "web_search_preview")
                ) {
                    return Some(tool.clone());
                }
                let function = tool.get("function").unwrap_or(tool);
                let name = function.get("name").and_then(Value::as_str)?;
                Some(json!({
                    "type": "function",
                    "name": name,
                    "description": function
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                    "parameters": function
                        .get("parameters")
                        .filter(|parameters| parameters.is_object())
                        .cloned()
                        .unwrap_or_else(|| json!({ "type": "object", "properties": {} })),
                    "strict": false,
                }))
            })
            .collect(),
    )
}

/// Sends the request, retrying retryable statuses and network errors.
#[allow(clippy::too_many_arguments)]
pub(super) async fn send(
    client: &reqwest::Client,
    endpoint: &str,
    bearer: &str,
    account_id: Option<&str>,
    custom_headers: &std::collections::BTreeMap<String, String>,
    body: &Value,
    options: &LlmStreamOptions,
) -> Result<reqwest::Response, ChatError> {
    let mut last_error = None;
    let mut status_retries = 0;
    let mut transport_retries = 0;
    let budget = RetryBudget::new(options.max_status_retries, options.max_transport_retries);
    for attempt in 0..budget.attempts() {
        let mut request = client
            .post(endpoint)
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .header(reqwest::header::USER_AGENT, CODEX_USER_AGENT)
            .header("originator", CODEX_ORIGINATOR)
            .header("openai-beta", CODEX_OPENAI_BETA)
            .bearer_auth(bearer)
            .json(body);
        // The account id is the JWT claim of the bearer; without the header the
        // backend cannot pick the account.
        if let Some(account_id) = account_id.filter(|id| !id.trim().is_empty()) {
            request = request.header("chatgpt-account-id", account_id);
        }
        request = apply_custom_headers(request, custom_headers)?;

        match request.send().await {
            Ok(response)
                if is_retryable_status(response.status()) && status_retries < budget.status =>
            {
                status_retries += 1;
                let delay = retry_after_delay(response.headers())
                    .unwrap_or_else(|| retry_delay(attempt, options.max_retry_delay_ms));
                tracing::warn!(
                    status = %response.status(),
                    retry = status_retries,
                    max_status_retries = budget.status,
                    ?delay,
                    "Codex request returned a retryable status; retrying"
                );
                tokio::time::sleep(delay).await;
            }
            Ok(response) => return Ok(response),
            Err(error) if transport_retries < budget.transport => {
                transport_retries += 1;
                let delay = retry_delay(attempt, options.max_retry_delay_ms);
                tracing::warn!(
                    error = %error,
                    retry = transport_retries,
                    max_transport_retries = budget.transport,
                    ?delay,
                    "Codex request failed; retrying"
                );
                last_error = Some(error);
                tokio::time::sleep(delay).await;
            }
            Err(error) => return Err(ChatError::Provider(error.to_string())),
        }
    }

    Err(ChatError::Provider(
        last_error
            .map(|error| error.to_string())
            .unwrap_or_else(|| "Codex request failed".to_string()),
    ))
}

/// Turns a failed response into the error the client sees.
pub(super) async fn failure(response: reqwest::Response) -> ChatError {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    ChatError::Provider(format!(
        "Codex API error {status}: {}",
        describe_non_json_body(&body)
    ))
}
