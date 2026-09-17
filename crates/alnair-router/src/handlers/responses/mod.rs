//! `POST /v1/responses` — OpenAI Responses API shape over the shared executor.
//!
//! The request is translated into the same router messages the chat handler
//! uses, so combos, fallback, token saving and usage accounting all apply. The
//! reply is either one JSON object or the Responses SSE event sequence, chosen
//! by `stream`.

mod stream;

use axum::Extension;
use axum::Json;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use crate::db::repos::usage::NewUsageRecord;
use crate::error::{Error, Result};
use crate::handlers::shared::tier_price;
use crate::middleware::AuthenticatedKey;
use crate::state::AppState;
use crate::upstream::chat_backend::{self, GenerationOptions, RouterMessage, ToolCallSpec};

/// Responses API request. `input` is either a plain string or an item list.
#[derive(Debug, Deserialize)]
pub struct ResponsesRequest {
    pub model: String,
    pub input: ResponsesInput,
    #[serde(default)]
    pub instructions: Option<String>,
    #[serde(default)]
    pub max_output_tokens: Option<i32>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub top_p: Option<f64>,
    #[serde(default)]
    pub stream: bool,
    /// Function tools in the Responses shape: flat, with `name` and
    /// `parameters` at the top level rather than nested under `function`.
    #[serde(default)]
    pub tools: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    pub tool_choice: Option<serde_json::Value>,
}

/// `input` accepts a bare string or the item list the Responses API defines.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ResponsesInput {
    Text(String),
    Items(Vec<serde_json::Value>),
}

