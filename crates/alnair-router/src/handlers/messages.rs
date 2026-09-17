//! `POST /v1/messages` and `/v1/messages/count_tokens` (Anthropic shape).

use axum::Extension;
use axum::Json;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures::StreamExt;
use serde_json::json;

use crate::db::repos::usage::NewUsageRecord;
use crate::error::Result;
use crate::handlers::shared::{StreamUsage, record_failed_attempts, router_headers, tier_price};
use crate::middleware::AuthenticatedKey;
use crate::protocol::anthropic::{
    AnthropicResponseBlock, AnthropicUsage, CountTokensResponse, MessagesRequest, MessagesResponse,
    estimate_messages,
};
use crate::state::AppState;
use crate::token_saver::Savings;
use crate::upstream::ExecutedStream;
use crate::upstream::chat_backend::{self, StreamChunk, TokenUsage, anthropic_stop_reason};

/// Handles an Anthropic Messages request, streaming or not.
pub async fn messages(
    State(state): State<AppState>,
    Extension(key): Extension<Option<AuthenticatedKey>>,
    Json(request): Json<MessagesRequest>,
) -> Result<Response> {
    let api_key_id = key.as_ref().map(|auth| auth.key.id.clone());

    let requested_model = request.model.clone();
    if let Some(auth) = &key {
        auth.policy.ensure_model(&requested_model)?;
    }
    let stream_requested = request.stream;

    let chat_request = request.into_chat_request()?;
    let options = chat_request.generation_options();
    let tools = chat_request.tools.clone();
    let messages = chat_request.into_messages()?;
    let (messages, saver_savings) =
        crate::token_saver::apply(&state.token_saver_settings(), messages, &requested_model).await;

    let resolver = state.resolver().await?;
    let targets = resolver.resolve(&requested_model)?;

    let executed = state
        .executor
        .stream(&targets, messages, tools, Some(&options), stream_requested)
        .await?;

    record_failed_attempts(&state, &api_key_id, &requested_model, &executed.attempts).await;

    if stream_requested {
        Ok(stream_response(state, api_key_id, requested_model, saver_savings, executed).await)
    } else {
        complete_response(state, api_key_id, requested_model, saver_savings, executed).await
    }
}

async fn complete_response(
    state: AppState,
    api_key_id: Option<String>,
    requested_model: String,
    saver_savings: Savings,
    executed: ExecutedStream,
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

    if let Err(error) = state
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
        .await
    {
        tracing::warn!(error = %error, "failed to record usage");
    }

    let mut content = Vec::new();
    if !completion.content.is_empty() {
        content.push(AnthropicResponseBlock::Text {
            text: completion.content.clone(),
        });
    }
    for call in &completion.tool_calls {
        content.push(AnthropicResponseBlock::ToolUse {
            id: call.id.clone(),
            name: call.name.clone(),
            input: serde_json::from_str(&call.arguments).unwrap_or(serde_json::Value::Null),
        });
    }

    let stop_reason = if completion.tool_calls.is_empty() {
        completion
            .finish_reason
            .clone()
            .unwrap_or_else(|| "end_turn".to_string())
    } else {
        "tool_use".to_string()
    };

    let body = MessagesResponse {
        id: message_id(),
        response_type: "message",
        role: "assistant",
        model: requested_model,
        content,
        stop_reason,
        usage: AnthropicUsage {
            input_tokens: usage.prompt_tokens,
            output_tokens: usage.completion_tokens,
        },
    };

    Ok((router_headers(&target, attempt_count), Json(body)).into_response())
}

async fn stream_response(
    state: AppState,
    api_key_id: Option<String>,
    requested_model: String,
    saver_savings: Savings,
    executed: ExecutedStream,
) -> Response {
    let target = executed.target.clone();
    let latency_ms = executed.latency_ms;
    let attempt_count = executed.attempts.len();
    let id = message_id();

    let price = tier_price(&state, &target).await;

    let usage_state = StreamUsage::new(
        state,
        api_key_id,
        requested_model.clone(),
        &target,
        attempt_count,
        latency_ms,
    )
    .with_savings(saver_savings, price);

    // Anthropic clients expect `message_start` first; content blocks open
    // lazily, because the kind of the first block is only known once a chunk
    // arrives (text, thinking or a tool call).
    let preamble: Vec<std::result::Result<Event, std::convert::Infallible>> =
        vec![Ok(Event::default().event("message_start").data(
            json!({
                "type": "message_start",
                "message": {
                    "id": id,
                    "type": "message",
                    "role": "assistant",
                    "model": requested_model,
                    "content": [],
                    "stop_reason": null,
                    "usage": { "input_tokens": 0, "output_tokens": 0 }
                }
            })
            .to_string(),
        ))];

    let stream = executed
        .stream
        .scan(
            MessagesState {
                usage: usage_state,
                next_index: 0,
                open: None,
                has_tool_calls: false,
                totals: None,
            },
            move |state, chunk| {
                let events = message_events(state, chunk);
                async move { Some(events) }
            },
        )
        .flat_map(futures::stream::iter);

    let stream = futures::stream::iter(preamble).chain(stream);

    let mut response = Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response();
    response
        .headers_mut()
        .extend(router_headers(&target, attempt_count));
    response
}

/// Kind of Anthropic content block currently being streamed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockKind {
    Text,
    Thinking,
}

