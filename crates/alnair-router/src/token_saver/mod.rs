//! Deterministic token-saving pipeline.
//!
//! Every chat request passes through [`apply`] exactly once, before it reaches a
//! provider, so the savings are the same whichever upstream ends up serving the
//! request. Five savers are available:
//!
//! | Saver | Side | What it does |
//! |---|---|---|
//! | [`slimmer`] (RTK) | input | compresses bulky tool output locally |
//! | [`headroom`] | input | asks an external Headroom proxy for deeper compression |
//! | [`directives`] terse | output | injects a concise-output directive |
//! | [`directives`] caveman | output | injects a stronger terseness directive |
//! | [`directives`] ponytail | output | injects a "lazy senior dev" directive |
//!
//! Terse and caveman both write a system directive, so they are mutually
//! exclusive ([`OutputSaver`] makes that unrepresentable); ponytail stacks on
//! top of either.
//!
//! Two properties hold for every step:
//!
//! - **Deterministic.** No randomness, no clocks, no per-request state. The same
//!   input and settings always produce the same output.
//! - **Fail-open.** A saver that cannot do its job returns the exact input it
//!   was given. Nothing here can fail a request.

mod directives;
mod headroom;
mod slimmer;

pub use directives::{CavemanLevel, PonytailLevel};
pub use headroom::{HeadroomProbeResult, probe_headroom};
pub use slimmer::{SlimmerLevel, SlimmerStats};

use serde::{Deserialize, Serialize};

use crate::config::TokenSaverConfig;
use crate::pricing::Price;
use crate::upstream::chat_backend::RouterMessage;

/// Which saver contributed a saving. Used by the usage columns, the Prometheus
/// counters and the dashboard breakdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Saver {
    Slimmer,
    Headroom,
    Terse,
    Caveman,
    Ponytail,
}

impl Saver {
    pub fn as_str(self) -> &'static str {
        match self {
            Saver::Slimmer => "rtk",
            Saver::Headroom => "headroom",
            Saver::Terse => "terse",
            Saver::Caveman => "caveman",
            Saver::Ponytail => "ponytail",
        }
    }
}

/// Side of the request a saver works on. Input savings are measured; output
/// savings are estimated, because the model simply writes fewer tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaverSide {
    Input,
    Output,
}

impl SaverSide {
    /// True when the numbers for this side are estimates rather than
    /// measurements. The dashboard labels these, and the API reports it.
    pub fn is_estimated(self) -> bool {
        matches!(self, SaverSide::Output)
    }
}

/// Settings for the external Headroom compression proxy.
#[derive(Debug, Clone, PartialEq)]
pub struct HeadroomSettings {
    /// Base URL of the proxy, e.g. `http://localhost:8787`.
    pub url: String,
    pub timeout_ms: u64,
}

/// The output-side directive that is active. Terse and caveman are mutually
/// exclusive, so this is one slot rather than two booleans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputSaver {
    Terse,
    Caveman(CavemanLevel),
}

impl OutputSaver {
    /// Fraction of the completion the directive is expected to remove. These are
    /// deliberate, conservative estimates mirroring the published figures for
    /// each saver (terse ~40%, caveman 65-75%), never measurements.
    pub fn ratio(self) -> f64 {
        match self {
            OutputSaver::Terse => 0.40,
            OutputSaver::Caveman(level) => level.ratio(),
        }
    }

    pub fn saver(self) -> Saver {
        match self {
            OutputSaver::Terse => Saver::Terse,
            OutputSaver::Caveman(_) => Saver::Caveman,
        }
    }
}

/// Pre-parsed pipeline settings, built once per configuration change so the
/// request path never parses a level string.
///
/// `None` means the saver is off. Config validation rejects unknown levels, so
/// parsing here always succeeds for a validated configuration.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TokenSaverSettings {
    /// Fast path: every saver off means the pipeline is a no-op.
    pub slimmer: Option<SlimmerLevel>,
    pub headroom: Option<HeadroomSettings>,
    pub output: Option<OutputSaver>,
    pub ponytail: Option<PonytailLevel>,
}

