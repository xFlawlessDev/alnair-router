//! Request shaping for an openai-compatible upstream's Responses API.
//!
//! The Responses endpoint carries the same conversation as chat completions
//! under a different shape: `instructions` replaces the system message, `input`
//! is an item list rather than a message list, tools are declared flat, and the
//! output limit is `max_output_tokens`. `temperature` and `top_p` exist here
//! too and are forwarded. The sampling knobs this API does *not* define
//! (`presence_penalty`, `frequency_penalty`, `seed`, `stop`) are dropped rather
//! than sent, because the endpoint rejects unknown fields.

use serde_json::{Value, json};

use crate::model_config::{LlmStreamOptions, ModelConfig, ThinkingLevel};
use crate::types::{ContentPart, Message, MessageContent};

/// Builds the request body for `POST {base_url}/responses`.
pub(super) fn build_request_body(
    config: &ModelConfig,
    messages: &[Message],
    options: &LlmStreamOptions,
    tools: Option<&[Value]>,
    streaming: bool,
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
                // A replayed `function_call` item may omit `id`; the API accepts
                // `call_id` alone and assigning one here would break the pairing
                // with its `function_call_output`.
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
                    MessageContent::Parts(parts) => {
                        let content: Vec<Value> = parts
                            .iter()
                            .map(|part| match part {
                                ContentPart::Text(text) => json!({
                                    "type": "input_text",
                                    "text": text.text,
                                }),
                                ContentPart::ImageUrl(image) => json!({
                                    "type": "input_image",
                                    "image_url": image.image_url.url,
                                }),
                                ContentPart::Media(media) => media.raw.clone(),
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
        "stream": streaming,
    });

    if !instructions.is_empty() {
        body["instructions"] = Value::String(instructions.join("\n\n"));
    }
    let tools = convert_tools(tools);
    if let Some(tools) = tools {
        body["tools"] = tools;
        body["tool_choice"] = json!("auto");
    }
    if let Some(max_output_tokens) = options.max_tokens.filter(|max| *max > 0) {
        body["max_output_tokens"] = json!(max_output_tokens);
    }
    if let Some(temperature) = options.temperature {
        body["temperature"] = json!(temperature);
    }
    if let Some(top_p) = options.top_p {
        body["top_p"] = json!(top_p);
    }
    if config.supports_thinking
        && let Some(effort) = reasoning_effort(options.thinking_level)
    {
        body["reasoning"] = json!({ "effort": effort });
    }

    body
}

fn text_item(role: &str, kind: &str, text: String) -> Value {
    json!({ "role": role, "content": [{ "type": kind, "text": text }] })
}

/// Maps the router's effort tiers onto the Responses vocabulary.
///
/// Both wire APIs accept `minimal|low|medium|high`, but they disagree at the
/// top. Responses adds `xhigh` and `max` as two *distinct* levels, `max` being
/// the highest; Chat Completions stops at `xhigh`. The router's fifth tier is
/// `Max`, so it maps onto `max` here — sending `xhigh` would silently ask for
/// one tier less than the caller requested.
fn reasoning_effort(level: Option<ThinkingLevel>) -> Option<&'static str> {
    match level? {
        ThinkingLevel::None => None,
        ThinkingLevel::Minimal => Some("minimal"),
        ThinkingLevel::Low => Some("low"),
        ThinkingLevel::Medium => Some("medium"),
        ThinkingLevel::High => Some("high"),
        ThinkingLevel::Max => Some("max"),
    }
}

/// Flattens the OpenAI function-tool shape into the Responses shape, where
/// `name`, `description` and `parameters` sit at the top level.
///
/// Returns `None` when there is nothing to declare, so an empty array is not
/// sent — an upstream that rejects `tools` outright is then never handed one.
pub(super) fn convert_tools(tools: Option<&[Value]>) -> Option<Value> {
    let tools = tools.filter(|tools| !tools.is_empty())?;

    let converted: Vec<Value> = tools
        .iter()
        .filter_map(|tool| {
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
            }))
        })
        .collect();

    (!converted.is_empty()).then_some(Value::Array(converted))
}