/// State carried across a streaming Anthropic response.
struct MessagesState {
    usage: StreamUsage,
    /// Index of the next content block to open.
    next_index: usize,
    /// The block currently open, if any, with its index.
    open: Option<(BlockKind, usize)>,
    /// Whether any tool call was forwarded, which drives `stop_reason`.
    has_tool_calls: bool,
    /// Latest token usage, reported on the closing `message_delta`.
    totals: Option<TokenUsage>,
}

/// Closes the open content block, if there is one.
fn close_block(
    state: &mut MessagesState,
    events: &mut Vec<std::result::Result<Event, std::convert::Infallible>>,
) {
    let Some((_, index)) = state.open.take() else {
        return;
    };

    events.push(Ok(Event::default().event("content_block_stop").data(
        json!({ "type": "content_block_stop", "index": index }).to_string(),
    )));
}

/// Opens a block of `kind`, reusing the current one when it already matches.
///
/// Anthropic requires one `content_block_start` per block and a matching stop,
/// so switching between text, thinking and tool calls has to close first.
fn ensure_block(
    state: &mut MessagesState,
    events: &mut Vec<std::result::Result<Event, std::convert::Infallible>>,
    kind: BlockKind,
) -> usize {
    if let Some((open_kind, index)) = state.open
        && open_kind == kind
    {
        return index;
    }

    close_block(state, events);

    let index = state.next_index;
    state.next_index += 1;
    state.open = Some((kind, index));

    let content_block = match kind {
        BlockKind::Text => json!({ "type": "text", "text": "" }),
        BlockKind::Thinking => json!({ "type": "thinking", "thinking": "" }),
    };
    events.push(Ok(Event::default().event("content_block_start").data(
        json!({
            "type": "content_block_start",
            "index": index,
            "content_block": content_block
        })
        .to_string(),
    )));

    index
}

/// Translates one upstream chunk into the Anthropic events it produces.
fn message_events(
    state: &mut MessagesState,
    chunk: Result<StreamChunk>,
) -> Vec<std::result::Result<Event, std::convert::Infallible>> {
    let mut events = Vec::new();

    match chunk {
        Ok(StreamChunk::Text(text)) => {
            let index = ensure_block(state, &mut events, BlockKind::Text);
            events.push(Ok(Event::default().event("content_block_delta").data(
                json!({
                    "type": "content_block_delta",
                    "index": index,
                    "delta": { "type": "text_delta", "text": text }
                })
                .to_string(),
            )));
        }
        Ok(StreamChunk::Thinking(text)) => {
            let index = ensure_block(state, &mut events, BlockKind::Thinking);
            events.push(Ok(Event::default().event("content_block_delta").data(
                json!({
                    "type": "content_block_delta",
                    "index": index,
                    "delta": { "type": "thinking_delta", "thinking": text }
                })
                .to_string(),
            )));
        }
        Ok(StreamChunk::ToolCall {
            id,
            name,
            arguments,
        }) => {
            close_block(state, &mut events);

            let index = state.next_index;
            state.next_index += 1;
            state.has_tool_calls = true;

            events.push(Ok(Event::default().event("content_block_start").data(
                json!({
                    "type": "content_block_start",
                    "index": index,
                    "content_block": {
                        "type": "tool_use", "id": id, "name": name, "input": {}
                    }
                })
                .to_string(),
            )));
            // The provider seam hands over whole tool calls, so the arguments
            // go out as a single `partial_json` fragment.
            events.push(Ok(Event::default().event("content_block_delta").data(
                json!({
                    "type": "content_block_delta",
                    "index": index,
                    "delta": { "type": "input_json_delta", "partial_json": arguments }
                })
                .to_string(),
            )));
            events.push(Ok(Event::default().event("content_block_stop").data(
                json!({ "type": "content_block_stop", "index": index }).to_string(),
            )));
        }
        Ok(StreamChunk::Usage(usage)) => {
            state.totals = Some(usage);
            state.usage.record(Some(usage), "ok");
        }
        Ok(StreamChunk::Done(reason)) => {
            close_block(state, &mut events);

            let stop_reason = anthropic_stop_reason(reason.as_deref(), state.has_tool_calls);
            let output_tokens = state.totals.map_or(0, |totals| totals.completion_tokens);

            events.push(Ok(Event::default().event("message_delta").data(
                json!({
                    "type": "message_delta",
                    "delta": { "stop_reason": stop_reason, "stop_sequence": null },
                    "usage": { "output_tokens": output_tokens }
                })
                .to_string(),
            )));
            events.push(Ok(Event::default()
                .event("message_stop")
                .data(json!({ "type": "message_stop" }).to_string())));
        }
        Err(error) => {
            tracing::error!(error = %error, "stream terminated with an upstream error");
            state.usage.record(None, "error");
            events.push(Ok(Event::default().event("error").data(
                json!({
                    "type": "error",
                    "error": { "type": "upstream_error", "message": error.to_string() }
                })
                .to_string(),
            )));
        }
    }

    events
}

/// `POST /v1/messages/count_tokens` — heuristic estimate, no tokenizer dependency.
pub async fn count_tokens(
    Extension(key): Extension<Option<AuthenticatedKey>>,
    Json(request): Json<MessagesRequest>,
) -> Result<Json<CountTokensResponse>> {
    if let Some(auth) = &key {
        auth.policy.ensure_model(&request.model)?;
    }

    let chat_request = request.into_chat_request()?;
    let messages = chat_request.into_messages()?;
    Ok(Json(CountTokensResponse {
        input_tokens: estimate_messages(&messages),
    }))
}

fn message_id() -> String {
    format!("msg_{}", uuid::Uuid::new_v4().simple())
}
