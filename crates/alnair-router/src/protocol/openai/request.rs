//! OpenAI `POST /v1/chat/completions` request body and its mapping onto the hub.

use serde::Deserialize;

use super::options::{parse_response_format, parse_tool_choice};
use crate::error::{Error, Result};
use crate::upstream::chat_backend::{
    self, GenerationOptions, MessagePart, RouterMessage, ToolCallSpec,
};

/// OpenAI `POST /v1/chat/completions` request body.
#[derive(Debug, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<OpenAiMessage>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub top_p: Option<f64>,
    #[serde(default)]
    pub max_tokens: Option<i32>,
    #[serde(default)]
    pub max_completion_tokens: Option<i32>,
    #[serde(default)]
    pub stop: Option<StopSequence>,
    #[serde(default)]
    pub seed: Option<i64>,
    #[serde(default)]
    pub presence_penalty: Option<f64>,
    #[serde(default)]
    pub frequency_penalty: Option<f64>,
    #[serde(default)]
    pub tools: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    pub stream_options: Option<StreamOptions>,
    /// OpenAI reasoning effort, e.g. `low`, `medium`, `high`.
    #[serde(default)]
    pub reasoning_effort: Option<String>,
    /// OpenAI built-in web search. Replayed verbatim to an OpenAI-compatible
    /// upstream; validated here so a malformed object fails fast instead of
    /// reaching a provider that would answer without searching.
    #[serde(default)]
    pub web_search_options: Option<serde_json::Value>,
    /// `auto`, `none`, `required`, or `{"type":"function","function":{"name":…}}`.
    #[serde(default)]
    pub tool_choice: Option<serde_json::Value>,
    #[serde(default)]
    pub parallel_tool_calls: Option<bool>,
    /// `{"type":"text"}`, `{"type":"json_object"}` or `{"type":"json_schema",…}`.
    #[serde(default)]
    pub response_format: Option<serde_json::Value>,
    /// Vendor-specific sampler knob; forwarded optimistically (see
    /// [`GenerationOptions::top_k`]).
    #[serde(default)]
    pub top_k: Option<i32>,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub service_tier: Option<String>,
    #[serde(default)]
    pub store: Option<bool>,
    #[serde(default)]
    pub prompt_cache_key: Option<String>,
    #[serde(default)]
    pub verbosity: Option<String>,
    #[serde(default)]
    pub logprobs: Option<bool>,
    #[serde(default)]
    pub top_logprobs: Option<u8>,
}

/// `stream_options` on a streaming request.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct StreamOptions {
    /// Ask for a final SSE chunk carrying token usage, as OpenAI does.
    #[serde(default)]
    pub include_usage: bool,
}

/// `stop` accepts either a string or an array of strings.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum StopSequence {
    One(String),
    Many(Vec<String>),
}

impl StopSequence {
    fn into_vec(self) -> Vec<String> {
        match self {
            StopSequence::One(value) => vec![value],
            StopSequence::Many(values) => values,
        }
    }
}

/// An OpenAI message. `content` may be a string or a multimodal part array.
#[derive(Debug, Deserialize)]
pub struct OpenAiMessage {
    pub role: String,
    #[serde(default)]
    pub content: Option<OpenAiContent>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<OpenAiToolCall>>,
    /// Assistant reasoning text, as DeepSeek/OpenRouter expose it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    /// Anthropic signature for `reasoning_content`, replayed during tool use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_signature: Option<String>,
    /// Anthropic `redacted_thinking` payload, replayed verbatim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redacted_thinking: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum OpenAiContent {
    Text(String),
    Parts(Vec<OpenAiContentPart>),
    Null,
}

#[derive(Debug, Deserialize)]
pub struct OpenAiContentPart {
    #[serde(rename = "type")]
    pub part_type: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub image_url: Option<OpenAiImageUrl>,
    /// Every other field of the part, kept so a part the router does not
    /// translate can be replayed upstream exactly as the client sent it.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl OpenAiContentPart {
    /// Rebuilds the original wire object for verbatim replay.
    fn to_raw(&self) -> serde_json::Value {
        let mut object = self.extra.clone();
        object.insert(
            "type".to_string(),
            serde_json::Value::String(self.part_type.clone()),
        );
        if let Some(text) = &self.text {
            object.insert("text".to_string(), serde_json::Value::String(text.clone()));
        }
        if let Some(image) = &self.image_url {
            object.insert(
                "image_url".to_string(),
                serde_json::json!({ "url": image.url }),
            );
        }
        serde_json::Value::Object(object)
    }

