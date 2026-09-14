# alnair-router — Roadmap

Forward plan for the crate. Status is measured against the code as of
2026-09-15.

**Legend:** `[ ]` not started · `[~]` partially done · `[x]` done

**Not production-ready.** Three blockers, all in §P0: upstream credentials are
stored **plaintext**, the admin API is **unauthenticated** (acceptable only while
bound to loopback), and `/v1/web/fetch` has a **known SSRF bypass** via redirects
and hostname resolution.

---

## Where things stand

Working today, verified by the test suite and a live smoke test:

- [x] Prefixed alias resolution, combo expansion (cycle detection, depth cap 8), tier numbering
- [x] Ordered fallback with first-chunk peek — never switches mid-stream after bytes are emitted
- [x] OpenAI-compatible: `/v1/chat/completions` (SSE + JSON), `/v1/responses`, `/v1/models`, `/v1/models/info`
- [x] Anthropic-native: `/v1/messages` (correct event ordering), `/v1/messages/count_tokens`
- [x] Thin proxying: embeddings, images, audio (speech + transcription), video (incl. async jobs), search
- [x] `/v1/web/fetch` with SSRF guard
- [x] Admin CRUD + usage stats, bearer auth on `/v1/*`
- [x] Router-issued API keys hashed with SHA-256
- [x] Own SQLite schema, migrations embedded via `sqlx::migrate!`

---

## P0 — Correctness and security

These are the items where the current behaviour is either wrong or unsafe to
ship to anything but a localhost developer.

### P0.1 — Encrypt upstream credentials at rest
`connections.api_key` is stored **plaintext** in SQLite (`src/db/repos/connections.rs`).
Router-issued keys are hashed, but the credentials *to your providers* are not.

- Add an encryption layer (AES-256-GCM, via a vetted crate such as `aes-gcm`)
  keyed from an env var or OS keyring.
- Migration to re-encrypt existing rows.
- Deliberate fallback: if no key is configured, refuse to start rather than
  silently storing plaintext.
- Tests: round-trip, wrong-key failure, migration.

### P0.2 — Decide and enforce the admin API posture
Admin routes are **unauthenticated by design** (they mint the keys protecting
`/v1/*`) and bind `127.0.0.1`. That is defensible for localhost, but nothing
stops a user from setting `host = "0.0.0.0"`.

- Refuse to bind a non-loopback host when admin auth is off, unless an explicit
  `i_understand_the_risk`-style flag is set.
- Or add a separate admin token, distinct from client API keys.
- Document the reverse-proxy requirement more prominently than a README note.

### P0.4 — Close the SSRF bypass in `/v1/web/fetch`
`is_private_url` (`src/handlers/media.rs:272`) is correct **for the URL it is
given** — it blocks loopback, RFC-1918, link-local (incl. `169.254.169.254`),
unique-local and unspecified addresses, and has tests for each. But it is only
applied to the **initial** URL, and two bypasses remain:

1. **Redirects.** The fetch uses a default `reqwest::Client`, which follows up to
   10 redirects. `https://attacker.example/x` → `302 Location: http://169.254.169.254/latest/meta-data/`
   is fetched without re-checking the guard.
2. **Hostnames that are not IP literals.** `is_private_url` returns `false` for any
   bare hostname (`_ => false`). A domain resolving to `127.0.0.1` — or DNS
   rebinding between check and connect — passes.

- Disable redirect following (`redirect(reqwest::redirect::Policy::none())`), or
  re-validate every hop.
- Resolve the hostname and validate the resolved IP(s) **before** connecting.
- Consider blocking non-HTTP(S) schemes explicitly.

This is exploitable by anything that can reach `/v1/web/fetch`, so it sits in P0
alongside the credential and admin-auth issues.

### P0.5 — Remove the dead `[queue]` config
`QueueSection` (`src/config.rs`) is parsed and defaulted but **never read** — no
`ChatQueueManager` exists in this package. It silently implies concurrency
control that does not happen.

- Either implement the limiter (see P1.1) or delete the section.
- Do not leave config that lies.

### P0.6 — Pin the retry contract
Retry logic lives **inside the provider layer** (`src/llm/providers/common.rs`,
`PROVIDER_MAX_RETRIES = 10`, retryable = 408/429/500/502/503/504), while the
executor makes exactly **one** attempt per tier. This is the reverse of the
original design note ("1 retry per target, then move on").

Consequences to decide on:
- A tier can burn up to 10 retries with backoff (default 30s, capped by
  `max_retry_delay_ms`) before failover is even considered. Worst-case latency is
  large and currently unconfigurable per-tier from the router.
- Surface a `max_retries_per_tier` / overall deadline knob, and document the
  actual behaviour.

---

## P1 — Completeness against 9router parity

- [ ] **P1.1 Concurrency limiting.** Reintroduce a queue/limiter over upstream
      calls — per-connection and global. This is what `[queue]` was meant to be.
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
      `cargo clippy -- -D warnings`, `cargo test` on Linux + Windows.
      Note the vendored provider tests take ~45s, so caching matters.
- [ ] **P3.3 Resolve vendored-code staleness.** `src/llm/` is a vendored
      provider stack with no external upstream to pull fixes from. Options:
      (a) accept the vendored copy as the source of truth, or (b) extract it
      into its own workspace crate and depend on it. The one-file seam makes
      (b) cheap — see `HANDOVER.md`.
- [ ] **P3.4 Container image.** No Dockerfile. A router is a natural container;
      add one with a non-root user and a volume for the SQLite DB.
- [~] **P3.5 Dashboard / admin UI.** The Vue app in `apps/web` covers the full
      admin API (connections, aliases, combos, keys, usage) and runs via Vite in
      development. Remaining: build/serve `dist/` from the Rust binary, and wire
      the optional admin token into the `/api/*` auth middleware (the UI already
      sends it when configured).
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

1. P0.5 (delete the lying config) — trivial, immediate.
2. P0.1, P0.2, P0.4 — the three things that make it unsafe beyond localhost
   (plaintext credentials, unauthenticated admin, SSRF bypass).
3. P3.1, P3.2 — cheap, and unblocks real collaboration.
4. P2.1 — the per-request catalog reload is the first thing that will hurt at load.
5. P1.1, P1.2 — concurrency and rate limiting, in that order.
6. P1.3 onward — parity features.
