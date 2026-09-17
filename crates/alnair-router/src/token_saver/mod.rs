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
use crate::protocol::anthropic::estimate_messages;
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

    /// Human-readable name for the dashboard.
    pub fn label(self) -> &'static str {
        match self {
            Saver::Slimmer => "RTK / Slimmer",
            Saver::Headroom => "Headroom",
            Saver::Terse => "Terse",
            Saver::Caveman => "Caveman",
            Saver::Ponytail => "Ponytail",
        }
    }

    /// Which side of the request the saver works on. Input savers shrink the
    /// prompt; output savers only ask the model to write less, which is why
    /// their figures are estimates.
    pub fn side(self) -> SaverSide {
        match self {
            Saver::Slimmer | Saver::Headroom => SaverSide::Input,
            Saver::Terse | Saver::Caveman | Saver::Ponytail => SaverSide::Output,
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
        entries.sort_by_key(|(_, tokens)| std::cmp::Reverse(*tokens));
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

/// One step of the pipeline, captured for the playground.
///
/// The playground exists to *prove* the pipeline works, so it reports what each
/// step actually did rather than re-deriving it: a step that declined shows
/// `applied: false` and unchanged counts. Token counts come from the same
/// estimator the router bills against, so the numbers reconcile with
/// [`Savings`].
#[derive(Debug, Clone)]
pub struct StepTrace {
    pub saver: Saver,
    pub side: SaverSide,
    /// True when this step changed the messages.
    pub applied: bool,
    /// Prompt tokens entering the step.
    pub tokens_before: u64,
    /// Prompt tokens leaving the step.
    pub tokens_after: u64,
    /// What the step did, or why it declined (Headroom unreachable, nothing
    /// worth compressing, …).
    pub detail: String,
}

impl StepTrace {
    /// Prompt tokens added or removed by this step.
    ///
    /// Signed on purpose: the input savers remove tokens, while a directive
    /// step *adds* the instructions it injects. Hiding that behind a
    /// non-negative number would misrepresent the cost of the output savers.
    pub fn delta(&self) -> i64 {
        self.tokens_after as i64 - self.tokens_before as i64
    }
}

/// A full pipeline run, with every step recorded.
#[derive(Debug, Clone)]
pub struct PipelineRun {
    /// Messages as they arrived.
    pub before: Vec<RouterMessage>,
    /// Messages as they would be sent upstream.
    pub after: Vec<RouterMessage>,
    pub savings: Savings,
    pub steps: Vec<StepTrace>,
}

impl PipelineRun {
    /// Prompt tokens in the original request.
    pub fn tokens_before(&self) -> u64 {
        estimate_messages(&self.before)
    }

    /// Prompt tokens that would be sent upstream.
    pub fn tokens_after(&self) -> u64 {
        estimate_messages(&self.after)
    }
}

/// Runs the pipeline: slimmer → headroom → terse/caveman → ponytail.
///
/// Returns the messages to send upstream plus what the request-side savers did.
/// This never fails: every saver falls back to its own input.
pub async fn apply(
    settings: &TokenSaverSettings,
    messages: Vec<RouterMessage>,
    model: &str,
) -> (Vec<RouterMessage>, Savings) {
    let run = run(settings, messages, model).await;
    (run.after, run.savings)
}

/// Runs the pipeline and keeps a trace of every step.
///
/// [`apply`] is the hot path and throws the trace away; this is the identical
/// code with the steps retained, so the playground can never drift from what
/// production actually does.
pub async fn run(
    settings: &TokenSaverSettings,
    messages: Vec<RouterMessage>,
    model: &str,
) -> PipelineRun {
    let before = messages.clone();
    let mut messages = messages;
    let mut savings = Savings::default();
    let mut steps = Vec::new();

    if settings.is_idle() || messages.is_empty() {
        return PipelineRun {
            before,
            after: messages,
            savings,
            steps,
        };
    }

    if let Some(level) = settings.slimmer {
        let tokens_before = estimate_messages(&messages);
        let stats = slimmer::compress(&mut messages, level);
        let saved = stats.tokens_saved();
        let tokens_after = estimate_messages(&messages);
        let detail = if saved > 0 {
            let line = format!(
                "{} tool result(s) · {} · saver reports {} tokens",
                stats.hits.len(),
                stats.filter_names(),
                saved
            );
            savings.notes.push(format!(
                "rtk: {} tokens from {} tool result(s) ({})",
                saved,
                stats.hits.len(),
                stats.filter_names()
            ));
            line
        } else {
            "no compressible tool output".to_string()
        };
        savings.slimmer_tokens = saved;
        steps.push(StepTrace {
            saver: Saver::Slimmer,
            side: SaverSide::Input,
            applied: saved > 0,
            tokens_before,
            // Measured from the rewritten messages rather than derived from the
            // saver's own count, so the playground shows the real prompt delta
            // and any disagreement with the reported figure stays visible.
            tokens_after,
            detail,
        });
    }

    if let Some(headroom) = &settings.headroom {
        let tokens_before = estimate_messages(&messages);
        let step = match headroom_mod::compress(headroom, messages.clone(), model).await {
            Ok(outcome) => {
                messages = outcome.messages;
                let reported = outcome.tokens_saved;
                savings.headroom_tokens = reported;
                if reported > 0 {
                    savings
                        .notes
                        .push(format!("headroom: {reported} tokens saved"));
                }
                StepTrace {
                    saver: Saver::Headroom,
                    side: SaverSide::Input,
                    applied: reported > 0,
                    tokens_before,
                    // Measured, not taken on the proxy's word: phantom savings
                    // would otherwise be indistinguishable from real ones.
                    tokens_after: estimate_messages(&messages),
                    detail: if reported > 0 {
                        format!("proxy reports {reported} tokens removed")
                    } else {
                        "proxy reported no savings".to_string()
                    },
                }
            }
            Err(reason) => {
                // Fail-open is the whole contract: note it and carry on.
                savings.notes.push(format!("headroom: {reason}"));
                StepTrace {
                    saver: Saver::Headroom,
                    side: SaverSide::Input,
                    applied: false,
                    tokens_before,
                    tokens_after: tokens_before,
                    detail: format!("skipped, request forwarded untouched ({reason})"),
                }
            }
        };
        steps.push(step);
    }

    // Directives append to the prompt, so these steps report the tokens they
    // cost. That is the honest trade: a few prompt tokens buy a much shorter
    // completion, which `finalize` prices once the model answers.
    if let Some(output) = settings.output {
        let tokens_before = estimate_messages(&messages);
        directives::inject_output(&mut messages, output);
        savings.output = Some(output);
        steps.push(StepTrace {
            saver: output.saver(),
            side: SaverSide::Output,
            applied: true,
            tokens_before,
            tokens_after: estimate_messages(&messages),
            detail: format!(
                "injected directive · expects ~{:.0}% shorter completions",
                output.ratio() * 100.0
            ),
        });
    }

    if let Some(level) = settings.ponytail {
        let tokens_before = estimate_messages(&messages);
        directives::inject_ponytail(&mut messages, level);
        savings.ponytail = Some(level);
        steps.push(StepTrace {
            saver: Saver::Ponytail,
            side: SaverSide::Output,
            applied: true,
            tokens_before,
            tokens_after: estimate_messages(&messages),
            detail: format!(
                "stacked on the output directive · ~{:.0}% shorter completions",
                level.ratio() * 100.0
            ),
        });
    }

    PipelineRun {
        before,
        after: messages,
        savings,
        steps,
    }
}

use headroom as headroom_mod;

#[cfg(test)]
mod tests;
