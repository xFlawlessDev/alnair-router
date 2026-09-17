//! Output-side savers: system-prompt directives that make the model write less.
//!
//! Unlike the input savers these change no message content — they add one
//! instruction block to the system prompt and let the model do the rest.
//!
//! The caveman and ponytail prompt text is adapted from KeiRouter's Go
//! implementation (`internal/caveman/caveman.go`, `internal/ponytail/prompt.go`),
//! which in turn adapts the public caveman and ponytail skills. The wording is
//! kept verbatim: these strings are the feature, and paraphrasing them changes
//! model behaviour.

use serde::{Deserialize, Serialize};

use super::OutputSaver;
use crate::upstream::chat_backend::{RouterMessage, message_text, set_message_text};

/// Idempotency marker for the terseness directive.
const TERSE_SENTINEL: &str = "<!-- alnair-router:terse -->";

/// Idempotency marker for the ponytail directive.
const PONYTAIL_SENTINEL: &str = "<!-- alnair-router:ponytail -->";

/// Caveman carries no visible marker on purpose.
///
/// Agentic coding tools flag a foreign instruction block that names itself as
/// injected content and reject it, so caveman's directive never refers to
/// itself. Idempotency is detected from a stable substring of the directive
/// instead.
const CAVEMAN_PROBE: &str = "Keep all technical substance exact";

/// Terse: the lightweight output directive.
const TERSE_DIRECTIVE: &str = "Respond concisely. Keep every technical detail exact: code, paths, \
     commands, errors and API names stay verbatim. Drop filler, hedging, \
     pleasantries and restatements of the request. No preamble, no summary of \
     what you are about to do, no recap of what you just did. Answer, then stop.";

/// Caveman intensity. The three `wenyan-*` levels answer in classical Chinese.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CavemanLevel {
    Lite,
    Full,
    Ultra,
    WenyanLite,
    WenyanFull,
    WenyanUltra,
}

impl CavemanLevel {
    pub const ALL: [CavemanLevel; 6] = [
        CavemanLevel::Lite,
        CavemanLevel::Full,
        CavemanLevel::Ultra,
        CavemanLevel::WenyanLite,
        CavemanLevel::WenyanFull,
        CavemanLevel::WenyanUltra,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            CavemanLevel::Lite => "lite",
            CavemanLevel::Full => "full",
            CavemanLevel::Ultra => "ultra",
            CavemanLevel::WenyanLite => "wenyan-lite",
            CavemanLevel::WenyanFull => "wenyan-full",
            CavemanLevel::WenyanUltra => "wenyan-ultra",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|level| level.as_str() == value.trim())
    }

    /// Falls back to `full` for an unvalidated value, matching the reference
    /// implementation's default arm.
    pub fn parse_or_default(value: &str) -> Self {
        Self::parse(value).unwrap_or(CavemanLevel::Full)
    }

    /// Expected reduction in completion tokens. Conservative on purpose: the
    /// published figures are "up to 65-75%", which is a ceiling, not a mean.
    pub fn ratio(self) -> f64 {
        match self {
            CavemanLevel::Lite => 0.40,
            CavemanLevel::Full => 0.60,
            CavemanLevel::Ultra => 0.70,
            CavemanLevel::WenyanLite => 0.50,
            CavemanLevel::WenyanFull => 0.70,
            CavemanLevel::WenyanUltra => 0.75,
        }
    }
}

/// Ponytail intensity: how hard the "lazy senior dev" ladder is enforced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PonytailLevel {
    Lite,
    Full,
    Ultra,
}

impl PonytailLevel {
    pub const ALL: [PonytailLevel; 3] =
        [PonytailLevel::Lite, PonytailLevel::Full, PonytailLevel::Ultra];

