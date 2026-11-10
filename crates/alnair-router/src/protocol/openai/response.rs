//! OpenAI-shaped non-streaming response and SSE chunk payloads.

use serde::Serialize;

use crate::upstream::chat_backend::{self, ToolCallSpec};

/// Non-streaming JSON response in OpenAI shape.
#[derive(Debug, Serialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: &'static str,
    pub created: i64,
    pub model: String,
    pub choices: Vec<ChatChoice>,
    pub usage: UsagePayload,
}

#[derive(Debug, Serialize)]
pub struct ChatChoice {
    pub index: u32,
    pub message: ChatChoiceMessage,
    pub finish_reason: String,
}

#[derive(Debug, Serialize)]
pub struct ChatChoiceMessage {
    pub role: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OpenAiToolCallOut>>,
    /// Citations the model attached to its answer, when the upstream produced
    /// any. Omitted entirely when there are none, which is the normal case.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Vec<OpenAiAnnotation>>,
}

/// A `url_citation` annotation in the OpenAI shape.
#[derive(Debug, Serialize)]
pub struct OpenAiAnnotation {
    #[serde(rename = "type")]
    pub annotation_type: &'static str,
    pub url_citation: OpenAiUrlCitation,
}

#[derive(Debug, Serialize)]
pub struct OpenAiUrlCitation {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_index: Option<usize>,
}

/// Builds the `annotations` field, omitting it when the turn carried no
/// citations.
///
/// An upstream that produces none is a normal response, not an error: rejecting
/// a successful, already-billed turn over a missing array would be worse than
/// the absence.
pub fn annotations_payload(citations: &[chat_backend::Citation]) -> Option<Vec<OpenAiAnnotation>> {
    if citations.is_empty() {
        return None;
    }
    Some(
        citations
            .iter()
            .map(|citation| OpenAiAnnotation {
                annotation_type: "url_citation",
                url_citation: OpenAiUrlCitation {
                    url: citation.url.clone(),
                    title: citation.title.clone(),
                    start_index: citation.start_index,
                    end_index: citation.end_index,
                },
            })
            .collect(),
    )
}

/// Tool call in OpenAI `message.tool_calls` shape.
#[derive(Debug, Serialize)]
pub struct OpenAiToolCallOut {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: &'static str,
    pub function: OpenAiFunctionOut,
}

#[derive(Debug, Serialize)]
pub struct OpenAiFunctionOut {
    pub name: String,
    pub arguments: String,
}

/// Builds the `tool_calls` field, omitting it entirely when there are none.
pub fn tool_calls_payload(specs: &[ToolCallSpec]) -> Option<Vec<OpenAiToolCallOut>> {
    if specs.is_empty() {
        return None;
    }
    Some(
        specs
            .iter()
            .map(|spec| OpenAiToolCallOut {
                id: spec.id.clone(),
                call_type: "function",
                function: OpenAiFunctionOut {
                    name: spec.name.clone(),
                    arguments: spec.arguments.clone(),
                },
            })
            .collect(),
    )
}

#[derive(Debug, Serialize)]
pub struct UsagePayload {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_tokens_details: Option<PromptTokensDetails>,
}

#[derive(Debug, Serialize)]
pub struct PromptTokensDetails {
    pub cached_tokens: u64,
}

/// SSE `chat.completion.chunk` payload.
#[derive(Debug, Serialize)]
pub struct ChatCompletionChunk {
    pub id: String,
    pub object: &'static str,
    pub created: i64,
    pub model: String,
    pub choices: Vec<ChunkChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsagePayload>,
}

#[derive(Debug, Serialize)]
pub struct ChunkChoice {
    pub index: u32,
    pub delta: ChunkDelta,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Serialize, Default)]
pub struct ChunkDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Reasoning stream, as DeepSeek and OpenRouter expose it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallDelta>>,
}

/// A streamed tool call. Fragments are identified by `index`; the first one for
/// an index carries `id` and `name`, later ones only extend `arguments`.
#[derive(Debug, Serialize)]
pub struct ToolCallDelta {
    pub index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub call_type: Option<&'static str>,
    pub function: FunctionDelta,
}

#[derive(Debug, Serialize, Default)]
pub struct FunctionDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}

/// Builds the terminal usage payload for a completed request.
pub fn usage_payload(usage: Option<chat_backend::TokenUsage>) -> UsagePayload {
    let usage = usage.unwrap_or_default();
    UsagePayload {
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
        total_tokens: usage.prompt_tokens + usage.completion_tokens,
        prompt_tokens_details: (usage.cached_tokens > 0).then_some(PromptTokensDetails {
            cached_tokens: usage.cached_tokens,
        }),
    }
}
