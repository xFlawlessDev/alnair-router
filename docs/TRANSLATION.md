# Protocol translation layer

Design for making alnair-router's three client endpoints — `/v1/chat/completions`,
`/v1/messages`, `/v1/responses` — translate *faithfully* onto any upstream,
regardless of which protocol the client speaks and which protocol the upstream
speaks.

Status: **implemented for Fase 1 and Fase 2** (P4.1–P4.4). Native Gemini
(Fase 3) is still a separate phase. Picking this up? Read
[`TRANSLATION-HANDOVER.md`](TRANSLATION-HANDOVER.md) first — it has the code
anchors.

---

## 1. What already works, and what does not

The router is already hub-and-spoke. Every endpoint lands on one canonical
conversation type and one canonical options type, and every upstream is driven
from those:

```
POST /v1/chat/completions → src/protocol/openai.rs:320      ┐
POST /v1/messages         → src/protocol/anthropic.rs:131   ├→ RouterMessage + GenerationOptions
POST /v1/responses        → src/handlers/responses/mod.rs:301┘        ↓
                                                     src/upstream/chat_backend.rs (the only seam)
                                                                        ↓
                                                                  crates/alnair-llm
```

So "any client endpoint against any upstream" **already works**. The problem is
that the hub is much narrower than the protocols it stands between, and anything
outside the hub is dropped **silently**.

`GenerationOptions` (`src/upstream/chat_backend.rs:247`) has nine fields:

```
temperature · top_p · max_tokens · stop · seed · presence_penalty
frequency_penalty · thinking_level · web_search_options
```

The three protocols together offer roughly twenty-five. Everything missing is
discarded without a word to the client:

| Client field | Endpoint | Fate today | Evidence |
|---|---|---|---|
| `tool_choice` | chat | **dropped** — field not parsed | `protocol/openai.rs:12` |
| `tool_choice` | messages | **dropped** — field not parsed | `protocol/anthropic.rs:14` |
| `tool_choice: "required"` | responses | **400** | `handlers/responses/mod.rs:493` |
| `tool_choice: {"type":"function"}` | responses | **400** | `handlers/responses/mod.rs:503` |
| `response_format` / `text.format` | chat, responses | **dropped** | absent from both request types |
| `parallel_tool_calls` | chat, responses | **dropped** | absent; upstream sees hardcoded `true` |
| `top_k` | messages | **dropped** | absent from `MessagesRequest` and `GenerationOptions` |
| `logprobs`, `top_logprobs` | chat | **dropped** | absent |
| `metadata`, `user` | chat, responses | **dropped** | absent |
| `store`, `service_tier`, `prompt_cache_key` | chat, responses | **dropped** | absent |
| `output_config.format` | messages | **dropped** | absent |
| `include` (logprobs) | responses | **dropped** | absent |
| citations / `annotations` | all (output) | **dropped** | `StreamChunk` has no variant |

Two further consequences worth naming, because they are the ones users hit:

- **`tool_choice` is hardcoded downstream.** Even if the hub carried it, the
  provider layer would ignore it: `alnair-llm/.../responses/request.rs:102`
  writes `tool_choice: "auto"`, `alnair-llm/.../codex/request.rs:98` does the
  same plus `parallel_tool_calls: true`, and `AnthropicRequest`
  (`alnair-llm/.../anthropic/mod.rs:25`) has no `tool_choice` field at all.
- **Capability is barely modelled.** `ModelConfig`
  (`alnair-llm/src/model_config.rs:38`) knows only `supports_thinking` and
  `supports_cache_control`. There is no way to ask "can this target do
  structured output?", which is exactly what a rejection policy needs.

### 1.1 What to take from rosetta-ai, and what not to

`ZenMux/rosetta-ai` solves the same problem with N×N converters (12 of them for
four protocols). **Do not copy that shape.** A router with N endpoints and M
upstreams wants hub-and-spoke: N+M implementations, not N×M, and one place to
enforce auth, retry, cost and usage accounting.

