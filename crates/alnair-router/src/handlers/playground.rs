//! Playground chat: a real streamed completion, driven from the dashboard.
//!
//! This is the chat half of the playground. It runs the same resolver, executor
//! and token-saving pipeline a client request does, so the transcript on screen
//! is evidence rather than a simulation, and streams the answer back as it
//! arrives. The admin guard already authenticated the caller, so no router-issued
//! client key is needed — the playground keeps working with
//! `server.require_api_key` on.

use axum::Json;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures::StreamExt;
use serde::Serialize;
use serde_json::json;

use crate::config::TokenSaverConfig;
use crate::error::{Error, Result};
use crate::handlers::shared::{
    StreamUsage, playground_settings, record_failed_attempts, tier_price,
};
use crate::protocol::openai::OpenAiMessage;
use crate::state::AppState;
use crate::token_saver::{Savings, SavingsTotals};
use crate::upstream::chat_backend::{GenerationOptions, StreamChunk, TokenUsage};
use crate::upstream::executor::ExecutedStream;

/// `POST /api/playground/chat` body.
#[derive(Debug, serde::Deserialize)]
pub struct PlaygroundChatRequest {
    /// Model reference to resolve: an alias prefix, combo name, or `prefix/model`.
    pub model: String,
    /// Conversation so far. The OpenAI shape is reused so `content` may be a
    /// string or multimodal parts, exactly as a client would send it.
    pub messages: Vec<OpenAiMessage>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub max_tokens: Option<i32>,
    /// Per-run saver settings. Absent runs the live configuration, so the
    /// default view is exactly what production does right now.
    #[serde(default)]
    pub overrides: Option<TokenSaverConfig>,
}

/// The routing decision, sent once before any tokens arrive.
#[derive(Debug, Serialize)]
struct RouterFrame {
    #[serde(rename = "type")]
    kind: &'static str,
    model: String,
    source: String,
    provider_type: String,
    /// Tiers attempted before one answered.
    attempts: usize,
}

/// Token and cost totals for the completion that just finished.
#[derive(Debug, Serialize)]
struct UsageFrame {
    #[serde(rename = "type")]
    kind: &'static str,
    prompt_tokens: u64,
    completion_tokens: u64,
    cost_usd: f64,
    /// What the request-side pipeline removed, priced where a rate was known.
    savings: SavingsTotals,
}

/// A failure after the stream opened. The status is already 200 by then, so the
/// problem travels in-band rather than as an HTTP error.
#[derive(Debug, Serialize)]
struct ErrorFrame {
    #[serde(rename = "type")]
    kind: &'static str,
    message: String,
}

/// `POST /api/playground/chat` — stream a real completion to the dashboard.
pub async fn chat(
    State(state): State<AppState>,
    Json(request): Json<PlaygroundChatRequest>,
) -> Result<Response> {
    // Overrides face the same validation the settings page does, so the
    // playground cannot demonstrate a configuration the request path would
    // reject. Empty messages are the caller's mistake, not a server fault.
    let settings = playground_settings(&state, request.overrides.as_ref())?;

    let model = request.model.trim().to_string();
    if model.is_empty() {
        return Err(Error::BadRequest("model is required".to_string()));
    }
    if request.messages.is_empty() {
        return Err(Error::BadRequest("messages must not be empty".to_string()));
    }

    let messages = request
        .messages
        .into_iter()
        .map(OpenAiMessage::into_router_message)
        .collect::<Result<Vec<_>>>()?;

    let (messages, saver_savings) = crate::token_saver::apply(&settings, messages, &model).await;

    let resolver = state.resolver().await?;
    let targets = resolver.resolve(&model)?;

    let options = GenerationOptions {
        temperature: request.temperature,
        max_tokens: request.max_tokens,
        ..Default::default()
    };

    let executed = state
        .executor
        .stream(&targets, messages, None, Some(&options), true)
        .await?;

    // Failures that happened before a tier succeeded are logged immediately.
    // The playground carries no client key, so the rows record `api_key_id: None`.
    record_failed_attempts(&state, &None, &model, &executed.attempts).await;

    Ok(stream_response(state, model, saver_savings, executed).await)
}