    /// Converts one wire part into a router part.
    ///
    /// Text and image parts are translated; audio, video and file parts are
    /// validated and then carried verbatim, because OpenAI-compatible
    /// upstreams spell them differently. A part whose payload is missing, or a
    /// type the router cannot vouch for, is a `400` — silently dropping it
    /// would let the model answer a question it never saw.
    fn into_message_part(self) -> Result<Option<MessagePart>> {
        match self.part_type.as_str() {
            "text" => Ok(self.text.map(MessagePart::Text)),
            "image_url" => Ok(self.image_url.map(|image| MessagePart::ImageUrl(image.url))),
            kind if is_media_part_type(kind) => {
                let raw = self.to_raw();
                media_part(raw).map(Some)
            }
            other => Err(Error::BadRequest(format!(
                "unsupported content part type `{other}`; supported: text, image_url, {}",
                MEDIA_PART_TYPES.join(", ")
            ))),
        }
    }
}

/// Content part types the router forwards to an OpenAI-compatible upstream
/// without interpreting them.
///
/// `input_audio`/`file` are OpenAI's spelling, `audio_url`/`video_url` the
/// one most OpenAI-compatible gateways use, and `input_file`/`input_video` the
/// Responses API spelling.
const MEDIA_PART_TYPES: [&str; 6] = [
    "input_audio",
    "audio_url",
    "video_url",
    "input_video",
    "file",
    "input_file",
];

/// True when `kind` names a media part the router forwards verbatim.
pub(crate) fn is_media_part_type(kind: &str) -> bool {
    MEDIA_PART_TYPES.contains(&kind)
}

/// Validates a media part and wraps it for verbatim replay.
///
/// Shared with the Responses handler, which sees the same part shapes.
pub(crate) fn media_part(raw: serde_json::Value) -> Result<MessagePart> {
    validate_media_part(&raw)?;
    Ok(MessagePart::Media(raw))
}

/// Checks that a media part carries the payload its type promises.
///
/// The router does not decode the media, but an empty or missing payload is a
/// client bug worth surfacing before it reaches the provider.
fn validate_media_part(raw: &serde_json::Value) -> Result<()> {
    let kind = raw
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let payload = raw.get(kind).ok_or_else(|| {
        Error::BadRequest(format!(
            "content part `{kind}` is missing its `{kind}` payload"
        ))
    })?;

    match kind {
        "input_audio" => {
            let data = payload
                .get("data")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            if data.trim().is_empty() {
                return Err(Error::BadRequest(
                    "`input_audio` requires a non-empty base64 `data` field".to_string(),
                ));
            }
            if payload
                .get("format")
                .and_then(serde_json::Value::as_str)
                .is_none()
            {
                return Err(Error::BadRequest(
                    "`input_audio` requires a `format` field, e.g. `wav` or `mp3`".to_string(),
                ));
            }
        }
        _ => {
            // The URL-shaped parts (`audio_url`, `video_url`, `file`, ...) all
            // carry either a `url` string or a nested object holding one.
            let url = payload
                .as_str()
                .map(str::to_string)
                .or_else(|| part_url(payload));
            if url.as_deref().unwrap_or_default().trim().is_empty() {
                return Err(Error::BadRequest(format!(
                    "content part `{kind}` requires a non-empty `url`"
                )));
            }
        }
    }

    Ok(())
}

/// Reads a URL from either `{ "url": … }` or `{ "file_data": …, "filename": … }`.
fn part_url(payload: &serde_json::Value) -> Option<String> {
    payload
        .get("url")
        .and_then(serde_json::Value::as_str)
        .or_else(|| payload.get("file_data").and_then(serde_json::Value::as_str))
        .map(str::to_string)
}

#[derive(Debug, Deserialize)]
pub struct OpenAiImageUrl {
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct OpenAiToolCall {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub function: Option<OpenAiFunctionCall>,
}

#[derive(Debug, Deserialize)]
pub struct OpenAiFunctionCall {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arguments: Option<String>,
}

impl ChatCompletionRequest {
    /// Converts generation parameters into router-owned options.
    ///
    /// A malformed `tool_choice` or `response_format` is a `400`: silently
    /// ignoring one would answer a request the caller did not make, which is
    /// exactly the failure mode this whole layer exists to remove.
    pub fn generation_options(&self) -> Result<GenerationOptions> {
        Ok(GenerationOptions {
            temperature: self.temperature,
            top_p: self.top_p,
            top_k: self.top_k,
            max_tokens: self.max_completion_tokens.or(self.max_tokens),
            stop: self.stop.clone().map(StopSequence::into_vec),
            seed: self.seed,
            presence_penalty: self.presence_penalty,
            frequency_penalty: self.frequency_penalty,
            thinking_level: self.reasoning_effort.clone(),
            tool_choice: parse_tool_choice(self.tool_choice.as_ref())?,
            parallel_tool_calls: self.parallel_tool_calls,
            response_format: parse_response_format(self.response_format.as_ref())?,
            metadata: self.metadata.clone(),
            user: self.user.clone(),
            service_tier: self.service_tier.clone(),
            store: self.store,
            prompt_cache_key: self.prompt_cache_key.clone(),
            verbosity: self.verbosity.clone(),
            logprobs: self.logprobs,
            top_logprobs: self.top_logprobs,
            include: Vec::new(),
            web_search_options: self.web_search_options.clone(),
        })
    }