#[derive(Debug, Serialize)]
pub struct ResponsesResponse {
    pub id: String,
    pub object: &'static str,
    pub created_at: i64,
    pub model: String,
    pub status: &'static str,
    pub output: Vec<ResponsesOutput>,
    pub usage: Option<ResponsesUsage>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponsesOutput {
    Message {
        id: String,
        role: &'static str,
        status: &'static str,
        content: Vec<ResponsesContent>,
    },
    FunctionCall {
        id: String,
        call_id: String,
        name: String,
        arguments: String,
    },
    Reasoning {
        id: String,
        summary: Vec<ResponsesContent>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct ResponsesContent {
    #[serde(rename = "type")]
    pub content_type: &'static str,
    pub text: String,
}

#[derive(Debug, Serialize)]
pub struct ResponsesUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

/// Handles a Responses API request, streaming or not based on `stream`.
pub async fn responses(
    State(state): State<AppState>,
    Extension(key): Extension<Option<AuthenticatedKey>>,
    Json(request): Json<ResponsesRequest>,
) -> Result<Response> {
    let api_key_id = key.as_ref().map(|auth| auth.key.id.clone());
    if let Some(auth) = &key {
        auth.policy.ensure_model(&request.model)?;
    }

    let messages = build_messages(&request)?;
    let (messages, saver_savings) =
        crate::token_saver::apply(&state.token_saver_settings(), messages, &request.model).await;
    let tools = resolve_tools(&request)?;

    let options = GenerationOptions {
        temperature: request.temperature,
        top_p: request.top_p,
        max_tokens: request.max_output_tokens,
        ..Default::default()
    };

    let resolver = state.resolver().await?;
    let targets = resolver.resolve(&request.model)?;

    let executed = state
        .executor
        .stream(&targets, messages, tools, Some(&options), request.stream)
        .await?;

    if request.stream {
        return Ok(stream::responses_stream(
            state,
            api_key_id,
            request.model.clone(),
            saver_savings,
            executed,
        )
        .await);
    }

    complete_response(state, api_key_id, request.model, saver_savings, executed).await
}

/// Non-streaming path: drain the stream, then emit a single JSON body.
async fn complete_response(
    state: AppState,
    api_key_id: Option<String>,
    requested_model: String,
    saver_savings: crate::token_saver::Savings,
    executed: crate::upstream::ExecutedStream,
) -> Result<Response> {
    let target = executed.target.clone();
    let latency_ms = executed.latency_ms;
    let attempt_count = executed.attempts.len();

    let completion = chat_backend::collect(executed.stream).await?;
    let usage = completion.usage.unwrap_or_default();
    let savings_totals =
        saver_savings.finalize(usage.completion_tokens, tier_price(&state, &target).await);

    state.metrics.record_request(latency_ms);
    state.metrics.record_usage(
        usage.prompt_tokens,
        usage.completion_tokens,
        usage.cached_tokens,
        usage.cost_usd,
    );
    state.metrics.record_token_totals(savings_totals);
    state.telemetry.record_usage(
        &target.connection_id,
        usage.prompt_tokens,
        usage.completion_tokens,
    );

    state
        .usage()
        .record(
            NewUsageRecord {
                api_key_id,
                requested_model: requested_model.clone(),
                resolved_provider: Some(target.provider_type.clone()),
                resolved_model: Some(target.model.clone()),
                connection_name: Some(target.connection_name.clone()),
                attempt: attempt_count,
                status: "ok".to_string(),
                prompt_tokens: usage.prompt_tokens,
                completion_tokens: usage.completion_tokens,
                cached_tokens: usage.cached_tokens,
                reasoning_tokens: usage.reasoning_tokens,
                cost_usd: usage.cost_usd,
                cost_input_usd: usage.cost_input_usd,
                cost_output_usd: usage.cost_output_usd,
                cost_reasoning_usd: usage.cost_reasoning_usd,
                latency_ms,
                ..Default::default()
            }
            .with_savings(savings_totals),
        )
        .await?;

    let response = completion_payload(&requested_model, &completion, "completed");

    Ok((
        crate::handlers::shared::router_headers(&target, attempt_count),
        Json(response),
    )
        .into_response())
}

/// Builds the response object for a finished completion.
fn completion_payload(
    model: &str,
    completion: &chat_backend::CompletionResponse,
    status: &'static str,
) -> ResponsesResponse {
    let usage = completion.usage.unwrap_or_default();
    let mut output = Vec::new();

    if !completion.content.is_empty() || completion.tool_calls.is_empty() {
        output.push(ResponsesOutput::Message {
            id: message_id(),
            role: "assistant",
            status: "completed",
            content: vec![ResponsesContent {
                content_type: "output_text",
                text: completion.content.clone(),
            }],
        });
    }
    for call in &completion.tool_calls {
        output.push(function_call_output(call));
    }

    ResponsesResponse {
        id: response_id(),
        object: "response",
        created_at: chrono::Utc::now().timestamp(),
        model: model.to_string(),
        status,
        output,
        usage: Some(ResponsesUsage {
            input_tokens: usage.prompt_tokens,
            output_tokens: usage.completion_tokens,
            total_tokens: usage.prompt_tokens + usage.completion_tokens,
        }),
    }
}

/// Wraps one tool call in the Responses output item shape.
fn function_call_output(call: &ToolCallSpec) -> ResponsesOutput {
    ResponsesOutput::FunctionCall {
        id: format!("fc_{}", uuid::Uuid::new_v4().simple()),
        call_id: call.id.clone(),
        name: call.name.clone(),
        arguments: call.arguments.clone(),
    }
}

pub(super) fn response_id() -> String {
    format!("resp_{}", uuid::Uuid::new_v4().simple())
}

pub(super) fn message_id() -> String {
    format!("msg_{}", uuid::Uuid::new_v4().simple())
}

/// Translates `instructions` plus `input` into provider-layer messages.
fn build_messages(request: &ResponsesRequest) -> Result<Vec<RouterMessage>> {
    let mut messages = Vec::new();

    if let Some(instructions) = &request.instructions
        && !instructions.trim().is_empty()
    {
        messages.push(chat_backend::message_text("system", instructions.clone()));
    }

    match &request.input {
        ResponsesInput::Text(text) => {
            if text.trim().is_empty() {
                return Err(Error::BadRequest("input must not be empty".to_string()));
            }
            messages.push(chat_backend::message_text("user", text.clone()));
        }
        ResponsesInput::Items(items) => convert_items(items, &mut messages)?,
    }

    if messages.is_empty() {
        return Err(Error::BadRequest("input must not be empty".to_string()));
    }

    Ok(messages)
}

/// Converts the Responses item list, pairing tool calls with their results.
///
/// A Responses tool turn is a `function_call` item followed by a
/// `function_call_output` item rather than an OpenAI message, so consecutive
/// calls are grouped into one assistant message the provider layer understands.
fn convert_items(items: &[serde_json::Value], out: &mut Vec<RouterMessage>) -> Result<()> {
    let mut pending_calls: Vec<ToolCallSpec> = Vec::new();

    for item in items {
        match item.get("type").and_then(|value| value.as_str()) {
            Some("function_call") => {
                let call_id = item
                    .get("call_id")
                    .or_else(|| item.get("id"))
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| {
                        Error::BadRequest("function_call requires call_id".to_string())
                    })?;
                let name = item
                    .get("name")
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| Error::BadRequest("function_call requires name".to_string()))?;

                pending_calls.push(ToolCallSpec {
                    id: call_id.to_string(),
                    name: name.to_string(),
                    arguments: arguments_text(item.get("arguments")),
                });
            }
            Some("function_call_output") => {
                flush_calls(&mut pending_calls, out);
                let call_id = item
                    .get("call_id")
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| {
                        Error::BadRequest("function_call_output requires call_id".to_string())
                    })?;
                out.push(chat_backend::message_tool_result(
                    output_text(item.get("output")),
                    call_id.to_string(),
                ));
            }
            _ => {
                flush_calls(&mut pending_calls, out);
                out.push(convert_message(item)?);
            }
        }
    }