What is worth taking is its *discipline*:

1. An explicit, written field matrix per protocol pair, so "what happens to
   `service_tier`?" has an answer rather than a shrug.
2. An explicit failure when a requested setting has no equivalent, instead of a
   silent downgrade.

That second point is the whole of §4.

---

## 2. Design principles

**P1 — One canonical hub, widened once.** New fields go into the canonical types
in `chat_backend.rs`; endpoints and providers translate to and from those. No
endpoint talks to another endpoint.

**P2 — The hub is a union, not an intersection.** A field the hub cannot hold is
a field no client can ever use. So the hub must be able to hold every field any
supported protocol can express, even when only one protocol uses it.

**P3 — Dropping a requested feature is an error; dropping an unsupported one is
not.** If the client asked for structured output and the target cannot do it,
that is a `400`. If the client asked for nothing, nothing happens.

**P4 — Capability is a property of the protocol, not the connection.** Whether
`/responses` supports `parallel_tool_calls` is decided by the wire API, not by
which vendor the URL points at. Connection-level flags stay for the two things
that genuinely vary per endpoint (`supports_thinking`,
`supports_cache_control`).

**P5 — Failover survives the rejection policy.** A combo whose tier 1 cannot do
structured output must still succeed on tier 2. Rejection happens only when *no*
tier can honour the request.

**P6 — Same 800-LOC cap as everywhere else.** `protocol/openai.rs` is 633 LOC and
`protocol/anthropic.rs` is 485. Both must be split into
`protocol/<name>/{mod,request,response}.rs` before fields are added, not after.

---

## 3. The widened hub

All of this lives in `crates/alnair-router/src/upstream/chat_backend.rs`, which
is already the only module allowed to touch `alnair_llm`. That keeps the
`vendored_llm_layer_is_imported_from_exactly_one_file` test green.

### 3.1 New canonical types

```rust
/// Who picks the tool, and which one. Canonical across all protocols.
pub enum ToolChoice {
    Auto,
    Required,
    None,
    Named { name: String },
}

/// Structured-output request, canonical across all protocols.
pub enum ResponseFormat {
    Text,
    JsonObject,
    JsonSchema { name: String, schema: serde_json::Value, strict: bool },
}

/// A citation the model attached to a span of output text.
pub struct Citation {
    pub url: String,
    pub title: Option<String>,
    /// Byte range in the accumulated output text, when the protocol reports one.
    pub start_index: Option<usize>,
    pub end_index: Option<usize>,
}
```

### 3.2 `GenerationOptions` after widening

Grouped for readability; flat in the struct to match the existing style.

```
# sampling (existing)
temperature · top_p · seed · stop · presence_penalty · frequency_penalty
# sampling (new)
top_k

# limits
max_tokens

# tools (new)
tool_choice: Option<ToolChoice>
parallel_tool_calls: Option<bool>

# structured output (new)
response_format: Option<ResponseFormat>

# reasoning (existing, unchanged)
thinking_level: Option<String>

# provider passthrough (new)
metadata: Option<serde_json::Value>       // chat `metadata` / anthropic `metadata.user_id`
user: Option<String>                      // chat `user` / anthropic `metadata.user_id`
service_tier: Option<String>
store: Option<bool>
prompt_cache_key: Option<String>
verbosity: Option<String>
logprobs: Option<bool>
top_logprobs: Option<u8>
include: Vec<String>                      // Responses `include`, e.g. logprobs

# existing
web_search_options: Option<serde_json::Value>
```

### 3.3 Output side

`StreamChunk` gains one variant:

```rust
Citation(Citation),
```

and `CompletionResponse` gains:

```rust
pub citations: Vec<Citation>,
```