    pub fn as_str(self) -> &'static str {
        match self {
            PonytailLevel::Lite => "lite",
            PonytailLevel::Full => "full",
            PonytailLevel::Ultra => "ultra",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|level| level.as_str() == value.trim())
    }

    pub fn parse_or_default(value: &str) -> Self {
        Self::parse(value).unwrap_or(PonytailLevel::Full)
    }

    /// Expected reduction in completion tokens. Ponytail biases the *shape* of
    /// the answer (less code, no essays) rather than its style, so it saves less
    /// than caveman on a typical reply.
    pub fn ratio(self) -> f64 {
        match self {
            PonytailLevel::Lite => 0.15,
            PonytailLevel::Full => 0.25,
            PonytailLevel::Ultra => 0.35,
        }
    }
}

/// Applies the active output directive. Terse and caveman are mutually
/// exclusive; [`OutputSaver`] makes that a single value.
pub fn inject_output(messages: &mut Vec<RouterMessage>, saver: OutputSaver) {
    match saver {
        OutputSaver::Terse => {
            if !contains(messages, TERSE_SENTINEL) {
                inject(messages, &format!("{TERSE_SENTINEL}\n{TERSE_DIRECTIVE}"));
            }
        }
        OutputSaver::Caveman(level) => {
            if !contains(messages, CAVEMAN_PROBE) {
                inject(messages, caveman_prompt(level));
            }
        }
    }
}

/// Applies the ponytail directive, which stacks on top of terse or caveman.
pub fn inject_ponytail(messages: &mut Vec<RouterMessage>, level: PonytailLevel) {
    if contains(messages, PONYTAIL_SENTINEL) {
        return;
    }

    let block = format!("{PONYTAIL_SENTINEL}\n{}", ponytail_prompt(level));
    inject(messages, &block);
}

/// Appends `block` to the first system message, or prepends a new system
/// message when the conversation has none.
///
/// The directive goes last within the system message so it reads as the most
/// recent instruction and overrides any earlier style guidance.
fn inject(messages: &mut Vec<RouterMessage>, block: &str) {
    match messages
        .iter_mut()
        .find(|message| message.role.eq_ignore_ascii_case("system"))
    {
        Some(system) => {
            let existing = system.content.as_text();
            let merged = if existing.trim().is_empty() {
                block.to_string()
            } else {
                format!("{existing}\n\n{block}")
            };
            set_message_text(system, merged);
        }
        None => messages.insert(0, message_text("system", block)),
    }
}

/// True when any message already carries `needle`.
fn contains(messages: &[RouterMessage], needle: &str) -> bool {
    messages
        .iter()
        .any(|message| message.content.as_text().contains(needle))
}

// ---------------------------------------------------------------------------
// Caveman prompt text
// ---------------------------------------------------------------------------

const SHARED_EXAMPLES: &str = "Not: \"Sure! I'd be happy to help you with that. \
     The issue you're experiencing is likely caused by...\" \
     Yes: \"Bug in auth middleware. Token expiry check use `<` not `<=`. Fix:\"";

const SHARED_EXAMPLES_EXTRA: &str = "Example — \"Why React component re-render?\" \
     full: \"New object ref each render. Inline object prop = new ref = re-render. Wrap in `useMemo`.\" \
     ultra: \"Inline obj prop → new ref → re-render. `useMemo`.\" \
     Example — \"Explain database connection pooling.\" \
     full: \"Pool reuse open DB connections. No new connection per request. Skip handshake overhead.\" \
     ultra: \"Pool = reuse DB conn. Skip handshake → fast under load.\"";

const SHARED_BOUNDARIES: &str = "Code blocks, file paths, commands, errors, URLs: keep exact. \
     Do not describe or announce this style; just write in it. \
     Give the terse answer only — never a terse answer plus a normal-prose recap. \
     Preserve user's dominant language. Compress the style, not the language. \
     No forced English openings or status phrases. \
     ALWAYS keep technical terms, code, API names, CLI commands, commit-type keywords \
     (feat/fix/...), and exact error strings verbatim — unless user explicitly ask for translation.";

