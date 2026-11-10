//! Responses API response objects and the non-streaming body builder.

use serde::Serialize;

use crate::upstream::chat_backend::{self, ToolCallSpec};

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
    /// `url_citation` annotations, omitted when the turn carried none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Vec<AnnotationPayload>>,
}

/// A `url_citation` annotation in the Responses shape, shared by the one-shot
/// body, the annotation event and the terminal response snapshot.
#[derive(Debug, Clone, Serialize)]
pub struct AnnotationPayload {
    #[serde(rename = "type")]
    pub annotation_type: &'static str,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_index: Option<usize>,
}

/// Builds the annotation list, or `None` when the turn carried no citations.
pub(super) fn annotations_payload(
    citations: &[chat_backend::Citation],
) -> Option<Vec<AnnotationPayload>> {
    if citations.is_empty() {
        return None;
    }
    Some(
        citations
            .iter()
            .map(|citation| AnnotationPayload {
                annotation_type: "url_citation",
                url: citation.url.clone(),
                title: citation.title.clone(),
                start_index: citation.start_index,
                end_index: citation.end_index,
            })
            .collect(),
    )
}

#[derive(Debug, Serialize)]
pub struct ResponsesUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

/// Builds the response object for a finished completion.
pub(super) fn completion_payload(
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
                annotations: annotations_payload(&completion.citations),
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