The existing seven variants (`Text`, `Thinking`, `ThinkingSignature`,
`RedactedThinking`, `ToolCall`, `Usage`, `Done`) are unchanged, so no handler
breaks.

### 3.4 Provider layer changes (`crates/alnair-llm`)

The provider layer must accept and honour the new options, or the hub is
decoration. Per provider:

| Provider | Change |
|---|---|
| `openai` (chat) | forward `tool_choice`, `parallel_tool_calls`, `response_format`, `logprobs`/`top_logprobs`, `metadata`, `user`, `service_tier`, `store`, `prompt_cache_key`, `verbosity`; parse `annotations` on output |
| `openai/responses` | stop hardcoding `tool_choice: "auto"` (`request.rs:102`); forward `parallel_tool_calls`, `text.format`, `include`, `store`, `metadata`, `service_tier`; parse `response.output_text.annotation.added` |
| `anthropic` | add `tool_choice` and `metadata` to `AnthropicRequest`; map `ResponseFormat::JsonSchema` onto `output_config.format`; emit `thinking` budget from the level (already done) |
| `codex` | stop hardcoding `tool_choice`/`parallel_tool_calls` (`request.rs:98-99`) |
| `commandcode`, `codebuddy_intl` | inherit from `openai`; only the transport-specific bits stay local |

---

## 4. The degradation contract

This is the part the whole design hangs on. Three cases, three behaviours.

### Case A — client requested a feature, target cannot honour it → `400`

Shape, modelled on OpenAI's own `unsupported_parameter` error:

```json
{
  "error": {
    "type": "invalid_request_error",
    "code": "unsupported_parameter",
    "param": "response_format",
    "message": "`response_format` with a JSON schema is not supported by the `anthropic-native` upstream selected for model `claude-x`; supported targets: openai-compatible, openai-codex. Remove `response_format`, or route this request to a model whose connection supports it."
  }
}
```

The message must name: the parameter, the requested value, the target protocol
that refused, and what to do. A `400` that only says "unsupported" costs the
user the same debugging round-trip as silence.

New error variant in `src/error.rs`:

```rust
#[error("unsupported parameter `{param}`: {message}")]
UnsupportedParameter { param: String, message: String },   // → 400
```

### Case B — client requested nothing → nothing happens

No warnings, no headers. The default path stays byte-identical to today.

### Case C — upstream cannot *produce* a feature → field absent

Citations, logprobs and reasoning signatures are output-side. If the upstream
produces none, the response simply omits them. This is not an error and must not
become one: a client that tolerates a missing `annotations` array is the normal
case, and rejecting the response after a successful, billed upstream call would
be worse than the missing field.

### 4.1 Rejection happens after routing, not during parsing

The naive placement — validate in the handler, before `resolver.resolve()` — is
wrong. Two reasons:

1. The handler does not yet know the target. `response_format` support depends on
   the *selected* connection's protocol.
2. Combos. If tier 1 is `anthropic-native` and tier 2 is `openai-compatible`,
   rejecting at parse time fails a request that would have succeeded on tier 2.

So the flow becomes:

```
handler:  parse → canonical hub types          (no capability checks)
resolver: expand combo → candidate tiers
executor: filter tiers by capability(provider_type, wire_api) against hub
          ├─ ≥1 tier survives → execute as today (fallback unchanged)
          └─ 0 tiers survive  → 400 UnsupportedParameter
```

The filter is a pure function, so it is trivially testable and cannot itself
fail over incorrectly.

### 4.2 The capability matrix

New in the seam, keyed on protocol rather than vendor:

```rust
pub struct TranslationSupport {
    pub tool_choice_required: bool,
    pub tool_choice_named: bool,
    pub parallel_tool_calls: bool,
    pub response_format_json_object: bool,
    pub response_format_json_schema: bool,
    pub top_k: bool,
    pub logprobs: bool,
    pub seed: bool,
    pub metadata: bool,
    pub service_tier: bool,
    pub store: bool,
    pub prompt_cache_key: bool,
    pub verbosity: bool,
    pub citations: bool,
}

pub fn translation_support(provider_type: &ProviderType, wire_api: WireApi) -> TranslationSupport;
```

