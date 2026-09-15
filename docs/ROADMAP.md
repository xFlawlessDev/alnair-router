# alnair-router — Roadmap

Forward plan for the crate. Status is measured against the code as of
2026-09-15.

**Legend:** `[ ]` not started · `[~]` partially done · `[x]` done

**P0 is closed** (2026-09-15): credentials are encrypted at rest, the admin API
enforces `server.admin_token` when configured (and refuses non-loopback binds
without one unless explicitly overridden), `/v1/web/fetch` validates and pins
every hop, the dead `[queue]` config is gone, and the retry contract is pinned
and configurable. The router is still developer-facing: no rate limiting, no
concurrency limiting, and the loopback admin API stays open by design.

---

## Where things stand

Working today, verified by the test suite and a live smoke test:

- [x] Prefixed alias resolution, combo expansion (cycle detection, depth cap 8), tier numbering
- [x] Ordered fallback with first-chunk peek — never switches mid-stream after bytes are emitted
- [x] OpenAI-compatible: `/v1/chat/completions` (SSE + JSON), `/v1/responses`, `/v1/models`, `/v1/models/info`
- [x] Anthropic-native: `/v1/messages` (correct event ordering), `/v1/messages/count_tokens`
- [x] Thin proxying: embeddings, images, audio (speech + transcription), video (incl. async jobs), search
- [x] `/v1/web/fetch` with a complete SSRF guard: scheme allowlist, DNS resolution + validation, pinned connections, per-hop redirect checks
- [x] Admin CRUD + usage stats, bearer auth on `/v1/*`, optional admin-token auth on `/api/*`
- [x] Router-issued API keys hashed with SHA-256
- [x] Upstream credentials AES-256-GCM encrypted at rest; mandatory `secrets.key`; boot re-encryption of legacy rows
- [x] Configurable retry contract with exponential backoff (`router.max_retries_per_tier`, `router.max_retry_delay_ms`)
- [x] Own SQLite schema, migrations embedded via `sqlx::migrate!`

---

## P0 — Correctness and security

**All closed (2026-09-15).** Short notes on what landed; the sections previously
here described the problems, which no longer exist.

### [x] P0.1 — Encrypt upstream credentials at rest
`api_key` is AES-256-GCM encrypted (`src/crypto.rs`), keyed from `secrets.key`
(64 hex characters or base64; mandatory — `config::validate` refuses to start
without it). `ConnectionRepository` encrypts on write and decrypts on read;
`Catalog::load` decrypts for resolution. `Db::migrate_credentials` rewrites
legacy plaintext rows on boot and aborts when a stored value does not decrypt
with the configured key. Tests: crypto round-trip/wrong-key, at-rest
assertions, boot migration.

### [x] P0.2 — Decide and enforce the admin API posture
`middleware::require_admin_token` enforces `server.admin_token` on every
`/api/*` route whenever the token is configured; tokens are compared via hashed
digests so the check is not timing-observable. A non-loopback bind without a
token is rejected at startup unless `server.allow_unauthenticated_admin = true`
is set explicitly. The dashboard sends the token from its header key dialog.
Tests: `tests/routes.rs`.

### [x] P0.4 — Close the SSRF bypass in `/v1/web/fetch`
`validate_fetch_url` allows only http(s), rejects localhost names and every
private/loopback/link-local/unique-local/unspecified/broadcast address —
including IPv4-mapped IPv6 forms — resolves hostnames and checks all resolved
addresses, then `pinned_client` connects via `resolve_to_addrs` with
`redirect::Policy::none`. Redirects are followed manually for up to five hops,
each re-validated. Tests: address-range units, scheme rejection, route-level 400s.

### [x] P0.5 — Remove the dead `[queue]` config
`QueueSection` is gone from `RouterConfig` and `router.example.toml`.
Concurrency limiting remains a real feature under P1.1.

### [x] P0.6 — Pin the retry contract
`router.max_retries_per_tier` (default 2) and `router.max_retry_delay_ms`
(default 30 000) flow through `RetryPolicy` → `LlmStreamOptions` into both
providers. Delay is 500 ms doubling per retry, capped; `Retry-After` wins. The
policy now applies even when a request carries no generation options — the old
`unwrap_or_default()` path silently fell back to 10 retries. Tests: retry-delay
math, policy mapping, fallback timing.

---

## P1 — Completeness against 9router parity

- [ ] **P1.1 Concurrency limiting.** Add a queue/limiter over upstream calls —
      per-connection and global. (`[queue]` was deleted in P0.5; this is the real
      feature.)