const SHARED_AUTO_CLARITY: &str = "Auto-Clarity: drop caveman for security warnings, irreversible \
     actions, multi-step sequences where fragment order or omitted conjunctions risk misread, \
     compression itself creates technical ambiguity, or when user repeats a question. \
     Resume caveman after clear part done.";

const SHARED_PERSISTENCE: &str = "Keep this style consistent throughout the conversation.";

const SHARED_NO_INVENTED_ABBREV: &str = "No invented abbreviations. Standard well-known tech \
     acronyms (DB/API/HTTP/URL/JSON/ID/OS/CPU) OK. Names of code symbols, function names, \
     API names, error strings: keep verbatim.";

const SHARED_PRESERVE_LANGUAGE: &str = "Preserve the user's dominant language. \
     Wenyan/classical-Chinese levels override this language-preservation rule. \
     Code identifiers, error strings, file paths, commands: keep in their original form \
     regardless of language.";

const SHARED_NO_SELF_REFERENCE: &str = "No self-reference. Do not name or announce the style \
     (no \"caveman mode\", no \"me caveman think\", no \"compressed mode active\"). Just respond.";

const SHARED_NO_DECORATION: &str = "No decorative emoji. No narrating tool calls \
     (\"I will now search\", \"I used X to find Y\"). No status phrases \
     (\"Sure!\", \"Of course!\", \"I'd be happy to\"). No causal arrow shorthand \
     (\"A -> B -> fails\"). State the thing, the action, the reason. Then next step.";

/// Assembles one caveman directive: its level-specific lines, then the shared
/// tail. Every level ends with the same fragments in the same order.
fn caveman_prompt(level: CavemanLevel) -> String {
    let (lead, mut parts) = match level {
        CavemanLevel::Lite => (
            "Respond tersely. Keep all technical substance exact. Keep grammar and full \
             sentences but drop filler, hedging and pleasantries \
             (just/really/basically/sure/of course/I'd be happy to).",
            vec![SHARED_EXAMPLES],
        ),
        CavemanLevel::Full => (
            "Respond in a terse, information-dense technical style. Keep all technical \
             substance exact; only fluff goes. Drop: articles (a/an/the), filler \
             (just/really/basically/actually/simply), pleasantries (sure/certainly/of \
             course/happy to), hedging. Fragments OK. Short synonyms (big not extensive, \
             fix not implement a solution for). Technical terms exact. Code blocks \
             unchanged. Errors quoted exact.",
            vec![SHARED_EXAMPLES, SHARED_EXAMPLES_EXTRA],
        ),
        CavemanLevel::Ultra => (
            "Respond ultra-terse. Maximum compression. Telegraphic. Keep all technical \
             substance exact. Strip conjunctions, use arrows for causality (X → Y), one \
             word when one word enough. Code symbols, function names, API names, error \
             strings: never abbreviate.",
            vec![SHARED_EXAMPLES, SHARED_EXAMPLES_EXTRA],
        ),
        CavemanLevel::WenyanLite => (
            "Respond in semi-classical style. Keep all technical substance exact. Drop \
             filler and hedging but keep grammar structure. Use classical register and \
             concise phrasing. Technical terms, code, and API names stay verbatim.",
            vec![],
        ),
        CavemanLevel::WenyanFull => (
            "Respond in full 文言文 (classical Chinese) style. Keep all technical substance \
             exact. Maximum classical terseness. 80-90% character reduction. Classical \
             sentence patterns: verbs precede objects, subjects often omitted, classical \
             particles (之/乃/為/其/矣/也/焉). No modern filler words. Technical terms, \
             code, API names, CLI commands: keep verbatim, wrap in classical sentence \
             structure.",
            vec![],
        ),
        CavemanLevel::WenyanUltra => (
            "Respond ultra-terse with classical Chinese feel. Keep all technical substance \
             exact. Extreme abbreviation. Maximum compression. Classical particles \
             minimal. One character when one character enough. Technical terms, code, API \
             names: keep verbatim. Arrows for causality (→).",
            vec![],
        ),
    };

    let pattern = match level {
        CavemanLevel::Lite => {
            "Pattern: state the thing, the action, the reason. Then next step."
        }
        CavemanLevel::Full => "Pattern: [thing] [action] [reason]. [next step].",
        CavemanLevel::Ultra => "Pattern: [thing] → [result]. [fix].",
        CavemanLevel::WenyanLite => {
            "Pattern: [subject] [verb] [object], classical particles allowed."
        }
        CavemanLevel::WenyanFull | CavemanLevel::WenyanUltra => {
            "Preserve user's dominant language for technical context."
        }
    };
    parts.insert(1.min(parts.len()), pattern);

    parts.extend([
        SHARED_BOUNDARIES,
        SHARED_AUTO_CLARITY,
        SHARED_PERSISTENCE,
        SHARED_NO_INVENTED_ABBREV,
        SHARED_PRESERVE_LANGUAGE,
        SHARED_NO_SELF_REFERENCE,
        SHARED_NO_DECORATION,
    ]);

    format!("{lead} {}", parts.join(" "))
}