Initial matrix (✅ native · ⚠️ needs translation work · ❌ absent):

| Feature | chat | responses | anthropic | codex | command-code |
|---|---|---|---|---|---|
| `tool_choice` required | ✅ | ✅ | ✅ `any` | ✅ | ✅ |
| `tool_choice` named | ✅ | ✅ | ✅ `{type:tool}` | ✅ | ✅ |
| `parallel_tool_calls` | ✅ | ✅ | ✅ inverted flag | ✅ | ✅ |
| `response_format` json_object | ✅ | ✅ `text.format` | ❌ | ✅ | ✅ |
| `response_format` json_schema | ✅ | ✅ `text.format` | ⚠️ `output_config.format` | ✅ | ✅ |
| `top_k` | ⚠️ vendor-specific | ❌ | ✅ | ❌ | ⚠️ |
| `logprobs` | ✅ | ✅ `include` | ❌ | ✅ | ✅ |
| `seed` | ✅ | ❌ | ❌ | ❌ | ✅ |
| `metadata` / `user` | ✅ | ✅ | ✅ `metadata.user_id` | ✅ | ✅ |
| `service_tier` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `store` | ❌ | ✅ | ❌ | ❌ (forced `false`) | ❌ |
| `prompt_cache_key` | ✅ | ✅ | ❌ (implicit) | ✅ | ✅ |
| `verbosity` | ✅ | ✅ `text.verbosity` | ❌ | ✅ | ✅ |
| citations (output) | ✅ | ✅ | ✅ | ✅ | ✅ |

The matrix is deliberately conservative. A ✅ means "we write it and a test
proves the body carries it", not "the vendor's docs mention it".

### 4.3 Gemini is out of scope for this phase

Gemini has no native provider in `alnair-llm`; it is served through its
[OI]-compatibility endpoint (`src/providers/catalog.rs:445`), so Google performs
the translation. That means `tool_choice`, `response_format` and `top_k` already
work there via the `chat` column. Native Gemini — `thoughtSignature`,
`groundingMetadata`, `googleSearch`, `thinkingConfig`, and a
`/v1beta/models/{m}:generateContent` client endpoint — is a separate phase (§6,
Fase 3) and is **not** a prerequisite for this one.

---

## 5. Module layout

Both protocol modules cross 800 LOC once fields are added, so they split first.

```
src/protocol/
├── openai/
│   ├── mod.rs        re-exports, docs                          (~60)
│   ├── request.rs    ChatCompletionRequest, parsing, → hub     (~330)
│   ├── response.rs   ChatCompletionResponse, chunks, ← hub     (~330)
│   └── tests.rs
└── anthropic/
    ├── mod.rs        re-exports                                (~60)
    ├── request.rs    MessagesRequest, → hub                    (~280)
    ├── response.rs   MessagesResponse, ← hub                   (~280)
    └── tests.rs

src/handlers/responses/
├── mod.rs            handler + wiring                          (~250)
├── request.rs        ResponsesRequest, items → hub             (~260)
├── response.rs       ResponsesResponse, ← hub                  (~200)
└── stream.rs         existing SSE emitter                      (unchanged)
```

`protocol/mod.rs` keeps `pub use openai::ChatCompletionRequest;` so no call site
changes.

---

## 6. Phases

### Fase 1 — Widen the hub (prerequisite)

1. Split `protocol/openai.rs` and `protocol/anthropic.rs` per §5. **No behaviour
   change**; the existing tests must pass untouched. This lands alone so the
   diff is reviewable.
2. Add `ToolChoice`, `ResponseFormat`, `Citation`; widen `GenerationOptions`;
   add `StreamChunk::Citation` and `CompletionResponse::citations`.
