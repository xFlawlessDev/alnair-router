# Handover — protocol translation fidelity (P4)

Briefing for whoever picks this up. Read this, then
[`docs/TRANSLATION.md`](TRANSLATION.md) (the design), then
`docs/ROADMAP.md` §P4 (the task list).

---

## State

**P4.1–P4.4 are implemented.** P4.5 (native Gemini) is untouched, as planned.

| Area | Status |
|---|---|
| `src/protocol/openai/{mod,request,response,options,tests}.rs` | split, then widened |
| `src/protocol/anthropic/{mod,request,response,tests}.rs` | split, then widened |
| `src/handlers/responses/{mod,request,response,stream}.rs` | split, then widened |
| `src/upstream/chat_backend/{mod,hub,capability,filter}.rs` | split, then widened |
| `crates/alnair-llm` providers | honour the new options; parse citations |
| `/v1/models/info` | publishes the `translation` matrix |
| `docs/ROADMAP.md` | P4.1–P4.4 ticked, P4.5 left open |

Decisions already made by the owner — do not re-litigate:

1. **Widen the hub first.** Not the Gemini endpoint, not a single-field spike.
2. **Rejection policy: `400` with an explicit error.** Not "forward + warn", not
   "forward silently". See `docs/TRANSLATION.md` §4.
3. **Keep hub-and-spoke.** Do *not* port rosetta-ai's N×N converter structure.

---

## What is where

The line numbers below are from the pre-split files and are now stale; the
module names are not.

### The hub
- `src/upstream/chat_backend/hub.rs` — `GenerationOptions`,
  `ToolChoice`, `ResponseFormat`, `Citation`, `StreamChunk`,
  `CompletionResponse`. **No provider-crate names here.**
- `src/upstream/chat_backend/mod.rs` — the only file allowed to name
  `alnair_llm`. Converts hub types → provider options and provider chunks →
  `StreamChunk`.
- `src/upstream/chat_backend/capability.rs` — `translation_support()` and the
  pure `unsupported_features()` predicate.
- `src/upstream/chat_backend/filter.rs` — `filter_targets()`, the tier filter
  the executor calls after resolution.

### Where the endpoints live now
- `src/protocol/openai/request.rs` — `ChatCompletionRequest`, including
  `tool_choice`, `response_format`, `parallel_tool_calls`, `top_k`, `metadata`,
  `user`, `service_tier`, `store`, `prompt_cache_key`, `verbosity`, `logprobs`.
- `src/protocol/openai/options.rs` — the shared `tool_choice` /
  `response_format` parsers. Both accept the flat Responses spelling and the
  nested chat spelling.
- `src/protocol/anthropic/request.rs` — `MessagesRequest`, including
  `tool_choice`, `top_k`, `metadata`, `output_config`; it translates Anthropic's
  spellings (`any`, `{type:"tool"}`, `output_config.format`) into the OpenAI
  wire shape the shared executor speaks.
- `src/handlers/responses/request.rs` — `ResponsesRequest` with `text.format`,
  `parallel_tool_calls`, `metadata`, `include`, `service_tier`, `store`,
  `prompt_cache_key`.

### Where the provider layer used to hardcode over the top
- `alnair-llm/.../openai/responses/request.rs` — `tool_choice` now follows the
  client's choice, falling back to `auto` only when none was given.
- `alnair-llm/.../codex/request.rs` — same, plus `parallel_tool_calls` follows
  the client and `include` extends rather than replaces the required encrypted
  reasoning include.
- `alnair-llm/.../anthropic/mod.rs` — `AnthropicRequest` gained `tool_choice`,
  `metadata` and `output_config`.

---

## Next concrete step

`docs/ROADMAP.md` P4.5: native Gemini (`ProviderType::GeminiNative` in
`alnair-llm`, plus `/v1beta/models/{m}:generateContent` and `:countTokens` as
client endpoints). The hub already carries everything those need.

---

