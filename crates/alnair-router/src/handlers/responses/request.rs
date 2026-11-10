//! `POST /v1/responses` request shape and its mapping onto the hub.

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::upstream::chat_backend::{
    self, GenerationOptions, MessagePart, ResponseFormat, RouterMessage, ToolCallSpec, ToolChoice,
};

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
    /// Reasoning configuration. Only `effort` is honoured, and it becomes the
    /// thinking level handed to the provider.
    #[serde(default)]
    pub reasoning: Option<ResponsesReasoning>,
    #[serde(default)]
    pub stream: bool,
    /// Function tools in the Responses shape: flat, with `name` and
    /// `parameters` at the top level rather than nested under `function`.
    #[serde(default)]
    pub tools: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    pub tool_choice: Option<serde_json::Value>,
    #[serde(default)]
    pub parallel_tool_calls: Option<bool>,
    /// `{"format": {"type": "text" | "json_object" | "json_schema", …}}` plus
    /// `verbosity`.
    #[serde(default)]
    pub text: Option<serde_json::Value>,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
    #[serde(default)]
    pub service_tier: Option<String>,
    #[serde(default)]
    pub store: Option<bool>,
    #[serde(default)]
    pub prompt_cache_key: Option<String>,
    /// Opt-ins such as `message.output_text.logprobs`.
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub top_logprobs: Option<u8>,
}

/// `reasoning` accepts the documented object; `summary` and `mode` are read
/// past because neither changes what the router forwards.
#[derive(Debug, Deserialize)]
pub struct ResponsesReasoning {
    /// `none|minimal|low|medium|high|xhigh|max`. Anything else is ignored, so an
    /// upstream-only value degrades to "no effort sent" rather than a 400 that
    /// would take down the whole turn.
    #[serde(default)]
    pub effort: Option<String>,
}

/// `input` accepts a bare string or the item list the Responses API defines.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ResponsesInput {
    Text(String),
    Items(Vec<serde_json::Value>),
}

/// Translates `instructions` plus `input` into provider-layer messages.
pub(super) fn build_messages(request: &ResponsesRequest) -> Result<Vec<RouterMessage>> {
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

    let parts: Vec<MessagePart> = match item.get("content") {
        None | Some(serde_json::Value::Null) => Vec::new(),
        Some(serde_json::Value::String(text)) => vec![MessagePart::Text(text.clone())],
        Some(serde_json::Value::Array(parts)) => parts
            .iter()
            .map(convert_part)
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect(),
        Some(_) => {
            return Err(Error::BadRequest(
                "message content must be a string or a part array".to_string(),
            ));
        }
    };

    Ok(chat_backend::message_parts(role, parts))
}

/// Converts one Responses content part, or `None` for a part that carries no
/// payload the router can forward.
fn convert_part(part: &serde_json::Value) -> Result<Option<MessagePart>> {
    let kind = part.get("type").and_then(|value| value.as_str());
    match kind {
        Some("input_text" | "output_text" | "text") => Ok(part
            .get("text")
            .and_then(|value| value.as_str())
            .map(|text| MessagePart::Text(text.to_string()))),
        Some("input_image" | "image_url") => Ok(part
            .get("image_url")
            .and_then(image_url)
            .map(MessagePart::ImageUrl)),
        Some(kind) if crate::protocol::openai::is_media_part_type(kind) => {
            crate::protocol::openai::media_part(part.clone()).map(Some)
        }
        // An unrecognized part is a client bug, not something to drop: the
        // model would otherwise answer a question it never saw.
        Some(other) => Err(Error::BadRequest(format!(
            "unsupported content part type `{other}`"
        ))),
        None => Err(Error::BadRequest(
            "content part requires a `type`".to_string(),
        )),
    }
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

/// Applies `tool_choice` to the declared tools, and reports the canonical
/// choice for the executor.
///
/// `none` drops the tools, which is how every dialect expresses it in practice
/// and what the upstream is least likely to reject. Everything else keeps them
/// and travels in [`GenerationOptions::tool_choice`], so a target that cannot
/// honour `required` or a named function is filtered out after routing rather
/// than silently downgraded to `auto` here.
pub(super) fn resolve_tools(
    request: &ResponsesRequest,
) -> Result<(Option<Vec<serde_json::Value>>, Option<ToolChoice>)> {
    let tools = normalize_tools(request.tools.clone());
    let choice = crate::protocol::openai::parse_tool_choice(request.tool_choice.as_ref())?;

    // `none` means "do not offer tools at all". Every other choice needs them
    // declared, and travels onward for the capability filter to judge.
    if matches!(choice, Some(ToolChoice::None)) {
        return Ok((None, choice));
    }

    Ok((tools, choice))
}

/// Builds the canonical options for this request.
///
/// Every field is carried, including the ones only the Responses endpoint
/// defines; the executor decides whether the target can honour them.
pub(super) fn generation_options(request: &ResponsesRequest) -> Result<GenerationOptions> {
    let (response_format, verbosity) = parse_text_config(request.text.as_ref())?;
    // The tools themselves are resolved separately; only the choice belongs in
    // the options, because it is what the capability matrix judges.
    let tool_choice = crate::protocol::openai::parse_tool_choice(request.tool_choice.as_ref())?;

    Ok(GenerationOptions {
        temperature: request.temperature,
        top_p: request.top_p,
        max_tokens: request.max_output_tokens,
        thinking_level: request
            .reasoning
            .as_ref()
            .and_then(|reasoning| reasoning.effort.clone()),
        tool_choice,
        parallel_tool_calls: request.parallel_tool_calls,
        response_format,
        metadata: request.metadata.clone(),
        service_tier: request.service_tier.clone(),
        store: request.store,
        prompt_cache_key: request.prompt_cache_key.clone(),
        verbosity,
        // `include` carries the logprob opt-in for this endpoint.
        logprobs: request
            .include
            .iter()
            .any(|include| include.contains("logprobs"))
            .then_some(true),
        top_logprobs: request.top_logprobs,
        include: request.include.clone(),
        ..Default::default()
    })
}

/// Reads `text.format` and `text.verbosity` out of the Responses `text` object.
fn parse_text_config(
    text: Option<&serde_json::Value>,
) -> Result<(Option<ResponseFormat>, Option<String>)> {
    let Some(text) = text else {
        return Ok((None, None));
    };
    let object = text
        .as_object()
        .ok_or_else(|| Error::BadRequest("text must be a JSON object".to_string()))?;

    let format = object
        .get("format")
        .map(|format| crate::protocol::openai::parse_response_format(Some(format)))
        .transpose()?
        .flatten();
    let verbosity = object
        .get("verbosity")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);

    Ok((format, verbosity))
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