- [ ] **P1.2 Rate limiting per client key.** Currently any valid key can hammer
      the router. Add token-bucket limits keyed by `api_key_id`, with 429 +
      `Retry-After`.
- [ ] **P1.3 OAuth subscription providers.** Claude Code / Codex / Copilot /
      Kiro. Deferred from v1. Requires login flow, token refresh, and a new
      `provider_type` that is not API-key based.
- [ ] **P1.4 Streaming for the non-streaming Anthropic path.** See the
      `TODO E4: non-streaming path` marker at `src/llm/providers/anthropic.rs:716`.
- [ ] **P1.5 Budget / quota enforcement.** Tokens and cost are *recorded*; nothing
      is *enforced*. Add per-key spend caps with a soft-warn and hard-block mode.
- [ ] **P1.6 Tool-call coverage in the non-streaming path.** `chat_backend::collect`
      currently errors when a provider emits a tool call and `stream:false` was
      requested. Decide: aggregate tool calls into the JSON response, or require
      streaming for tool use.

---

## P2 — Performance and operations

- [ ] **P2.1 Cache the routing catalog.** `AppState::resolver()` reloads
      connections + aliases + combos + entries from SQLite on **every request**.
      Add a generation counter or short TTL, invalidated by admin writes.
- [ ] **P2.2 Split the oversized vendored files.** `src/llm/providers/openai.rs`
      (~1745 LOC) and `anthropic.rs` (~1006 LOC) exceed the project's 800-LOC cap.
      Convert to module folders before they grow further.
- [ ] **P2.3 Structured request logging / metrics.** `x-router-*` headers exist,
      but there are no Prometheus-style counters (requests by tier, failover rate,
      upstream error rate). Needed to see a bad tier in production.
- [ ] **P2.4 Health/readiness split.** `/api/health` is a static `ok`. Add
      readiness that checks DB connectivity and, optionally, upstream reachability.
- [ ] **P2.5 Graceful upstream timeouts.** Per-connection connect/idle timeout
      config, surfaced through config rather than hardcoded.

---

## P3 — Repository and release hygiene

Required before this is a real standalone repo.

- [ ] **P3.1 Add a LICENSE file.** `Cargo.toml` declares
      `MIT OR Apache-2.0` but no license text is present. Pick one (or both) and
      add the file — a declared-but-missing license is worse than none.
- [ ] **P3.2 CI.** No pipeline exists. Minimum: `cargo fmt --check`,
      `cargo clippy -- -D warnings`, `cargo test` on Linux + Windows, plus
      `npm run check` + `npm test` for `apps/web`. Caching matters: the vendor
      layer and retry-backoff tests dominate runtime.
- [ ] **P3.3 Resolve vendored-code staleness.** `src/llm/` is a vendored
      provider stack with no external upstream to pull fixes from. Options:
      (a) accept the vendored copy as the source of truth, or (b) extract it
      into its own workspace crate and depend on it. The one-file seam makes
      (b) cheap — see `HANDOVER.md`.
- [ ] **P3.4 Container image.** No Dockerfile. A router is a natural container;
      add one with a non-root user and a volume for the SQLite DB.
- [~] **P3.5 Dashboard / admin UI.** The Vue app in `apps/web` covers the full
      admin API (connections, aliases, combos, keys, usage) and runs via Vite in
      development; the backend now enforces the optional admin token the UI
      sends. Remaining: build/serve `dist/` from the Rust binary.
- [ ] **P3.6 End-to-end tests against a real provider.** The suite uses an
      in-process mock. Nothing verifies real OpenAI/Anthropic wire compatibility.
      Add a nightly/opt-in test gated on credentials.

---

## Explicitly out of scope

Carried over from the original design, still deferred. Not "todo" — these are
deliberate exclusions so the package stays small:

- RTK `tool_result` compression, prompt-cache tricks, caveman/ponytail prompt injection
- MITM proxy, Cloudflare/Tailscale tunnels, proxy pools, `pxpipe`
- Bundling a shared storage/media/RAG stack (would drag unrelated dependencies in
  and defeat the separate-DB design)
- Ollama as an upstream — rejected at write time and by the type system

---

## Suggested order

1. P2.1 — the per-request catalog reload is the first thing that will hurt at load.
2. P3.1, P3.2 — LICENSE and CI, cheap, and unblocks real collaboration.
3. P1.1, P1.2 — concurrency and rate limiting, in that order.
4. P1.3 onward — parity features.
5. P3.4, P3.5 remainder, P3.6 — container, onboarding the dashboard build, e2e tests.
