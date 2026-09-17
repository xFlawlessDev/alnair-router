//! RTK / Slimmer: local compression of bulky tool output before it leaves the
//! router.
//!
//! Tool results — diffs, greps, file listings, build logs — are routinely 30-50%
//! of a prompt, and almost all of the bulk is noise the model does not need.
//! This module detects what a blob of tool output *is*, applies the matching
//! filter, and keeps the original whenever the filter cannot help.
//!
//! Ported from the RTK pipeline in 9Router (`open-sse/rtk/*`), including its
//! detection order and its safety rules. Two guarantees matter:
//!
//! - **Never worse.** A filtered result that is empty or larger than the input
//!   is discarded in favour of the original.
//! - **Never fatal.** A filter that panics is caught and the input passes
//!   through untouched.
//!
//! Only messages with `role == "tool"` are touched. Both wire formats normalize
//! tool results to that role before the executor is reached, so one path covers
//! OpenAI and Anthropic alike.

mod detect;
mod filters;

use serde::{Deserialize, Serialize};

pub use detect::FilterKind;

use crate::upstream::chat_backend::{RouterMessage, message_is_text, set_message_text};

/// Smallest blob worth inspecting. Below this the filter overhead outweighs any
/// saving.
const MIN_COMPRESS_SIZE: usize = 500;

/// Largest blob considered. Beyond this, scanning costs more than it saves.
const RAW_CAP: usize = 10 * 1024 * 1024;

/// How much of a blob the detector reads to decide what it is.
const DETECT_WINDOW: usize = 1024;

/// Compression intensity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SlimmerLevel {
    /// Lossless filters only: grouping and de-duplication, no line dropping.
    Minimal,
    /// Every filter, including the lossy head/tail truncation.
    Aggressive,
}

impl SlimmerLevel {
    pub const ALL: [SlimmerLevel; 2] = [SlimmerLevel::Minimal, SlimmerLevel::Aggressive];

    pub fn as_str(self) -> &'static str {
        match self {
            SlimmerLevel::Minimal => "minimal",
            SlimmerLevel::Aggressive => "aggressive",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|level| level.as_str() == value.trim())
    }

    pub fn parse_or_default(value: &str) -> Self {
        Self::parse(value).unwrap_or(SlimmerLevel::Minimal)
    }

    /// True when filters that drop lines outright may run.
    pub fn allows_lossy(self) -> bool {
        matches!(self, SlimmerLevel::Aggressive)
    }
}

/// One filter application.
///
/// Counts are in characters, not bytes, because the token estimate is
/// character-based — a multi-byte diff would otherwise look larger than it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilterHit {
    pub kind: FilterKind,
    pub chars_in: usize,
    pub chars_out: usize,
}

/// What the slimmer did to one request's tool results.
#[derive(Debug, Clone, Default)]
pub struct SlimmerStats {
    pub bytes_before: usize,
    pub bytes_after: usize,
    pub hits: Vec<FilterHit>,
}

impl SlimmerStats {
    /// Token saving, derived from the character delta with the crate's
    /// heuristic estimator so the number lines up with the rest of the router.
    pub fn tokens_saved(&self) -> u64 {
        self.hits
            .iter()
            .map(|hit| estimate_chars(hit.chars_in).saturating_sub(estimate_chars(hit.chars_out)))
            .sum()
    }

    pub fn filter_names(&self) -> String {
        let mut names = Vec::new();
        for hit in &self.hits {
            let name = hit.kind.as_str();
            if !names.contains(&name) {
                names.push(name);
            }
        }
        names.join(", ")
    }
}

/// The character-count form of [`estimate_tokens`], without building a string.
fn estimate_chars(chars: usize) -> u64 {
    (chars as u64).div_ceil(4).max(1)
}

/// Compresses every tool result in `messages` that a filter recognizes.
pub fn compress(messages: &mut [RouterMessage], level: SlimmerLevel) -> SlimmerStats {
    let mut stats = SlimmerStats::default();

    for message in messages.iter_mut() {
        if !message.role.eq_ignore_ascii_case("tool") {
            continue;
        }

        let text = message.content.as_text();
        let bytes_in = text.len();
        stats.bytes_before += bytes_in;

        // A tool result carrying images has no textual bulk to squeeze; leave
        // the multipart structure alone rather than flattening it.
        if !message_is_text(message) {
            stats.bytes_after += bytes_in;
            continue;
        }

        match compress_one(&text, level) {
            Some((compressed, kind)) => {
                stats.bytes_after += compressed.len();
                stats.hits.push(FilterHit {
                    kind,
                    chars_in: text.chars().count(),
                    chars_out: compressed.chars().count(),
                });
                set_message_text(message, compressed);
            }
            None => stats.bytes_after += bytes_in,
        }
    }

    stats
}

/// Applies the matching filter to one blob, or `None` to keep it as it is.
fn compress_one(text: &str, level: SlimmerLevel) -> Option<(String, FilterKind)> {
    if text.len() < MIN_COMPRESS_SIZE || text.len() > RAW_CAP {
        return None;
    }

    // Error traces are the one thing a model genuinely needs verbatim, and a
    // truncated stack trace is worse than a long one.
    if looks_like_error(text) {
        return None;
    }

    let kind = detect::detect(text)?;
    if !level.allows_lossy() && kind.is_lossy() {
        return None;
    }

    // A filter that panics must not take the request down with it.
    let outcome = std::panic::catch_unwind(|| filters::apply(kind, text));
    let compressed = outcome.ok()?;

    if compressed.trim().is_empty() || compressed.len() >= text.len() {
        return None;
    }

    Some((compressed, kind))
}

/// True when a blob opens with an error signature.
///
/// `RouterMessage` has no `is_error` flag, so the reference implementation's
/// "preserve error traces" rule is expressed as a content check on the opening
/// lines.
fn looks_like_error(text: &str) -> bool {
    const SIGNATURES: [&str; 7] = [
        "Error",
        "error:",
        "error[",
        "Traceback (most recent call last)",
        "panic:",
        "fatal:",
        "FATAL",
    ];

    text.lines()
        .find(|line| !line.trim().is_empty())
        .is_some_and(|first| {
            let first = first.trim();
            SIGNATURES.iter().any(|sig| first.starts_with(sig))
        })
}

#[cfg(test)]
mod tests;
