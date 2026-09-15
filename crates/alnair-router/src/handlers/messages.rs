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
use crate::handlers::shared::{StreamUsage, record_failed_attempts, router_headers};
use crate::middleware::AuthenticatedKey;
use crate::protocol::anthropic::{
    AnthropicResponseBlock, AnthropicUsage, CountTokensResponse, MessagesRequest, MessagesResponse,
    estimate_messages,
};
use crate::state::AppState;
use crate::upstream::ExecutedStream;
use crate::upstream::chat_backend::{self, StreamChunk};

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

    let resolver = state.resolver().await?;
    let targets = resolver.resolve(&requested_model)?;

    let executed = state
        .executor
        .stream(&targets, messages, tools, Some(&options), stream_requested)
        .await?;

    record_failed_attempts(&state, &api_key_id, &requested_model, &executed.attempts).await;

    if stream_requested {
        Ok(stream_response(
            state,
            api_key_id,
            requested_model,
            executed,
        ))
    } else {
        complete_response(state, api_key_id, requested_model, executed).await
    }
}

async fn complete_response(
    state: AppState,
    api_key_id: Option<String>,
    requested_model: String,
    executed: ExecutedStream,
) -> Result<Response> {
    let target = executed.target.clone();
    let latency_ms = executed.latency_ms;
    let attempt_count = executed.attempts.len();

    let completion = chat_backend::collect(executed.stream).await?;
    let usage = completion.usage.unwrap_or_default();

    state.metrics.record_request(latency_ms);
    state.metrics.record_usage(
        usage.prompt_tokens,
        usage.completion_tokens,
        usage.cached_tokens,
        usage.cost_usd,
    );
    state.telemetry.record_usage(
        &target.connection_id,
        usage.prompt_tokens,
        usage.completion_tokens,
    );

    if let Err(error) = state
        .usage()
        .record(NewUsageRecord {
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
            cost_usd: usage.cost_usd,
            latency_ms,
        })
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

fn stream_response(
    state: AppState,
    api_key_id: Option<String>,
    requested_model: String,
    executed: ExecutedStream,
) -> Response {
    let target = executed.target.clone();
    let latency_ms = executed.latency_ms;
    let attempt_count = executed.attempts.len();
    let id = message_id();

    let usage_state = StreamUsage::new(
        state,
        api_key_id,
        requested_model.clone(),
        &target,
        attempt_count,
        latency_ms,
    );

    // Anthropic clients expect a fixed event order: message_start, then the
    // content block opening, deltas, and finally the closing events.
    let preamble: Vec<std::result::Result<Event, std::convert::Infallible>> = vec![
        Ok(Event::default().event("message_start").data(
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
        )),
        Ok(Event::default().event("content_block_start").data(
            json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": { "type": "text", "text": "" }
            })
            .to_string(),
        )),
    ];

    let stream = executed.stream.flat_map(move |chunk| {
        let mut usage_state = usage_state.clone();

        let events: Vec<std::result::Result<Event, std::convert::Infallible>> = match chunk {
            Ok(StreamChunk::Text(text)) => {
                vec![Ok(Event::default().event("content_block_delta").data(
                    json!({
                        "type": "content_block_delta",
                        "index": 0,
                        "delta": { "type": "text_delta", "text": text }
                    })
                    .to_string(),
                ))]
            }
            Ok(StreamChunk::Thinking(_)) | Ok(StreamChunk::ToolCall { .. }) => Vec::new(),
            Ok(StreamChunk::Usage(usage)) => {
                usage_state.record(Some(usage), "ok");
                Vec::new()
            }
            Ok(StreamChunk::Done) => vec![
                Ok(Event::default()
                    .event("content_block_stop")
                    .data(json!({ "type": "content_block_stop", "index": 0 }).to_string())),
                Ok(Event::default().event("message_delta").data(
                    json!({
                        "type": "message_delta",
                        "delta": { "stop_reason": "end_turn", "stop_sequence": null },
                        "usage": { "output_tokens": 0 }
                    })
                    .to_string(),
                )),
                Ok(Event::default()
                    .event("message_stop")
                    .data(json!({ "type": "message_stop" }).to_string())),
            ],
            Err(error) => {
                tracing::error!(error = %error, "stream terminated with an upstream error");
                usage_state.record(None, "error");
                vec![Ok(Event::default().event("error").data(
                    json!({
                        "type": "error",
                        "error": { "type": "upstream_error", "message": error.to_string() }
                    })
                    .to_string(),
                ))]
            }
        };
        futures::stream::iter(events)
    });

    let stream = futures::stream::iter(preamble).chain(stream);

    let mut response = Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response();
    response
        .headers_mut()
        .extend(router_headers(&target, attempt_count));
    response
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
