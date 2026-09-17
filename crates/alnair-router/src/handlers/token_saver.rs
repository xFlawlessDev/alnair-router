//! Token-saver playground: run the real pipeline against a sample request.
//!
//! This exists to *prove* the savers work. It calls the same
//! [`crate::token_saver::run`] the request path uses, so what it shows is what
//! actually happens to a live request — there is no second implementation to
//! drift out of sync.

use axum::Json;
use axum::extract::State;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};

use crate::config::TokenSaverConfig;
use crate::error::Result;
use crate::state::AppState;
use crate::token_saver::{SaverSide, SavingsTotals, TokenSaverSettings};
use crate::upstream::chat_backend::RouterMessage;

/// Model name handed to the pipeline when the request names none. Headroom uses
/// it to pick a tokenizer; a generic id is enough for the local savers.
const DEFAULT_MODEL: &str = "gpt-4o";

/// `POST /api/token-saver/playground` body.
#[derive(Debug, Deserialize)]
pub struct PlaygroundRequest {
    /// Conversation to run through the pipeline. The savers act on tool
    /// results, so include them as `role: "tool"` messages to see compression.
    pub messages: Vec<RouterMessage>,
    #[serde(default)]
    pub model: Option<String>,
    /// Completion length to assume, so the output directives can be priced.
    /// Absent means no guess is made and no cost is reported.
    #[serde(default)]
    pub assumed_completion_tokens: Option<u64>,
    /// Per-run saver settings. Absent uses the live configuration, so the
    /// default view is exactly what production does right now.
    #[serde(default)]
    pub overrides: Option<TokenSaverConfig>,
}

/// One pipeline step, as reported to the dashboard.
#[derive(Debug, Serialize)]
pub struct PlaygroundStep {
    /// Stable id, e.g. `rtk`.
    pub saver: &'static str,
    /// Display name, e.g. `RTK / Slimmer`.
    pub label: &'static str,
    /// `input` (shrinks the prompt) or `output` (asks for a shorter answer).
    pub side: &'static str,
    /// False when the step declined; the counts below are then unchanged.
    pub applied: bool,
    pub tokens_before: u64,
    pub tokens_after: u64,
    /// Signed: negative for input savers, positive for directives, which add
    /// the instruction text they inject.
    pub delta: i64,
    /// What the step did, or why it declined.
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct PlaygroundResponse {
    pub model: String,
    /// False when every saver is off, so the request would be forwarded as-is.
    pub active: bool,
    pub tokens_before: u64,
    pub tokens_after: u64,
    /// `before - after` across the prompt.
    pub prompt_tokens_saved: i64,
    pub steps: Vec<PlaygroundStep>,
    /// Messages exactly as submitted.
    pub before: Vec<RouterMessage>,
    /// Messages exactly as they would be sent upstream.
    pub after: Vec<RouterMessage>,
    /// Operator-facing notes, including why a saver declined.
    pub notes: Vec<String>,
    /// Savings including the output-side estimate. `None` when no completion
    /// length was supplied, because the output savers cannot be priced without
    /// one and inventing a number would not be proof of anything.
    pub totals: Option<SavingsTotals>,
}

/// `POST /api/token-saver/playground` — run the real pipeline on a sample.
pub async fn playground(
    State(state): State<AppState>,
    Json(request): Json<PlaygroundRequest>,
) -> Result<impl IntoResponse> {
    // No overrides means "show me what production does right now". Overrides
    // face the same validation the settings page does, so the playground cannot
    // demonstrate a configuration the request path would reject. A rejected
    // override is the caller's mistake, so it reports as a bad request rather
    // than a server fault.
    let settings = match &request.overrides {
        Some(config) => {
            config.validate().map_err(|error| match error {
                crate::error::Error::Config(message) => crate::error::Error::BadRequest(message),
                other => other,
            })?;
            TokenSaverSettings::from_config(config)
        }
        None => state.token_saver_settings(),
    };

    let model = request
        .model
        .map(|model| model.trim().to_string())
        .filter(|model| !model.is_empty())
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());

    let active = !settings.is_idle();
    let run = crate::token_saver::run(&settings, request.messages, &model).await;

    // A catalog miss only suppresses the cost figure; the token counts stand.
    let totals = match request.assumed_completion_tokens {
        Some(completion) => {
            let price = state.pricing_cache.price_for(&model).await;
            Some(run.savings.finalize(completion, price))
        }
        None => None,
    };

    let steps: Vec<PlaygroundStep> = run
        .steps
        .iter()
        .map(|step| PlaygroundStep {
            saver: step.saver.as_str(),
            label: step.saver.label(),
            side: match step.side {
                SaverSide::Input => "input",
                SaverSide::Output => "output",
            },
            applied: step.applied,
            tokens_before: step.tokens_before,
            tokens_after: step.tokens_after,
            delta: step.delta(),
            detail: step.detail.clone(),
        })
        .collect();

    let tokens_before = run.tokens_before();
    let tokens_after = run.tokens_after();

    Ok(Json(PlaygroundResponse {
        model,
        active,
        tokens_before,
        tokens_after,
        prompt_tokens_saved: tokens_before as i64 - tokens_after as i64,
        steps,
        before: run.before,
        after: run.after,
        notes: run.savings.notes,
        totals,
    }))
}
