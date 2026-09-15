//! `POST /v1/responses` — OpenAI Responses API shape over the shared executor.

use axum::Extension;
use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};

use crate::db::repos::usage::NewUsageRecord;
use crate::error::Result;
use crate::middleware::AuthenticatedKey;
use crate::protocol::openai::OpenAiMessage;
use crate::state::AppState;
use crate::upstream::chat_backend;

/// Responses API request. Input is either a plain string or a message list.
#[derive(Debug, Deserialize)]
pub struct ResponsesRequest {
    pub model: String,
    pub input: ResponsesInput,
    #[serde(default)]
    pub instructions: Option<String>,
    #[serde(default)]
    pub max_output_tokens: Option<i32>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub top_p: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ResponsesInput {
    Text(String),
    Messages(Vec<OpenAiMessage>),
}

#[derive(Debug, Serialize)]
pub struct ResponsesResponse {
    pub id: String,
    pub object: &'static str,
    pub created_at: i64,
    pub model: String,
    pub status: &'static str,
    pub output: Vec<ResponsesOutput>,
    pub usage: ResponsesUsage,
}

#[derive(Debug, Serialize)]
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
}

#[derive(Debug, Serialize)]
pub struct ResponsesContent {
    #[serde(rename = "type")]
    pub content_type: &'static str,
    pub text: String,
}

#[derive(Debug, Serialize)]
pub struct ResponsesUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

/// Handles a Responses API request.
pub async fn responses(
    State(state): State<AppState>,
    Extension(key): Extension<Option<AuthenticatedKey>>,
    Json(request): Json<ResponsesRequest>,
) -> Result<Json<ResponsesResponse>> {
    let api_key_id = key.as_ref().map(|auth| auth.key.id.clone());
    if let Some(auth) = &key {
        auth.policy.ensure_model(&request.model)?;
    }

    let mut messages = Vec::new();
    if let Some(instructions) = &request.instructions
        && !instructions.trim().is_empty()
    {
        messages.push(chat_backend::message_text("system", instructions.clone()));
    }
    messages.extend(convert_input(request.input)?);

    let options = chat_backend::GenerationOptions {
        temperature: request.temperature,
        top_p: request.top_p,
        max_tokens: request.max_output_tokens,
        ..Default::default()
    };

    let resolver = state.resolver().await?;
    let targets = resolver.resolve(&request.model)?;

    let executed = state
        .executor
        .stream(&targets, messages, None, Some(&options), false)
        .await?;

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

    state
        .usage()
        .record(NewUsageRecord {
            api_key_id,
            requested_model: request.model.clone(),
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
        .await?;

    let mut output = Vec::new();
    if !completion.content.is_empty() || completion.tool_calls.is_empty() {
        output.push(ResponsesOutput::Message {
            id: format!("msg_{}", uuid::Uuid::new_v4().simple()),
            role: "assistant",
            status: "completed",
            content: vec![ResponsesContent {
                content_type: "output_text",
                text: completion.content.clone(),
            }],
        });
    }
    for call in &completion.tool_calls {
        output.push(ResponsesOutput::FunctionCall {
            id: format!("fc_{}", uuid::Uuid::new_v4().simple()),
            call_id: call.id.clone(),
            name: call.name.clone(),
            arguments: call.arguments.clone(),
        });
    }

    Ok(Json(ResponsesResponse {
        id: format!("resp_{}", uuid::Uuid::new_v4().simple()),
        object: "response",
        created_at: chrono::Utc::now().timestamp(),
        model: request.model,
        status: "completed",
        output,
        usage: ResponsesUsage {
            input_tokens: usage.prompt_tokens,
            output_tokens: usage.completion_tokens,
            total_tokens: usage.prompt_tokens + usage.completion_tokens,
        },
    }))
}

fn convert_input(
    input: ResponsesInput,
) -> Result<Vec<crate::upstream::chat_backend::RouterMessage>> {
    match input {
        ResponsesInput::Text(text) => Ok(vec![chat_backend::message_text("user", text)]),
        ResponsesInput::Messages(messages) => messages
            .into_iter()
            .map(OpenAiMessage::into_router_message)
            .collect(),
    }
}