impl TokenSaverSettings {
    pub fn from_config(config: &TokenSaverConfig) -> Self {
        // Caveman wins when both are set: validation rejects that combination
        // outright, so this only decides what an unvalidated config does.
        let output = if config.caveman_enabled {
            Some(OutputSaver::Caveman(CavemanLevel::parse_or_default(
                &config.caveman_level,
            )))
        } else if config.terse_enabled {
            Some(OutputSaver::Terse)
        } else {
            None
        };

        Self {
            slimmer: config
                .slimmer_enabled
                .then(|| SlimmerLevel::parse_or_default(&config.slimmer_level)),
            headroom: config.headroom_enabled.then(|| HeadroomSettings {
                url: config.headroom_url.trim().trim_end_matches('/').to_string(),
                timeout_ms: config.headroom_timeout_ms.max(1),
            }),
            output,
            ponytail: config
                .ponytail_enabled
                .then(|| PonytailLevel::parse_or_default(&config.ponytail_level)),
        }
    }

    /// True when no saver is enabled, so [`apply`] can return immediately.
    pub fn is_idle(&self) -> bool {
        self.slimmer.is_none()
            && self.headroom.is_none()
            && self.output.is_none()
            && self.ponytail.is_none()
    }
}

/// What the request-side of the pipeline managed to do.
///
/// Input numbers are measured here; output savers only record *that* they were
/// applied, because how much they save is not knowable until the model answers.
#[derive(Debug, Clone, Default)]
pub struct Savings {
    pub slimmer_tokens: u64,
    pub headroom_tokens: u64,
    /// Directive injected into the system prompt, if any.
    pub output: Option<OutputSaver>,
    pub ponytail: Option<PonytailLevel>,
    /// Operator-facing explanations for anything that did not happen as hoped
    /// (Headroom unreachable, phantom savings rejected, …).
    pub notes: Vec<String>,
}

impl Savings {
    /// True when the request-side pipeline changed nothing at all.
    pub fn is_empty(&self) -> bool {
        self.slimmer_tokens == 0
            && self.headroom_tokens == 0
            && self.output.is_none()
            && self.ponytail.is_none()
    }

    /// Closes the calculation once the provider reported its completion token
    /// count, applying `price` (the winning tier's catalog rate) to both sides.
    pub fn finalize(&self, completion_tokens: u64, price: Option<Price>) -> SavingsTotals {
        let mut totals = SavingsTotals {
            saved_rtk_tokens: self.slimmer_tokens,
            saved_headroom_tokens: self.headroom_tokens,
            ..SavingsTotals::default()
        };

        let mut estimated_completion = 0u64;
        if let Some(output) = self.output {
            let saved = estimate(self.ratio_share(output.ratio(), completion_tokens));
            estimated_completion += saved;
            match output.saver() {
                Saver::Terse => totals.saved_terse_tokens = saved,
                Saver::Caveman => totals.saved_caveman_tokens = saved,
                _ => {}
            }
        }
        if let Some(ponytail) = self.ponytail {
            let saved = estimate(self.ratio_share(ponytail.ratio(), completion_tokens));
            estimated_completion += saved;
            totals.saved_ponytail_tokens = saved;
        }

        if let Some(price) = price {
            let input_saved = (self.slimmer_tokens + self.headroom_tokens) as f64;
            totals.saved_cost_usd = input_saved * price.input_per_million_usd / 1_000_000.0
                + estimated_completion as f64 * price.output_per_million_usd / 1_000_000.0;
        }

        totals
    }

    /// Splits the completion between the stacked savers.
    ///
    /// Ponytail layers on top of terse/caveman, so it is assigned the remainder
    /// rather than a share of the original: applied to 100 tokens, caveman
    /// (60%) leaves 40 and ponytail (25%) takes 10 of those.
    fn ratio_share(&self, ratio: f64, completion_tokens: u64) -> f64 {
        if self.ponytail.is_some() && self.output.is_some() {
            let remaining = completion_tokens as f64 * (1.0 - self.output_ratio());
            return remaining * ratio;
        }
        completion_tokens as f64 * ratio
    }

    fn output_ratio(&self) -> f64 {
        self.output.map(OutputSaver::ratio).unwrap_or(0.0)
    }
}

/// Rounds an estimated token count; never negative.
fn estimate(value: f64) -> u64 {
    value.max(0.0).round() as u64
}

