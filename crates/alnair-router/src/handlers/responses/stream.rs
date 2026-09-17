//! Responses API streaming: `response.*` SSE events over the shared chunk stream.

use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures::StreamExt;
use serde_json::json;

use crate::handlers::shared::{StreamUsage, router_headers};
use crate::state::AppState;
use crate::token_saver::Savings;
use crate::upstream::ExecutedStream;
use crate::upstream::chat_backend::{StreamChunk, TokenUsage};

use super::{ResponsesContent, ResponsesOutput, ResponsesUsage, message_id};

/// One SSE item; the error type is uninhabited, so a stream never fails mid-flight.
type SseItem = std::result::Result<Event, std::convert::Infallible>;

/// State carried across the streamed response.
struct StreamState {
    usage: StreamUsage,
    response_id: String,
    created_at: i64,
    model: String,
    sequence: u64,
    /// Next `output_index` to hand out; items are numbered in arrival order.
    next_index: usize,
    /// Open reasoning item: index, id and the text accumulated so far.
    reasoning: Option<(usize, String, String)>,
    /// Open text item: index, id and the text accumulated so far.
    text: Option<(usize, String, String)>,
    /// Items already emitted, kept so the terminal response can report them.
    /// Closed items are removed from the open slots above, so the final output
    /// has to be accumulated as they finish rather than read back.
    items: Vec<(usize, ResponsesOutput)>,
    totals: Option<TokenUsage>,
}

/// Streams a Responses reply as the documented `response.*` event sequence.
pub(super) async fn responses_stream(
    state: AppState,
    api_key_id: Option<String>,
    requested_model: String,
    saver_savings: Savings,
    executed: ExecutedStream,
) -> Response {
    let target = executed.target.clone();
    let latency_ms = executed.latency_ms;
    let attempt_count = executed.attempts.len();
    let created_at = chrono::Utc::now().timestamp();

    // Priced before `state` moves into the recorder, which owns it for the life
    // of the stream.
    let price = crate::handlers::shared::tier_price(&state, &target).await;
    let usage_state = StreamUsage::new(
        state,
        api_key_id,
        requested_model.clone(),
        &target,
        attempt_count,
        latency_ms,
    )
    .with_savings(saver_savings, price);

    let mut stream_state = StreamState {
        usage: usage_state,
        response_id: super::response_id(),
        created_at,
        model: requested_model.clone(),
        sequence: 0,
        next_index: 0,
        reasoning: None,
        text: None,
        items: Vec::new(),
        totals: None,
    };

    // The stream opens by announcing the response and its in-progress status.
    let mut opening: Vec<SseItem> = Vec::new();
    opening.push(stream_state.event(
        "response.created",
        json!({
            "type": "response.created",
            "response": stream_state.snapshot("in_progress", Vec::new()),
        }),
    ));
    opening.push(stream_state.event(
        "response.in_progress",
        json!({
            "type": "response.in_progress",
            "response": stream_state.snapshot("in_progress", Vec::new()),
        }),
    ));

    let stream = executed
        .stream
        .scan(stream_state, move |state, chunk| {
            let events = advance(state, chunk);
            async move { Some(events) }
        })
        .flat_map(futures::stream::iter);

    let stream = futures::stream::iter(opening).chain(stream);

    let mut response = Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response();
    response
        .headers_mut()
        .extend(router_headers(&target, attempt_count));
    response
}