/// Turns the executed stream into the playground's SSE frames.
async fn stream_response(
    state: AppState,
    requested_model: String,
    saver_savings: Savings,
    executed: ExecutedStream,
) -> Response {
    let target = executed.target.clone();
    let latency_ms = executed.latency_ms;
    let attempt_count = executed.attempts.len();

    // Price is resolved before `state` moves into the recorder, which owns it for
    // the life of the stream.
    let price = tier_price(&state, &target).await;

    let usage_recorder = StreamUsage::new(
        state,
        None,
        requested_model.clone(),
        &target,
        attempt_count,
        latency_ms,
    )
    .with_savings(saver_savings.clone(), price);

    let opening = Event::default().data(
        serde_json::to_string(&RouterFrame {
            kind: "router",
            model: target.model.clone(),
            source: target.source.clone(),
            provider_type: target.provider_type.clone(),
            attempts: attempt_count,
        })
        .unwrap_or_default(),
    );

    let stream = executed
        .stream
        .scan(
            StreamState {
                usage: usage_recorder,
                savings: saver_savings,
                price,
            },
            move |state, chunk| {
                let events = frame_events(state, chunk);
                async move { Some(events) }
            },
        )
        .flat_map(futures::stream::iter);

    let stream = futures::stream::once(async move { Ok(opening) }).chain(stream);

    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

/// State carried across the streamed response.
struct StreamState {
    usage: StreamUsage,
    savings: Savings,
    price: Option<crate::pricing::Price>,
}

/// Translates one upstream chunk into the frames it produces.
fn frame_events(
    state: &mut StreamState,
    chunk: Result<StreamChunk>,
) -> Vec<std::result::Result<Event, std::convert::Infallible>> {
    match chunk {
        Ok(StreamChunk::Text(text)) => vec![Ok(data_frame(&json!({
            "type": "delta",
            "text": text,
        })))],
        // Reasoning is shown as its own kind so the transcript can style it
        // apart from the answer without losing it.
        Ok(StreamChunk::Thinking(text)) => vec![Ok(data_frame(&json!({
            "type": "thinking",
            "text": text,
        })))],
        Ok(StreamChunk::ThinkingSignature(_)) | Ok(StreamChunk::RedactedThinking(_)) => Vec::new(),
        Ok(StreamChunk::ToolCall {
            id,
            name,
            arguments,
        }) => vec![Ok(data_frame(&json!({
            "type": "tool_call",
            "id": id,
            "name": name,
            "arguments": arguments,
        })))],
        Ok(StreamChunk::Usage(usage)) => {
            state.usage.record(Some(usage), "ok");
            vec![Ok(usage_frame(&usage, &state.savings, state.price))]
        }
        Ok(StreamChunk::Done(_)) => {
            // Providers that report no usage still need a row, and the recorder
            // ignores this second call when a usage chunk already landed.
            state.usage.record(None, "ok");
            vec![Ok(Event::default().data("[DONE]"))]
        }
        Err(error) => {
            tracing::warn!(error = %error, "playground chat terminated with an upstream error");
            state.usage.record(None, "error");
            vec![
                Ok(data_frame(&ErrorFrame {
                    kind: "error",
                    message: error.to_string(),
                })),
                Ok(Event::default().data("[DONE]")),
            ]
        }
    }
}

/// Builds the terminal frame carrying tokens, cost and pipeline savings.
fn usage_frame(
    usage: &TokenUsage,
    savings: &Savings,
    price: Option<crate::pricing::Price>,
) -> Event {
    data_frame(&UsageFrame {
        kind: "usage",
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
        cost_usd: usage.cost_usd,
        savings: savings.finalize(usage.completion_tokens, price),
    })
}

/// Serializes one payload into a plain `data:` frame.
fn data_frame<T: Serialize>(payload: &T) -> Event {
    Event::default().data(serde_json::to_string(payload).unwrap_or_default())
}