/// The final saving figures for one request.
///
/// `saved_rtk_tokens` and `saved_headroom_tokens` are measured; the three
/// directive columns are estimates. [`Self::inputs_are_estimated`] reports that
/// split so the API and the dashboard can label it instead of quietly
/// presenting a guess as a measurement.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct SavingsTotals {
    pub saved_rtk_tokens: u64,
    pub saved_headroom_tokens: u64,
    pub saved_terse_tokens: u64,
    pub saved_caveman_tokens: u64,
    pub saved_ponytail_tokens: u64,
    pub saved_cost_usd: f64,
}

impl SavingsTotals {
    /// Every saved token, measured and estimated together.
    pub fn total_tokens(&self) -> u64 {
        self.saved_rtk_tokens
            + self.saved_headroom_tokens
            + self.saved_terse_tokens
            + self.saved_caveman_tokens
            + self.saved_ponytail_tokens
    }

    pub fn is_empty(&self) -> bool {
        self.total_tokens() == 0 && self.saved_cost_usd == 0.0
    }

    /// Savers that contributed, most significant first, for logs and the
    /// dashboard breakdown.
    pub fn contributions(&self) -> Vec<(Saver, u64)> {
        let mut entries = vec![
            (Saver::Slimmer, self.saved_rtk_tokens),
            (Saver::Headroom, self.saved_headroom_tokens),
            (Saver::Terse, self.saved_terse_tokens),
            (Saver::Caveman, self.saved_caveman_tokens),
            (Saver::Ponytail, self.saved_ponytail_tokens),
        ];
        entries.retain(|(_, tokens)| *tokens > 0);
        entries.sort_by(|a, b| b.1.cmp(&a.1));
        entries
    }

    /// One human-readable line for the activity feed.
    pub fn describe(&self) -> Option<String> {
        let contributions = self.contributions();
        if contributions.is_empty() && self.saved_cost_usd == 0.0 {
            return None;
        }

        let measured = self.saved_rtk_tokens + self.saved_headroom_tokens;
        let estimated = self.total_tokens() - measured;
        let savers = contributions
            .iter()
            .map(|(saver, _)| saver.as_str())
            .collect::<Vec<_>>()
            .join(", ");

        let mut parts = Vec::new();
        if measured > 0 {
            parts.push(format!("{measured} prompt"));
        }
        if estimated > 0 {
            parts.push(format!("{estimated} output (est.)"));
        }

        Some(format!(
            "token saver: {} ≈ ${:.4} saved — {savers}",
            parts.join(" + "),
            self.saved_cost_usd
        ))
    }
}

/// Runs the pipeline: slimmer → headroom → terse/caveman → ponytail.
///
/// Returns the messages to send upstream plus what the request-side savers did.
/// This never fails: every saver falls back to its own input.
pub async fn apply(
    settings: &TokenSaverSettings,
    mut messages: Vec<RouterMessage>,
    model: &str,
) -> (Vec<RouterMessage>, Savings) {
    let mut savings = Savings::default();

    if settings.is_idle() || messages.is_empty() {
        return (messages, savings);
    }

    if let Some(level) = settings.slimmer {
        let stats = slimmer::compress(&mut messages, level);
        if stats.tokens_saved() > 0 {
            savings.notes.push(format!(
                "rtk: {} tokens from {} tool result(s) ({})",
                stats.tokens_saved(),
                stats.hits.len(),
                stats.filter_names()
            ));
        }
        savings.slimmer_tokens = stats.tokens_saved();
    }

    if let Some(headroom) = &settings.headroom {
        match headroom_mod::compress(headroom, messages.clone(), model).await {
            Ok(outcome) => {
                messages = outcome.messages;
                savings.headroom_tokens = outcome.tokens_saved;
                if outcome.tokens_saved > 0 {
                    savings.notes.push(format!(
                        "headroom: {} tokens saved",
                        outcome.tokens_saved
                    ));
                }
            }
            Err(reason) => {
                // Fail-open is the whole contract: note it and carry on.
                savings.notes.push(format!("headroom: {reason}"));
            }
        }
    }

    if let Some(output) = settings.output {
        directives::inject_output(&mut messages, output);
        savings.output = Some(output);
    }
    if let Some(level) = settings.ponytail {
        directives::inject_ponytail(&mut messages, level);
        savings.ponytail = Some(level);
    }

    (messages, savings)
}

use headroom as headroom_mod;

#[cfg(test)]
mod tests;
