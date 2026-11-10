//! Canonical hub types: the union of everything the three client protocols can
//! express, in router-owned form.
//!
//! Nothing here names `alnair_llm`; the seam's `mod.rs` owns that coupling. The
//! hub lives in its own file because it is the contract every endpoint and
//! every provider translates against, and it is the part of the seam that
//! grows.
//!
//! The hub is a *union*, not an intersection: a field it cannot hold is a field
//! no client can ever use. Fields only one protocol defines (`include` for
//! Responses, `top_k` for Anthropic) are still carried here so the endpoints
//! that do define them are not second-class.

/// A single multimodal part of a message, in router-owned form.
#[derive(Debug, Clone)]
pub enum MessagePart {
    Text(String),
    ImageUrl(String),
    /// An audio, video or file part, kept as the exact wire object the client
    /// sent. OpenAI-compatible upstreams disagree on the spelling of these
    /// parts, so the router validates the shape and replays it verbatim rather
    /// than translating it into one canonical form.
    Media(serde_json::Value),
}

/// A tool call an assistant message is asking the model to run.
#[derive(Debug, Clone)]
pub struct ToolCallSpec {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// Who picks the tool, and which one. Canonical across all protocols.
///
/// Each dialect spells these differently — Anthropic's `any` and
/// `{type: "tool"}` are `required` and `Named` here — so the spelling is
/// decided in the provider layer, never at an endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolChoice {
    Auto,
    Required,
    None,
    Named { name: String },
}

/// Structured-output request, canonical across all protocols.
#[derive(Debug, Clone, PartialEq)]
pub enum ResponseFormat {
    Text,
    JsonObject,
    JsonSchema {
        name: String,
        schema: serde_json::Value,
        strict: bool,
    },
}

/// A citation the model attached to a span of output text.
///
/// The offsets are byte positions in the accumulated output text. They are
/// optional because not every upstream reports them; a `None` pair means the
/// consumer has to place the citation itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Citation {
    pub url: String,
    pub title: Option<String>,
    pub start_index: Option<usize>,
    pub end_index: Option<usize>,
}

/// Token accounting reported by a provider.
#[derive(Debug, Clone, Default, Copy)]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_tokens: u64,
    /// Reasoning tokens included in `completion_tokens`, when reported.
    pub reasoning_tokens: u64,
    /// Input-token cost in USD.
    pub cost_input_usd: f64,
    /// Output-token cost in USD (reasoning premium excluded).
    pub cost_output_usd: f64,
    /// Reasoning premium over the output rate, in USD.
    pub cost_reasoning_usd: f64,
    /// The part of `cost_input_usd` billed at the cache rate, in USD.
    pub cost_cache_usd: f64,
    /// Total cost in USD: input + output + reasoning.
    pub cost_usd: f64,
}

/// One streamed chunk from an upstream provider.
#[derive(Debug, Clone)]
pub enum StreamChunk {
    Text(String),
    Thinking(String),
    /// Signature for the preceding thinking block, replayed to Anthropic on the
    /// next turn.
    ThinkingSignature(String),
    /// Redacted thinking block payload, replayed verbatim.
    RedactedThinking(String),
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
    /// A citation for a span of the text already emitted in this turn.
    ///
    /// Output-side only: an upstream that produces none sends none, which is a
    /// normal response rather than an error.
    Citation(Citation),
    Usage(TokenUsage),
    /// Terminal chunk, carrying the provider's finish reason when it reports one.
    Done(Option<String>),
}

/// Normalizes a provider finish reason onto OpenAI's vocabulary.
///
/// Providers disagree on the terminal token (`tool_use` vs `tool_calls`,
/// `end_turn` vs `stop`), so the wire shape is decided here rather than letting
/// an Anthropic-flavoured reason leak to an OpenAI client. A completed tool call
/// always wins over a generic `stop`, because clients drive their tool loop off
/// `finish_reason`.
pub fn openai_finish_reason(reason: Option<&str>, has_tool_calls: bool) -> String {
    match reason.unwrap_or_default() {
        "tool_calls" | "tool-calls" | "tool_use" | "toolUse" => "tool_calls",
        "length" | "max_tokens" | "max-tokens" | "max_output_tokens" => "length",
        _ if has_tool_calls => "tool_calls",
        _ => "stop",
    }
    .to_string()
}

