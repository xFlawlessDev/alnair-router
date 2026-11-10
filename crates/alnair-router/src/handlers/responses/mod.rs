//! `POST /v1/responses` — OpenAI Responses API shape over the shared executor.
//!
//! The request is translated into the same router messages the chat handler
//! uses, so combos, fallback, token saving and usage accounting all apply. The
//! reply is either one JSON object or the Responses SSE event sequence, chosen
//! by `stream`.

mod request;
mod response;
mod stream;

use axum::Extension;
use axum::Json;
use axum::extract::State;
use axum::response::{IntoResponse, Response};

use crate::db::repos::usage::NewUsageRecord;
use crate::error::Result;
use crate::handlers::shared::tier_price;
use crate::middleware::AuthenticatedKey;
use crate::state::AppState;
use crate::upstream::chat_backend;

use request::{build_messages, generation_options, resolve_tools};
use response::completion_payload;

pub use request::ResponsesRequest;

/// Handles a Responses API request, streaming or not based on `stream`.
pub async fn responses(
    State(state): State<AppState>,
    Extension(key): Extension<Option<AuthenticatedKey>>,
    Json(request): Json<ResponsesRequest>,
) -> Result<Response> {
    let token_reservation = key.as_ref().and_then(|auth| auth.token_reservation.clone());
    let api_key_id = key.as_ref().map(|auth| auth.key.id.clone());
    if let Some(auth) = &key {
        auth.policy.ensure_model(&request.model)?;
    }

    let messages = build_messages(&request)?;
    let (messages, saver_savings) =
        crate::token_saver::apply(&state.token_saver_settings(), messages, &request.model).await;
    let (tools, _) = resolve_tools(&request)?;

    let options = generation_options(&request)?;

    let resolver = state.resolver().await?;
    let targets = resolver.resolve(&request.model)?;

    let executed = state
        .executor
        .stream(&targets, messages, tools, Some(&options), request.stream)
        .await?;

    if request.stream {
        return Ok(stream::responses_stream(
            state,
            api_key_id,
            request.model.clone(),
            token_reservation,
            saver_savings,
            executed,
        )
        .await);
    }

    complete_response(
        state,
        api_key_id,
        request.model,
        token_reservation,
        saver_savings,
        executed,
    )
    .await
}

/// Non-streaming path: drain the stream, then emit a single JSON body.
async fn complete_response(
    state: AppState,
    api_key_id: Option<String>,
    requested_model: String,
    token_reservation: Option<crate::token_rate::SharedReservation>,
    saver_savings: crate::token_saver::Savings,
    executed: crate::upstream::ExecutedStream,
) -> Result<Response> {
    let target = executed.target.clone();
    let latency_ms = executed.latency_ms;
    let attempt_count = executed.attempts.len();

    let completion = chat_backend::collect(executed.stream).await?;
    crate::token_rate::reconcile(&token_reservation, completion.usage);
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
                cost_cache_usd: usage.cost_cache_usd,
                latency_ms,
                ..Default::default()
            }
            .with_savings(savings_totals),
        )
        .await?;

    let response = completion_payload(&requested_model, &completion, "completed");

    Ok((
        crate::handlers::shared::router_headers(&target, attempt_count),
        Json(response),
    )
        .into_response())
}

#[cfg(test)]
mod tests;