3. Add `translation_support()` and `Error::UnsupportedParameter`.
4. Wire the executor's capability filter.
5. Parse and render the new fields on all three endpoints.
6. Make `alnair-llm` honour them per §3.4.

*Acceptance:* `tool_choice: "required"` from chat reaches a `chat` upstream as
`tool_choice: "required"`, reaches an `anthropic-native` upstream as
`{"type":"any"}`, and reaches a `responses` upstream as `"required"`; the same
request against a target that cannot do it returns the §4 Case A body.

### Fase 2 — Visible contract

Surface the matrix so a client can plan ahead instead of discovering a `400`:
`GET /v1/models/info` gains a per-model `translation` block, and
`docs/TRANSLATION.md` §4.2 becomes the published matrix.

### Fase 3 — Native Gemini

Request/response types for `generateContent`, a `ProviderType::GeminiNative` in
`alnair-llm`, and `/v1beta/models/{m}:generateContent` + `:countTokens` as client
endpoints. Depends on Fase 1 for the hub fields.

### Fase 4 — N×N parity for the remaining gaps

`document`/`input_file` blocks, audio input, `include` permutations, and the
Gemini-only features. Tracked against the matrix, one row at a time.

---

## 7. Test strategy

The existing suite is ~203 tests; this feature is mostly *mapping* correctness,
so the tests are mostly table-driven body assertions.

1. **Round-trip per field.** For each row of the §4.2 matrix, one test that
   builds a client request in each of the three protocols, asserts the upstream
   body carries the equivalent, and asserts the response renders back.
2. **Rejection.** One test per Case A: the target protocol that cannot honour a
   parameter produces the exact `param` and a message naming the target.
3. **Failover survives.** A two-tier combo where tier 1 cannot honour the
   parameter and tier 2 can must succeed on tier 2 — never `400`.
4. **Silence.** A request using only today's fields produces a byte-identical
   upstream body. This is the regression guard for the whole change.
5. **Split is behaviour-preserving.** Fase 1 step 1 must not change a single
   test expectation.

Provider-layer tests live in `crates/alnair-llm` (the slow half, ~23s); the
matrix and rejection tests live in `crates/alnair-router` and run in the fast
loop.

---

## 8. Risks

| Risk | Mitigation |
|---|---|
| The 800-LOC cap is breached mid-change | Split first (Fase 1 step 1), before adding fields |
| A `400` breaks a working client | Case B/C guarantee today's requests are untouched; test 4 in §7 is the guard |
| Capability matrix drifts from reality | A ✅ is only added with a body assertion; the matrix is generated from the same table the tests read |
| Combos regress | Capability filtering is a pure function applied to the tier list, not inside the executor loop |
| Wire-shape changes leak to the dashboard | `apps/web/src/types/api.ts` is mirrored in the same commit that adds a field, per the repo rule |

---

## 9. Open questions

1. **`supports_thinking` vs protocol.** `thinking` is currently a per-connection
   flag. Under P4 it is arguably a protocol property with a per-model caveat.
   Should it move into `TranslationSupport`, or stay a connection flag? Leaning
   stay — the same protocol serves both thinking and non-thinking models.
2. **`top_k` on chat.** It is vendor-specific, not part of [OI] chat
   completions. Forward it optimistically and let the existing
   "retry without the field" path (`alnair-llm/.../openai/mod.rs:137`) handle a
   rejection, or mark it ❌ for chat? Leaning ⚠️: forward, and rely on the
   existing degradation retry.
3. **Citations as a `StreamChunk` variant.** Streaming citations arrive
   interleaved with text. Emitting them as a separate chunk loses the ordering
   relative to the text they annotate, unless the chunk carries an offset —
   which is why `Citation` has `start_index`/`end_index`. Confirm every upstream
   that emits citations also emits offsets; if not, fall back to
   end-of-stream delivery.