/// Normalizes a provider finish reason onto Anthropic's vocabulary.
///
/// The inverse of [`openai_finish_reason`]: an OpenAI-flavoured `length` becomes
/// `max_tokens`, and any completed tool call becomes `tool_use`.
pub fn anthropic_stop_reason(reason: Option<&str>, has_tool_calls: bool) -> String {
    match reason.unwrap_or_default() {
        "tool_calls" | "tool-calls" | "tool_use" | "toolUse" => "tool_use",
        "length" | "max_tokens" | "max-tokens" | "max_output_tokens" => "max_tokens",
        "stop_sequence" => "stop_sequence",
        _ if has_tool_calls => "tool_use",
        _ => "end_turn",
    }
    .to_string()
}

/// Completion result in router-owned types.
#[derive(Debug, Clone, Default)]
pub struct CompletionResponse {
    pub content: String,
    /// Concatenated reasoning text, when the provider emits any.
    pub thinking: Option<String>,
    /// Anthropic signature for `thinking`, replayed on the next turn.
    pub thinking_signature: Option<String>,
    /// Anthropic `redacted_thinking` payload, replayed verbatim.
    pub redacted_thinking: Option<String>,
    pub finish_reason: Option<String>,
    pub usage: Option<TokenUsage>,
    /// Tool calls the model requested, in arrival order.
    pub tool_calls: Vec<ToolCallSpec>,
    /// Citations the model attached to its output, in arrival order.
    ///
    /// Empty when the upstream produced none, which is not an error.
    pub citations: Vec<Citation>,
}

/// Generation options in router-owned form.
///
/// Grouped here for readability; flat in the struct to match the rest of the
/// codebase.
///
/// ```text
/// # sampling
/// temperature · top_p · seed · stop · presence_penalty · frequency_penalty
/// top_k
/// # limits
/// max_tokens
/// # tools
/// tool_choice · parallel_tool_calls
/// # structured output
/// response_format
/// # reasoning
/// thinking_level
/// # provider passthrough
/// metadata · user · service_tier · store · prompt_cache_key · verbosity
/// logprobs · top_logprobs · include
/// # hosted search
/// web_search_options
/// ```
#[derive(Debug, Clone, Default)]
pub struct GenerationOptions {
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    /// Include-at-most-N sampling. Anthropic and some vendors define it; the
    /// OpenAI chat API does not, so it is forwarded optimistically there and
    /// stripped by the provider's retry-without-the-field path if refused.
    pub top_k: Option<i32>,
    pub max_tokens: Option<i32>,
    pub stop: Option<Vec<String>>,
    pub seed: Option<i64>,
    pub presence_penalty: Option<f64>,
    pub frequency_penalty: Option<f64>,
    /// Extended-thinking effort, one of `minimal|low|medium|high|max`.
    pub thinking_level: Option<String>,
    /// Who picks the tool, when the client says.
    pub tool_choice: Option<ToolChoice>,
    /// Whether the model may issue tool calls concurrently.
    pub parallel_tool_calls: Option<bool>,
    /// Structured-output constraint.
    pub response_format: Option<ResponseFormat>,
    /// Client metadata: OpenAI's `metadata` object and Anthropic's
    /// `metadata.user_id` both land here.
    pub metadata: Option<serde_json::Value>,
    /// End-user identifier, kept apart from `metadata` because every protocol
    /// has a dedicated field for it.
    pub user: Option<String>,
    /// Priority tier, e.g. `auto`, `default`, `flex`.
    pub service_tier: Option<String>,
    /// Ask the upstream to retain the turn server-side.
    pub store: Option<bool>,
    /// Prompt-cache routing hint.
    pub prompt_cache_key: Option<String>,
    /// Output verbosity, `low|medium|high`.
    pub verbosity: Option<String>,
    /// Whether the client wants log probabilities.
    pub logprobs: Option<bool>,
    /// How many alternative tokens per position, 0-20.
    pub top_logprobs: Option<u8>,
    /// Responses `include`, e.g. `message.output_text.logprobs`. Carried as a
    /// list because the API treats it as a set of opt-ins.
    pub include: Vec<String>,
    /// OpenAI `web_search_options`, carried through to an OpenAI-compatible
    /// upstream untouched. The router validates the shape but never rewrites
    /// it, because each provider exposes its own knobs.
    pub web_search_options: Option<serde_json::Value>,
}

/// Maps an Anthropic `thinking.budget_tokens` value onto an effort level.
pub fn thinking_level_from_budget(budget: u64) -> &'static str {
    match budget {
        0..=1024 => "minimal",
        1025..=4096 => "low",
        4097..=8192 => "medium",
        8193..=16384 => "high",
        _ => "max",
    }
}