/// Maps one upstream chunk onto the events it produces.
fn advance(state: &mut StreamState, chunk: crate::error::Result<StreamChunk>) -> Vec<SseItem> {
    let mut events = Vec::new();

    match chunk {
        Ok(StreamChunk::Thinking(text)) => {
            // Reasoning always precedes content, so opening a reasoning item
            // implicitly closes a text item from an earlier turn.
            close_text(state, &mut events);
            let (index, id) = ensure_reasoning(state, &mut events);
            if let Some((_, _, buffer)) = state.reasoning.as_mut() {
                buffer.push_str(&text);
            }
            events.push(state.event(
                "response.reasoning_summary_text.delta",
                json!({
                    "type": "response.reasoning_summary_text.delta",
                    "item_id": id,
                    "output_index": index,
                    "summary_index": 0,
                    "delta": text,
                }),
            ));
        }
        Ok(StreamChunk::Text(text)) => {
            close_reasoning(state, &mut events);
            let (index, id) = ensure_text(state, &mut events);
            if let Some((_, _, buffer)) = state.text.as_mut() {
                buffer.push_str(&text);
            }
            events.push(state.event(
                "response.output_text.delta",
                json!({
                    "type": "response.output_text.delta",
                    "item_id": id,
                    "output_index": index,
                    "content_index": 0,
                    "delta": text,
                }),
            ));
        }
        Ok(StreamChunk::ToolCall {
            id: call_id,
            name,
            arguments,
        }) => {
            close_reasoning(state, &mut events);
            close_text(state, &mut events);

            let index = state.next_index;
            state.next_index += 1;
            let item_id = format!("fc_{}", uuid::Uuid::new_v4().simple());

            events.push(state.event(
                "response.output_item.added",
                json!({
                    "type": "response.output_item.added",
                    "output_index": index,
                    "item": {
                        "type": "function_call",
                        "id": item_id,
                        "call_id": call_id,
                        "name": name,
                        "arguments": "",
                        "status": "in_progress",
                    },
                }),
            ));
            events.push(state.event(
                "response.function_call_arguments.delta",
                json!({
                    "type": "response.function_call_arguments.delta",
                    "item_id": item_id,
                    "output_index": index,
                    "delta": arguments,
                }),
            ));
            events.push(state.event(
                "response.function_call_arguments.done",
                json!({
                    "type": "response.function_call_arguments.done",
                    "item_id": item_id,
                    "output_index": index,
                    "arguments": arguments,
                }),
            ));
            events.push(state.event(
                "response.output_item.done",
                json!({
                    "type": "response.output_item.done",
                    "output_index": index,
                    "item": {
                        "type": "function_call",
                        "id": item_id,
                        "call_id": call_id,
                        "name": name,
                        "arguments": arguments,
                        "status": "completed",
                    },
                }),
            ));

            state.items.push((
                index,
                ResponsesOutput::FunctionCall {
                    id: item_id,
                    call_id,
                    name,
                    arguments,
                },
            ));
        }
        Ok(StreamChunk::Usage(usage)) => {
            state.totals = Some(usage);
            state.usage.record(Some(usage), "ok");
        }
        Ok(StreamChunk::Done(_)) => {
            close_reasoning(state, &mut events);
            close_text(state, &mut events);

            let output = state.output();
            events.push(state.event(
                "response.completed",
                json!({
                    "type": "response.completed",
                    "response": state.snapshot("completed", output),
                }),
            ));
        }
        Err(error) => {
            tracing::error!(error = %error, "responses stream terminated with an upstream error");
            state.usage.record(None, "error");
            events.push(state.event(
                "response.failed",
                json!({
                    "type": "response.failed",
                    "response": state.snapshot("failed", Vec::new()),
                    "error": { "message": error.to_string(), "type": "upstream_error" },
                }),
            ));
        }
    }

    events
}

impl StreamState {
    /// Builds one SSE event and advances the sequence counter.
    fn event(&mut self, name: &str, mut payload: serde_json::Value) -> SseItem {
        self.sequence += 1;
        if let Some(object) = payload.as_object_mut() {
            object.insert("sequence_number".to_string(), json!(self.sequence));
        }
        Ok(Event::default().event(name).data(payload.to_string()))
    }

    /// Current response object, with the items emitted so far.
    fn snapshot(&self, status: &str, output: Vec<ResponsesOutput>) -> serde_json::Value {
        json!({
            "id": self.response_id,
            "object": "response",
            "created_at": self.created_at,
            "status": status,
            "model": self.model,
            "output": output,
            "usage": self.totals.map(|usage| ResponsesUsage {
                input_tokens: usage.prompt_tokens,
                output_tokens: usage.completion_tokens,
                total_tokens: usage.prompt_tokens + usage.completion_tokens,
            }),
        })
    }

    /// Output items in the order they were opened.
    fn output(&self) -> Vec<ResponsesOutput> {
        let mut items = self.items.clone();
        items.sort_by_key(|(index, _)| *index);
        items.into_iter().map(|(_, item)| item).collect()
    }
}