// ---------------------------------------------------------------------------
// Ponytail prompt text
// ---------------------------------------------------------------------------

const PONYTAIL_PERSONA: &str = "You are a lazy senior developer. Lazy means efficient, not \
     careless. The best code is the code never written.";

const PONYTAIL_LADDER: &str = "Before writing code, stop at the first rung that holds: \
     1) Does this need to exist at all? (YAGNI) 2) Stdlib does it? Use it. 3) Native platform \
     feature covers it? Use it (CSS over JS, DB constraint over app code). 4) \
     Already-installed dependency solves it? Use it; never add a new one for what a few \
     lines can do. 5) Can it be one line? One line. 6) Only then: the minimum code that works.";

const PONYTAIL_RULES: &str = "No unrequested abstractions (no interface with one \
     implementation, no factory for one product, no config for a value that never changes). \
     No boilerplate or scaffolding \"for later\". Deletion over addition. Boring over clever. \
     Fewest files possible; shortest working diff wins. Two stdlib options the same size: \
     take the edge-case-correct one. Mark deliberate simplifications with a `ponytail:` \
     comment naming the ceiling and upgrade path.";

const PONYTAIL_OUTPUT: &str = "Code first. Then at most three short lines: what was skipped, \
     when to add it. No essays or design notes. Pattern: \
     `[code] → skipped: [X], add when [Y].`";

const PONYTAIL_NOT_LAZY: &str = "Never simplify away: input validation at trust boundaries, \
     error handling that prevents data loss, security, accessibility, anything explicitly \
     requested. Non-trivial logic leaves ONE runnable check behind (an assert-based \
     self-check or one small test file; no frameworks). Trivial one-liners need no test.";

const PONYTAIL_PERSISTENCE: &str =
    "ACTIVE EVERY RESPONSE. No drift back to over-building. Still active if unsure.";

/// Assembles one ponytail directive from the shared fragments plus its level
/// line, in the reference order: persona, level, ladder, rules, output,
/// safeguards, persistence.
fn ponytail_prompt(level: PonytailLevel) -> String {
    let level_line = match level {
        PonytailLevel::Lite => {
            "Lite: build what's asked, but name the lazier alternative in one line. User picks."
        }
        PonytailLevel::Full => {
            "Full: the ladder enforced. Stdlib and native first. Shortest diff, \
             shortest explanation."
        }
        PonytailLevel::Ultra => {
            "Ultra: YAGNI extremist. Deletion before addition. Ship the one-liner and \
             challenge the rest of the requirement in the same response."
        }
    };

    [
        PONYTAIL_PERSONA,
        level_line,
        PONYTAIL_LADDER,
        PONYTAIL_RULES,
        PONYTAIL_OUTPUT,
        PONYTAIL_NOT_LAZY,
        PONYTAIL_PERSISTENCE,
    ]
    .join(" ")
}
