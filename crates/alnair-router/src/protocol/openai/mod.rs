//! Conversion between the OpenAI wire format and router-owned types.
//!
//! Split into files because the wire shapes are large:
//!
//! - [`request`] — `ChatCompletionRequest` and its mapping onto the hub.
//! - [`options`] — the `tool_choice` / `response_format` spellings shared with
//!   the Anthropic and Responses endpoints.
//! - [`response`] — `ChatCompletionResponse`, SSE chunk payloads, and the
//!   helpers the chat handler builds them with.
//!
//! `protocol::openai::ChatCompletionRequest` keeps its path so no call site
//! moves.

mod options;
mod request;
mod response;

pub(crate) use options::{parse_response_format, parse_tool_choice};
pub use request::{
    ChatCompletionRequest, OpenAiContent, OpenAiContentPart, OpenAiFunctionCall, OpenAiImageUrl,
    OpenAiMessage, OpenAiToolCall, StopSequence, StreamOptions,
};
pub(crate) use request::{is_media_part_type, media_part};
pub use response::{
    ChatChoice, ChatChoiceMessage, ChatCompletionChunk, ChatCompletionResponse, ChunkChoice,
    ChunkDelta, FunctionDelta, OpenAiAnnotation, OpenAiFunctionOut, OpenAiToolCallOut,
    OpenAiUrlCitation, PromptTokensDetails, ToolCallDelta, UsagePayload, annotations_payload,
    tool_calls_payload, usage_payload,
};

#[cfg(test)]
mod tests;
