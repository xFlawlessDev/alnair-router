//! Helpers shared by the chat and messages handlers.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::db::repos::usage::NewUsageRecord;
use crate::model::ResolvedTarget;
use crate::state::AppState;
use crate::upstream::AttemptOutcome;
use crate::upstream::chat_backend::TokenUsage;
use crate::upstream::executor::Attempt;

/// Emits `x-router-*` headers so callers can see which tier answered.
pub fn router_headers(target: &ResolvedTarget, attempt_count: usize) -> http::HeaderMap {
    let mut headers = http::HeaderMap::new();
    if let Ok(value) = target.model.parse() {
        headers.insert("x-router-model", value);
    }
    if let Ok(value) = target.provider_type.parse() {
        headers.insert("x-router-provider", value);
    }
    if let Ok(value) = attempt_count.to_string().parse() {
        headers.insert("x-router-attempt", value);
    }
    if let Ok(value) = target.source.parse() {
        headers.insert("x-router-source", value);
    }
    headers
}

/// Records every attempt that failed before a tier succeeded.
pub async fn record_failed_attempts(
    state: &AppState,
    api_key_id: &Option<String>,
    requested_model: &str,
    attempts: &[Attempt],
) {
    for attempt in attempts.iter().filter(|a| !a.outcome.is_success()) {
        let AttemptOutcome::Failed(reason) = &attempt.outcome else {
            continue;
        };

        let record = NewUsageRecord {
            api_key_id: api_key_id.clone(),
            requested_model: requested_model.to_string(),
            resolved_provider: Some(attempt.provider_type.clone()),
            resolved_model: Some(attempt.model.clone()),
            attempt: attempt.index,
            status: "error".to_string(),
            prompt_tokens: 0,
            completion_tokens: 0,
            cached_tokens: 0,
            cost_usd: 0.0,
            latency_ms: attempt.latency_ms,
        };

        if let Err(error) = state.usage().record(record).await {
            tracing::warn!(error = %error, "failed to record usage for a failed attempt");
        }
        tracing::debug!(source = %attempt.source, reason = %reason, "recorded failed attempt");
    }
}

/// Records a streamed request's usage exactly once, when the stream terminates.
///
/// Clones share a `recorded` flag: the recorder is copied per stream event, so
/// without shared state a `Usage` chunk followed by `Done` would insert two
/// rows for a single request.
#[derive(Clone)]
pub struct StreamUsage {
    state: AppState,
    api_key_id: Option<String>,
    requested_model: String,
    provider_type: String,
    model: String,
    attempt: usize,
    latency_ms: u64,
    recorded: Arc<AtomicBool>,
}

impl StreamUsage {
    pub fn new(
        state: AppState,
        api_key_id: Option<String>,
        requested_model: String,
        target: &ResolvedTarget,
        attempt: usize,
        latency_ms: u64,
    ) -> Self {
        Self {
            state,
            api_key_id,
            requested_model,
            provider_type: target.provider_type.clone(),
            model: target.model.clone(),
            attempt,
            latency_ms,
            recorded: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Writes the usage row, ignoring any call after the first.
    pub fn record(&mut self, usage: Option<TokenUsage>, status: &str) {
        if self.recorded.swap(true, Ordering::SeqCst) {
            return;
        }

        let usage = usage.unwrap_or_default();
        let record = NewUsageRecord {
            api_key_id: self.api_key_id.clone(),
            requested_model: self.requested_model.clone(),
            resolved_provider: Some(self.provider_type.clone()),
            resolved_model: Some(self.model.clone()),
            attempt: self.attempt,
            status: status.to_string(),
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            cached_tokens: usage.cached_tokens,
            cost_usd: usage.cost_usd,
            latency_ms: self.latency_ms,
        };

        let state = self.state.clone();
        tokio::spawn(async move {
            if let Err(error) = state.usage().record(record).await {
                tracing::warn!(error = %error, "failed to record streamed usage");
            }
        });
    }
}
