//! Parsing the OpenAI wire spelling of `tool_choice` and `response_format` onto
//! the canonical hub enums.
//!
//! Both fields are accepted in their documented shape only. Anything else is a
//! `400` rather than a shrug: the entire point of the canonical hub is that a
//! requested feature either reaches the upstream or is refused explicitly.

use crate::error::{Error, Result};
use crate::upstream::chat_backend::{ResponseFormat, ToolChoice};

/// `"auto" | "none" | "required" | {"type":"function","function":{"name":…}}`.
///
/// `None` in, `None` out — an absent field means the upstream's own default,
/// not a forced `auto`.
pub(crate) fn parse_tool_choice(value: Option<&serde_json::Value>) -> Result<Option<ToolChoice>> {
    let Some(value) = value else {
        return Ok(None);
    };

    match value {
        serde_json::Value::String(choice) => match choice.as_str() {
            "auto" => Ok(Some(ToolChoice::Auto)),
            "none" => Ok(Some(ToolChoice::None)),
            "required" => Ok(Some(ToolChoice::Required)),
            other => Err(Error::BadRequest(format!(
                "unsupported tool_choice `{other}`; use `auto`, `none`, `required` \
                 or a function object"
            ))),
        },
        serde_json::Value::Object(object) => {
            let kind = object.get("type").and_then(serde_json::Value::as_str);
            if kind != Some("function") {
                return Err(Error::BadRequest(
                    "tool_choice object must have `type: \"function\"`".to_string(),
                ));
            }
            // Chat completions nests the name under `function`; the Responses
            // API declares tools flat, so its `tool_choice` is flat too. Both
            // are accepted because both are documented spellings of the same
            // request.
            let name = object
                .get("function")
                .and_then(|function| function.get("name"))
                .or_else(|| object.get("name"))
                .and_then(serde_json::Value::as_str)
                .filter(|name| !name.trim().is_empty())
                .ok_or_else(|| {
                    Error::BadRequest("tool_choice names a function without a `name`".to_string())
                })?;
            Ok(Some(ToolChoice::Named {
                name: name.to_string(),
            }))
        }
        _ => Err(Error::BadRequest(
            "tool_choice must be a string or a function object".to_string(),
        )),
    }
}

/// `{"type":"text"}`, `{"type":"json_object"}`, or `{"type":"json_schema",
/// "json_schema":{"name":…,"schema":{…},"strict":…}}`.
pub(crate) fn parse_response_format(
    value: Option<&serde_json::Value>,
) -> Result<Option<ResponseFormat>> {
    let Some(value) = value else {
        return Ok(None);
    };

    let object = value
        .as_object()
        .ok_or_else(|| Error::BadRequest("response_format must be a JSON object".to_string()))?;
    let kind = object.get("type").and_then(serde_json::Value::as_str);

    match kind {
        Some("text") => Ok(Some(ResponseFormat::Text)),
        Some("json_object") => Ok(Some(ResponseFormat::JsonObject)),
        Some("json_schema") => {
            let schema = object
                .get("json_schema")
                .and_then(serde_json::Value::as_object)
                .ok_or_else(|| {
                    Error::BadRequest(
                        "response_format of type `json_schema` requires a `json_schema` object"
                            .to_string(),
                    )
                })?;
            let name = schema
                .get("name")
                .and_then(serde_json::Value::as_str)
                .filter(|name| !name.trim().is_empty())
                .ok_or_else(|| {
                    Error::BadRequest(
                        "response_format.json_schema requires a non-empty `name`".to_string(),
                    )
                })?;
            let body = schema.get("schema").ok_or_else(|| {
                Error::BadRequest(
                    "response_format.json_schema requires a `schema` object".to_string(),
                )
            })?;
            if !body.is_object() {
                return Err(Error::BadRequest(
                    "response_format.json_schema.schema must be a JSON object".to_string(),
                ));
            }
            Ok(Some(ResponseFormat::JsonSchema {
                name: name.to_string(),
                schema: body.clone(),
                strict: schema
                    .get("strict")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
            }))
        }
        Some(other) => Err(Error::BadRequest(format!(
            "unsupported response_format type `{other}`; use `text`, `json_object` or `json_schema`"
        ))),
        None => Err(Error::BadRequest(
            "response_format requires a `type`".to_string(),
        )),
    }
}