/// Opens the reasoning item if it is not already open.
fn ensure_reasoning(state: &mut StreamState, events: &mut Vec<SseItem>) -> (usize, String) {
    if let Some((index, id, _)) = &state.reasoning {
        return (*index, id.clone());
    }

    let index = state.next_index;
    state.next_index += 1;
    let id = format!("rs_{}", uuid::Uuid::new_v4().simple());

    events.push(state.event(
        "response.output_item.added",
        json!({
            "type": "response.output_item.added",
            "output_index": index,
            "item": { "type": "reasoning", "id": id, "summary": [] },
        }),
    ));
    events.push(state.event(
        "response.reasoning_summary_part.added",
        json!({
            "type": "response.reasoning_summary_part.added",
            "item_id": id,
            "output_index": index,
            "summary_index": 0,
            "part": { "type": "summary_text", "text": "" },
        }),
    ));

    state.reasoning = Some((index, id.clone(), String::new()));
    (index, id)
}

/// Closes the open reasoning item, if any.
fn close_reasoning(state: &mut StreamState, events: &mut Vec<SseItem>) {
    let Some((index, id, text)) = state.reasoning.take() else {
        return;
    };

    events.push(state.event(
        "response.reasoning_summary_text.done",
        json!({
            "type": "response.reasoning_summary_text.done",
            "item_id": id,
            "output_index": index,
            "summary_index": 0,
            "text": text,
        }),
    ));
    events.push(state.event(
        "response.reasoning_summary_part.done",
        json!({
            "type": "response.reasoning_summary_part.done",
            "item_id": id,
            "output_index": index,
            "summary_index": 0,
            "part": { "type": "summary_text", "text": text },
        }),
    ));
    events.push(state.event(
        "response.output_item.done",
        json!({
            "type": "response.output_item.done",
            "output_index": index,
            "item": {
                "type": "reasoning",
                "id": id,
                "summary": [{ "type": "summary_text", "text": text }],
            },
        }),
    ));

    state.items.push((
        index,
        ResponsesOutput::Reasoning {
            id,
            summary: vec![ResponsesContent {
                content_type: "summary_text",
                text,
            }],
        },
    ));
}

/// Opens the assistant text item if it is not already open.
fn ensure_text(state: &mut StreamState, events: &mut Vec<SseItem>) -> (usize, String) {
    if let Some((index, id, _)) = &state.text {
        return (*index, id.clone());
    }

    let index = state.next_index;
    state.next_index += 1;
    let id = message_id();

    events.push(state.event(
        "response.output_item.added",
        json!({
            "type": "response.output_item.added",
            "output_index": index,
            "item": {
                "type": "message",
                "id": id,
                "role": "assistant",
                "status": "in_progress",
                "content": [],
            },
        }),
    ));
    events.push(state.event(
        "response.content_part.added",
        json!({
            "type": "response.content_part.added",
            "item_id": id,
            "output_index": index,
            "content_index": 0,
            "part": { "type": "output_text", "text": "" },
        }),
    ));

    state.text = Some((index, id.clone(), String::new()));
    (index, id)
}

/// Closes the open text item, if any.
fn close_text(state: &mut StreamState, events: &mut Vec<SseItem>) {
    let Some((index, id, text)) = state.text.take() else {
        return;
    };

    events.push(state.event(
        "response.output_text.done",
        json!({
            "type": "response.output_text.done",
            "item_id": id,
            "output_index": index,
            "content_index": 0,
            "text": text,
        }),
    ));
    events.push(state.event(
        "response.content_part.done",
        json!({
            "type": "response.content_part.done",
            "item_id": id,
            "output_index": index,
            "content_index": 0,
            "part": { "type": "output_text", "text": text },
        }),
    ));
    events.push(state.event(
        "response.output_item.done",
        json!({
            "type": "response.output_item.done",
            "output_index": index,
            "item": {
                "type": "message",
                "id": id,
                "role": "assistant",
                "status": "completed",
                "content": [{ "type": "output_text", "text": text }],
            },
        }),
    ));

    state.items.push((
        index,
        ResponsesOutput::Message {
            id,
            role: "assistant",
            status: "completed",
            content: vec![ResponsesContent {
                content_type: "output_text",
                text,
            }],
        },
    ));
}
