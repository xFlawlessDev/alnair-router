//! `POST /v1/chat/completions`: resolve → fallback-execute → OpenAI-shaped reply.

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
use crate::protocol::openai::{
    ChatChoice, ChatChoiceMessage, ChatCompletionChunk, ChatCompletionRequest,
    ChatCompletionResponse, ChunkChoice, ChunkDelta, FunctionDelta, ToolCallDelta,
    tool_calls_payload, usage_payload,
};
use crate::state::AppState;
use crate::token_saver::Savings;
use crate::upstream::ExecutedStream;
use crate::upstream::chat_backend::{self, StreamChunk, TokenUsage, openai_finish_reason};

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
    let include_usage = request
        .stream_options
        .is_some_and(|options| options.include_usage);
    let options = request.generation_options();
    let tools = request.tools.clone();
    let messages = request.into_messages()?;
    let (messages, saver_savings) =
        crate::token_saver::apply(&state.token_saver_settings(), messages, &requested_model).await;

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
            include_usage,
            saver_savings,
            executed,
        )
        .await)
    } else {
        complete_response(state, api_key_id, requested_model, saver_savings, executed).await
    }
}

/// Non-streaming path: drain the stream, then emit a single JSON body.
async fn complete_response(
    state: AppState,
    api_key_id: Option<String>,
    requested_model: String,
    saver_savings: crate::token_saver::Savings,
    executed: ExecutedStream,
) -> Result<Response> {
    let target = executed.target.clone();
    let latency_ms = executed.latency_ms;
    let attempt_count = executed.attempts.len();

    let completion = chat_backend::collect(executed.stream).await?;

    let usage_snapshot = completion.usage.unwrap_or_default();
    let savings_totals = saver_savings.finalize(
        usage_snapshot.completion_tokens,
        tier_price(&state, &target).await,
    );
    state.metrics.record_token_totals(savings_totals);
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

/// State carried across a streaming response.
struct StreamState {
    usage: StreamUsage,
    /// Position assigned to the next `tool_calls` delta.
    tool_call_index: usize,
    /// Whether any tool call was forwarded, which drives `finish_reason`.
    has_tool_calls: bool,
    /// Latest token usage, kept so it can close the stream when requested.
    totals: Option<TokenUsage>,
}

/// Streaming path: forward chunks as OpenAI `chat.completion.chunk` SSE events.
async fn stream_response(
    state: AppState,
    api_key_id: Option<String>,
    requested_model: String,
    include_usage: bool,
    saver_savings: Savings,
    executed: ExecutedStream,
) -> Response {
    let target = executed.target.clone();
    let latency_ms = executed.latency_ms;
    let attempt_count = executed.attempts.len();
    let id = completion_id();
    let created = chrono::Utc::now().timestamp();

    // Price is resolved before `state` moves into the recorder, which owns it
    // for the life of the stream.
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

    // OpenAI opens every stream with the assistant role, so clients that key
    // off `delta.role` see it exactly once.
    let opening = vec![Ok(chunk_event(
        &id,
        created,
        &requested_model,
        ChunkDelta {
            role: Some("assistant"),
            ..ChunkDelta::default()
        },
        None,
    ))];

    let model = requested_model.clone();
    let stream = executed
        .stream
        .scan(
            StreamState {
                usage: usage_state,
                tool_call_index: 0,
                has_tool_calls: false,
                totals: None,
            },
            move |state, chunk| {
                let events = stream_events(state, chunk, &id, created, &model, include_usage);
                async move { Some(events) }
            },
        )
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

/// Translates one upstream chunk into the SSE events it produces.
fn stream_events(
    state: &mut StreamState,
    chunk: Result<StreamChunk>,
    id: &str,
    created: i64,
    model: &str,
    include_usage: bool,
) -> Vec<std::result::Result<Event, std::convert::Infallible>> {
    match chunk {
        Ok(StreamChunk::Text(text)) => vec![Ok(chunk_event(
            id,
            created,
            model,
            ChunkDelta {
                content: Some(text),
                ..ChunkDelta::default()
            },
            None,
        ))],
        Ok(StreamChunk::Thinking(text)) => vec![Ok(chunk_event(
            id,
            created,
            model,
            ChunkDelta {
                reasoning_content: Some(text),
                ..ChunkDelta::default()
            },
            None,
        ))],
        // Anthropic-only block metadata has no OpenAI wire representation.
        Ok(StreamChunk::ThinkingSignature(_)) | Ok(StreamChunk::RedactedThinking(_)) => Vec::new(),
        Ok(StreamChunk::ToolCall {
            id: call_id,
            name,
            arguments,
        }) => {
            let index = state.tool_call_index;
            state.tool_call_index += 1;
            state.has_tool_calls = true;

            vec![Ok(chunk_event(
                id,
                created,
                model,
                ChunkDelta {
                    tool_calls: Some(vec![ToolCallDelta {
                        index,
                        id: Some(call_id),
                        call_type: Some("function"),
                        function: FunctionDelta {
                            name: Some(name),
                            arguments: Some(arguments),
                        },
                    }]),
                    ..ChunkDelta::default()
                },
                None,
            ))]
        }
        Ok(StreamChunk::Usage(usage)) => {
            state.totals = Some(usage);
            state.usage.record(Some(usage), "ok");
            Vec::new()
        }
        Ok(StreamChunk::Done(reason)) => {
            let finish_reason = openai_finish_reason(reason.as_deref(), state.has_tool_calls);

            let mut events = vec![Ok(chunk_event(
                id,
                created,
                model,
                ChunkDelta::default(),
                Some(finish_reason),
            ))];

            if include_usage {
                events.push(Ok(usage_event(id, created, model, state.totals)));
            }

            events.push(Ok(Event::default().data("[DONE]")));
            events
        }
        Err(error) => {
            tracing::error!(error = %error, "stream terminated with an upstream error");
            state.usage.record(None, "error");
            vec![Ok(Event::default().data(
                json!({
                    "error": { "message": error.to_string(), "type": "upstream_error" }
                })
                .to_string(),
            ))]
        }
    }
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
        usage: None,
    };
    Event::default().data(serde_json::to_string(&payload).unwrap_or_default())
}

/// Terminal `include_usage` chunk: no choices, just the token counts.
fn usage_event(id: &str, created: i64, model: &str, usage: Option<TokenUsage>) -> Event {
    let payload = ChatCompletionChunk {
        id: id.to_string(),
        object: "chat.completion.chunk",
        created,
        model: model.to_string(),
        choices: Vec::new(),
        usage: Some(usage_payload(usage)),
    };
    Event::default().data(serde_json::to_string(&payload).unwrap_or_default())
}

/// Generates an OpenAI-style completion id.
fn completion_id() -> String {
    format!("chatcmpl-{}", uuid::Uuid::new_v4().simple())
}