## Repo rules that apply (from AGENTS.md — CI enforces these)

- `cargo fmt --all` then `cargo clippy --workspace --all-targets -- -D warnings`.
- `cargo test --workspace`. Fast loop: `cargo test -p alnair-router --lib` or
  `--test routes`. The slow half is `cargo test -p alnair-llm` (~23s).
- **No file over 800 LOC.** `lib.rs`/`main.rs` hold declarations only.
- Visibility ladder: private → `pub(super)` → `pub(crate)` → bare `pub` for real
  public API only.
- Inline `#[cfg(test)]` modules under 150 lines, otherwise a sibling `tests.rs`.
- Wire JSON is snake_case end to end; **mirror shape changes in
  `apps/web/src/types/api.ts`** in the same commit.
- Provider work belongs in `crates/alnair-llm`, never in the router. The
  one-file seam test will fail otherwise.

---

## Traps

- **Do not validate capability in the handler.** At parse time the target is
  unknown, and a combo's tier 1 may refuse while tier 2 succeeds. Filter tiers
  after `resolver.resolve()`, which is inside `Executor::stream`.
  (`docs/TRANSLATION.md` §4.1)
- **Do not break silent requests.** A request using only today's fields must
  produce a byte-identical upstream body. That is the regression guard for the
  whole change (§7 test 4); it is covered twice, in
  `crates/alnair-llm/src/model_config.rs` and in
  `crates/alnair-router/tests/routes.rs`.
- **Output-side absence is not an error.** No citations from an upstream is a
  normal response, not a `400` (§4 Case C).
- **`citation_annotations` is not a request field.** It reports whether the
  *client-facing renderer* can hand back a structured citation, and it is the
  one matrix row that must not be read as "the upstream supports citations". A
  `/v1/messages` client has no citation block type, so the Anthropic renderer
  flattens a citation into inline text; that row is `false` on purpose. Because
  no protocol lets a client ask for citations, this row is never consulted by
  `unsupported_features` and can never cause a `400`.
- **Gemini is not a blocker.** It is served through its [OI]-compatibility
  endpoint (`src/providers/catalog.rs:445`), so it inherits the `chat` column of
  the capability matrix. Native Gemini is P4.5.
- **The seam is a folder, and the guard follows it.** The module was split into
  `src/upstream/chat_backend/{mod,hub,capability,filter}.rs`, so
  `vendored_llm_layer_is_imported_from_exactly_one_file` now allows exactly
  `upstream/chat_backend/mod.rs` (plus its `tests.rs`) to name the provider
  crate. `hub.rs` and `capability.rs` deliberately import `ProviderType`/`WireApi`
  through the seam's re-exports. If you add a submodule, do the same — do not
  widen the guard.

---

## Open questions for the owner (from §9 of the design)

1. Does `supports_thinking` move into `TranslationSupport`, or stay a
   per-connection flag? Current lean: **stay** — the same protocol serves both
   thinking and non-thinking models. Unchanged by this work.
2. `top_k` on chat completions is vendor-specific. **Resolved: forward.**
   `TranslationSupport.top_k` is `true` for the chat protocol, so the router
   forwards it optimistically and the existing strip-and-retry path handles an
   upstream that refuses it. The `responses` and `codex` rows mark it `false`,
   because neither API defines the field and both reject fields they do not.
3. Citations offsets. **Resolved defensively:** `Citation.start_index` and
   `end_index` are `Option`, both parsers treat a missing offset as `None`, and
   the Anthropic renderer falls back to a trailing source marker. No upstream
   that omits offsets can fail the turn.
4. The `citations` matrix row. **Resolved: renamed to `citation_annotations` and
   set `false` for `anthropic-native`.** The original row claimed every protocol
   could deliver citations, which was wrong for the Messages client: it receives
   inline text, not an `annotations` array. Codex was also `false` in practice
   until its stream learned to read `response.output_text.annotation.added`; it
   now does, so its row is a truthful ✅.
