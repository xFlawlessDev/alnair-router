//! `POST /v1/chat/completions`: resolve → fallback-execute → OpenAI-shaped reply.

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
use crate::protocol::openai::{
    ChatChoice, ChatChoiceMessage, ChatCompletionChunk, ChatCompletionRequest,
    ChatCompletionResponse, ChunkChoice, ChunkDelta, tool_calls_payload, usage_payload,
};
use crate::state::AppState;
use crate::upstream::ExecutedStream;
use crate::upstream::chat_backend::{self, StreamChunk};

/// Handles a chat completion, streaming or not based on `stream`.
pub async fn chat_completions(
    State(state): State<AppState>,
    axum::Extension(key): axum::Extension<Option<AuthenticatedKey>>,
    Json(request): Json<ChatCompletionRequest>,
) -> Result<Response> {
    let api_key_id = key.as_ref().map(|auth| auth.key.id.clone());

    let requested_model = request.model.clone();
    if let Some(auth) = &key {
        auth.policy.ensure_model(&requested_model)?;
    }
    let stream_requested = request.stream;
    let options = request.generation_options();
    let tools = request.tools.clone();
    let messages = request.into_messages()?;

    let resolver = state.resolver().await?;
    let targets = resolver.resolve(&requested_model)?;

    let executed = state
        .executor
        .stream(&targets, messages, tools, Some(&options), stream_requested)
        .await?;

    // Failures that happened before a tier succeeded are logged immediately.
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

/// Non-streaming path: drain the stream, then emit a single JSON body.
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

    let usage_snapshot = completion.usage.unwrap_or_default();
    state.metrics.record_request(latency_ms);
    state.metrics.record_usage(
        usage_snapshot.prompt_tokens,
        usage_snapshot.completion_tokens,
        usage_snapshot.cached_tokens,
        usage_snapshot.cost_usd,
    );
    state.telemetry.record_usage(
        &target.connection_id,
        usage_snapshot.prompt_tokens,
        usage_snapshot.completion_tokens,
    );

    if let Some(usage) = completion.usage {
        state
            .usage()
            .record(NewUsageRecord {
                api_key_id,
                requested_model: requested_model.clone(),
                resolved_provider: Some(target.provider_type.clone()),
                resolved_model: Some(target.model.clone()),
                attempt: attempt_count,
                status: "ok".to_string(),
                prompt_tokens: usage.prompt_tokens,
                completion_tokens: usage.completion_tokens,
                cached_tokens: usage.cached_tokens,
                cost_usd: usage.cost_usd,
                latency_ms,
            })
            .await?;
    }

    let body = ChatCompletionResponse {
        id: completion_id(),
        object: "chat.completion",
        created: chrono::Utc::now().timestamp(),
        model: requested_model,
        choices: vec![ChatChoice {
            index: 0,
            message: ChatChoiceMessage {
                role: "assistant",
                content: (!completion.content.is_empty()).then(|| completion.content.clone()),
                tool_calls: tool_calls_payload(&completion.tool_calls),
            },
            finish_reason: completion
                .finish_reason
                .unwrap_or_else(|| "stop".to_string()),
        }],
        usage: usage_payload(completion.usage),
    };

    Ok((router_headers(&target, attempt_count), Json(body)).into_response())
}

/// Streaming path: forward chunks as OpenAI `chat.completion.chunk` SSE events.
fn stream_response(
    state: AppState,
    api_key_id: Option<String>,
    requested_model: String,
    executed: ExecutedStream,
) -> Response {
    let target = executed.target.clone();
    let latency_ms = executed.latency_ms;
    let attempt_count = executed.attempts.len();
    let id = completion_id();
    let created = chrono::Utc::now().timestamp();

    let usage_state = StreamUsage::new(
        state.clone(),
        api_key_id,
        requested_model.clone(),
        &target,
        attempt_count,
        latency_ms,
    );

    let model = requested_model.clone();
    let stream = executed.stream.flat_map(move |chunk| {
        let id = id.clone();
        let model = model.clone();
        let mut usage_state = usage_state.clone();

        let events: Vec<std::result::Result<Event, std::convert::Infallible>> = match chunk {
            Ok(StreamChunk::Text(text)) => vec![Ok(chunk_event(
                &id,
                created,
                &model,
                ChunkDelta {
                    role: None,
                    content: Some(text),
                },
                None,
            ))],
            // Thinking, tool calls and usage have no OpenAI chunk equivalent in
            // this minimal shape; usage is recorded rather than forwarded.
            Ok(StreamChunk::Thinking(_)) | Ok(StreamChunk::ToolCall { .. }) => Vec::new(),
            Ok(StreamChunk::Usage(usage)) => {
                usage_state.record(Some(usage), "ok");
                Vec::new()
            }
            Ok(StreamChunk::Done) => vec![
                Ok(chunk_event(
                    &id,
                    created,
                    &model,
                    ChunkDelta::default(),
                    Some("stop".to_string()),
                )),
                Ok(Event::default().data("[DONE]")),
            ],
            Err(error) => {
                tracing::error!(error = %error, "stream terminated with an upstream error");
                usage_state.record(None, "error");
                vec![Ok(Event::default().data(
                    json!({
                        "error": { "message": error.to_string(), "type": "upstream_error" }
                    })
                    .to_string(),
                ))]
            }
        };
        futures::stream::iter(events)
    });

    let mut response = Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response();
    response
        .headers_mut()
        .extend(router_headers(&target, attempt_count));
    response
}

fn chunk_event(
    id: &str,
    created: i64,
    model: &str,
    delta: ChunkDelta,
    finish_reason: Option<String>,
) -> Event {
    let payload = ChatCompletionChunk {
        id: id.to_string(),
        object: "chat.completion.chunk",
        created,
        model: model.to_string(),
        choices: vec![ChunkChoice {
            index: 0,
            delta,
            finish_reason,
        }],
    };
    Event::default().data(serde_json::to_string(&payload).unwrap_or_default())
}

/// Generates an OpenAI-style completion id.
fn completion_id() -> String {
    format!("chatcmpl-{}", uuid::Uuid::new_v4().simple())
}