    flush_calls(&mut pending_calls, out);
    Ok(())
}

/// Emits the buffered tool calls as one assistant message.
fn flush_calls(pending: &mut Vec<ToolCallSpec>, out: &mut Vec<RouterMessage>) {
    if pending.is_empty() {
        return;
    }
    out.push(chat_backend::message_assistant_tool_calls(
        String::new(),
        std::mem::take(pending),
    ));
}

/// Converts one `message` item, accepting both plain text and content parts.
fn convert_message(item: &serde_json::Value) -> Result<RouterMessage> {
    let role = item
        .get("role")
        .and_then(|value| value.as_str())
        .ok_or_else(|| Error::BadRequest("input item requires role or type".to_string()))?;
    // The Responses API names the system role `developer`.
    let role = if role == "developer" { "system" } else { role };

    let parts: Vec<chat_backend::MessagePart> = match item.get("content") {
        None | Some(serde_json::Value::Null) => Vec::new(),
        Some(serde_json::Value::String(text)) => {
            vec![chat_backend::MessagePart::Text(text.clone())]
        }
        Some(serde_json::Value::Array(parts)) => parts
            .iter()
            .filter_map(
                |part| match part.get("type").and_then(|value| value.as_str()) {
                    Some("input_text" | "output_text" | "text") => part
                        .get("text")
                        .and_then(|value| value.as_str())
                        .map(|text| chat_backend::MessagePart::Text(text.to_string())),
                    Some("input_image" | "image_url") => part
                        .get("image_url")
                        .and_then(image_url)
                        .map(chat_backend::MessagePart::ImageUrl),
                    _ => None,
                },
            )
            .collect(),
        Some(_) => {
            return Err(Error::BadRequest(
                "message content must be a string or a part array".to_string(),
            ));
        }
    };

    Ok(chat_backend::message_parts(role, parts))
}

/// Reads an image URL from either wire shape.
fn image_url(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(url) => Some(url.clone()),
        serde_json::Value::Object(object) => object
            .get("url")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        _ => None,
    }
}

/// Renders a tool argument field, which is a string on the way out and may be
/// an object when a client echoes the parsed call back.
fn arguments_text(value: Option<&serde_json::Value>) -> String {
    match value {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
        None => "{}".to_string(),
    }
}

/// Renders a tool result, which may be a string or structured output.
fn output_text(value: Option<&serde_json::Value>) -> String {
    match value {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

/// Applies `tool_choice` to the declared tools.
///
/// The provider layer exposes a single automatic tool mode, so `none` is
/// honoured by dropping the tools while `required` and named-function choices
/// are rejected rather than silently downgraded to `auto`.
fn resolve_tools(request: &ResponsesRequest) -> Result<Option<Vec<serde_json::Value>>> {
    let tools = normalize_tools(request.tools.clone());

    match request.tool_choice.as_ref() {
        None => Ok(tools),
        Some(serde_json::Value::String(choice)) => match choice.as_str() {
            "auto" => Ok(tools),
            "none" => Ok(None),
            "required" => Err(Error::BadRequest(
                "tool_choice 'required' is not supported; use 'auto' or 'none'".to_string(),
            )),
            other => Err(Error::BadRequest(format!(
                "unsupported tool_choice '{other}'; use 'auto' or 'none'"
            ))),
        },
        Some(serde_json::Value::Object(object))
            if object.get("type").and_then(|value| value.as_str()) == Some("function") =>
        {
            Err(Error::BadRequest(
                "naming a function in tool_choice is not supported; use 'auto' or 'none'"
                    .to_string(),
            ))
        }
        Some(_) => Err(Error::BadRequest(
            "tool_choice must be 'auto' or 'none'".to_string(),
        )),
    }
}

/// Rewrites Responses function tools into the nested shape providers expect.
///
/// Responses declares tools flat (`name`, `parameters` at the top level); the
/// chat-completions wire format the provider layer speaks nests them under
/// `function`. Already-nested tools pass through untouched.
pub(super) fn normalize_tools(
    tools: Option<Vec<serde_json::Value>>,
) -> Option<Vec<serde_json::Value>> {
    let tools = tools?;
    if tools.is_empty() {
        return None;
    }

    let normalized = tools
        .into_iter()
        .map(|tool| {
            let is_function = tool.get("type").and_then(|value| value.as_str()) == Some("function");
            if is_function && tool.get("function").is_none() {
                let mut function = serde_json::Map::new();
                for key in ["name", "description", "parameters"] {
                    if let Some(value) = tool.get(key) {
                        function.insert(key.to_string(), value.clone());
                    }
                }
                return serde_json::json!({ "type": "function", "function": function });
            }
            tool
        })
        .collect();

    Some(normalized)
}

#[cfg(test)]
mod tests;
