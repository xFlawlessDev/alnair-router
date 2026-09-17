//! Helpers shared by the chat and messages handlers.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::db::repos::usage::NewUsageRecord;
use crate::error::Result;
use crate::model::ResolvedTarget;
use crate::pricing::Price;
use crate::state::AppState;
use crate::token_saver::{Savings, TokenSaverSettings};
use crate::upstream::AttemptOutcome;
use crate::upstream::chat_backend::TokenUsage;
use crate::upstream::executor::Attempt;

/// Resolves the saver settings a playground run should use.
///
/// Overrides face the same validation the settings page does, so neither
/// playground endpoint can demonstrate a configuration the request path would
/// reject. A rejected override is the caller's mistake, so it reports as a bad
/// request rather than a server fault. Absent overrides mean "show me what
/// production does right now".
pub fn playground_settings(
    state: &AppState,
    overrides: Option<&crate::config::TokenSaverConfig>,
) -> Result<TokenSaverSettings> {
    match overrides {
        Some(config) => {
            config.validate().map_err(|error| match error {
                crate::error::Error::Config(message) => crate::error::Error::BadRequest(message),
                other => other,
            })?;
            Ok(TokenSaverSettings::from_config(config))
        }
        None => Ok(state.token_saver_settings()),
    }
}

/// Looks up the rate the winning tier is billed at, for pricing token savings.
///
/// Connections may pin a catalog id for relays whose upstream model path does
/// not match any priced key, so the override wins when present.
pub async fn tier_price(state: &AppState, target: &ResolvedTarget) -> Option<Price> {
    let key = target.pricing_model.as_deref().unwrap_or(&target.model);
    state.pricing_cache.price_for(key).await
}

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
            connection_name: Some(attempt.connection_name.clone()),
            attempt: attempt.index,
            status: "error".to_string(),
            latency_ms: attempt.latency_ms,
            ..Default::default()
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
    connection_id: String,
    connection_name: String,
    provider_type: String,
    model: String,
    attempt: usize,
    latency_ms: u64,
    /// What the request-side savers did, kept so the streamed row can report it.
    savings: Savings,
    /// Rate to price those savings at; absent leaves the token counts only.
    price: Option<Price>,
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
            connection_id: target.connection_id.clone(),
            connection_name: target.connection_name.clone(),
            provider_type: target.provider_type.clone(),
            model: target.model.clone(),
            attempt,
            latency_ms,
            savings: Savings::default(),
            price: None,
            recorded: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Attaches the pipeline's savings and the rate to price them at.
    pub fn with_savings(mut self, savings: Savings, price: Option<Price>) -> Self {
        self.savings = savings;
        self.price = price;
        self
    }

    /// Writes the usage row, ignoring any call after the first.
    pub fn record(&mut self, usage: Option<TokenUsage>, status: &str) {
        if self.recorded.swap(true, Ordering::SeqCst) {
            return;
        }

        let usage = usage.unwrap_or_default();
        let totals = self.savings.finalize(usage.completion_tokens, self.price);

        self.state.metrics.record_request(self.latency_ms);
        self.state.metrics.record_usage(
            usage.prompt_tokens,
            usage.completion_tokens,
            usage.cached_tokens,
            usage.cost_usd,
        );
        self.state.metrics.record_token_totals(totals);
        if let Some(line) = totals.describe() {
            tracing::debug!(%line, "streamed request token savings");
        }
        self.state.telemetry.record_usage(
            &self.connection_id,
            usage.prompt_tokens,
            usage.completion_tokens,
        );

        let record = NewUsageRecord {
            api_key_id: self.api_key_id.clone(),
            requested_model: self.requested_model.clone(),
            resolved_provider: Some(self.provider_type.clone()),
            resolved_model: Some(self.model.clone()),
            connection_name: Some(self.connection_name.clone()),
            attempt: self.attempt,
            status: status.to_string(),
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            cached_tokens: usage.cached_tokens,
            reasoning_tokens: usage.reasoning_tokens,
            cost_usd: usage.cost_usd,
            cost_input_usd: usage.cost_input_usd,
            cost_output_usd: usage.cost_output_usd,
            cost_reasoning_usd: usage.cost_reasoning_usd,
            latency_ms: self.latency_ms,
            ..Default::default()
        }
        .with_savings(totals);

        let state = self.state.clone();
        tokio::spawn(async move {
            if let Err(error) = state.usage().record(record).await {
                tracing::warn!(error = %error, "failed to record streamed usage");
            }
        });
    }
}