    /// Rejects a `web_search_options` value the router cannot forward.
    ///
    /// OpenAI documents the object with `search_context_size` and
    /// `user_location`; an OpenAI-compatible upstream may accept more. The
    /// router only insists on the one property every implementation agrees on
    /// being an object, so a malformed value is a `400` rather than a request
    /// that silently loses its web search.
    pub fn validate_web_search_options(&self) -> Result<()> {
        let Some(options) = &self.web_search_options else {
            return Ok(());
        };
        let object = options.as_object().ok_or_else(|| {
            Error::BadRequest("web_search_options must be a JSON object".to_string())
        })?;

        if let Some(size) = object.get("search_context_size") {
            let size = size.as_str().ok_or_else(|| {
                Error::BadRequest(
                    "web_search_options.search_context_size must be a string".to_string(),
                )
            })?;
            if !matches!(size, "low" | "medium" | "high") {
                return Err(Error::BadRequest(format!(
                    "web_search_options.search_context_size must be low, medium or high; got `{size}`"
                )));
            }
        }

        Ok(())
    }

    /// Converts the OpenAI message list into provider-layer messages.
    pub fn into_messages(self) -> Result<Vec<RouterMessage>> {
        if self.messages.is_empty() {
            return Err(Error::BadRequest("messages must not be empty".to_string()));
        }
        self.messages
            .into_iter()
            .map(OpenAiMessage::into_router_message)
            .collect()
    }
}

impl OpenAiMessage {
    /// Converts one OpenAI message into a provider-layer message.
    pub fn into_router_message(self) -> Result<RouterMessage> {
        let role = self.role.trim().to_ascii_lowercase();
        if role.is_empty() {
            return Err(Error::BadRequest("message role is required".to_string()));
        }

        let reasoning_content = self.reasoning_content;
        let thinking_signature = self.thinking_signature;
        let redacted_thinking = self.redacted_thinking;

        // Assistant messages carrying tool calls are rebuilt as such, so the
        // provider layer can pair them with the following tool results.
        if let Some(calls) = self.tool_calls.filter(|calls| !calls.is_empty()) {
            let specs = calls
                .into_iter()
                .filter_map(|call| {
                    let function = call.function?;
                    Some(ToolCallSpec {
                        id: call.id.unwrap_or_else(|| "call_0".to_string()),
                        name: function.name.unwrap_or_default(),
                        arguments: function.arguments.unwrap_or_else(|| "{}".to_string()),
                    })
                })
                .collect();

            let mut message = chat_backend::message_assistant_tool_calls(
                self.content.map(content_to_text).unwrap_or_default(),
                specs,
            );
            message.thinking = reasoning_content;
            message.thinking_signature = thinking_signature;
            message.redacted_thinking = redacted_thinking;
            return Ok(message);
        }

        if role == "tool" {
            let tool_call_id = self.tool_call_id.ok_or_else(|| {
                Error::BadRequest("tool message requires tool_call_id".to_string())
            })?;
            return Ok(chat_backend::message_tool_result(
                self.content.map(content_to_text).unwrap_or_default(),
                tool_call_id,
            ));
        }

        let mut message = match self.content {
            Some(OpenAiContent::Parts(parts)) => {
                let parts: Vec<MessagePart> = parts
                    .into_iter()
                    .map(OpenAiContentPart::into_message_part)
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .flatten()
                    .collect();

                if parts.is_empty() {
                    chat_backend::message_text(&role, String::new())
                } else {
                    chat_backend::message_parts(&role, parts)
                }
            }
            other => {
                chat_backend::message_text(&role, other.map(content_to_text).unwrap_or_default())
            }
        };
        if role == "assistant" {
            message.thinking = reasoning_content;
            message.thinking_signature = thinking_signature;
            message.redacted_thinking = redacted_thinking;
        }
        Ok(message)
    }
}

fn content_to_text(content: OpenAiContent) -> String {
    match content {
        OpenAiContent::Text(text) => text,
        OpenAiContent::Null => String::new(),
        OpenAiContent::Parts(parts) => parts
            .into_iter()
            .filter(|part| part.part_type == "text")
            .filter_map(|part| part.text)
            .collect::<Vec<_>>()
            .join(" "),
    }
}
