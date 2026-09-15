# alnair-router — Roadmap

Forward plan for the crate. Status is measured against the code as of
2026-09-15.

**Legend:** `[ ]` not started · `[~]` partially done · `[x]` done

**P0 is closed** (2026-09-15): credentials are encrypted at rest, the admin API
enforces `server.admin_token` when configured (and refuses non-loopback binds
without one unless explicitly overridden), `/v1/web/fetch` validates and pins
every hop, the dead `[queue]` config is gone, and the retry contract is pinned
and configurable.

**P1 is closed except P1.3** (OAuth providers, deferred with a design note
below): upstream concurrency caps, per-key rate limiting, monthly budgets,
Anthropic non-streaming, and tool calls in non-streaming responses all shipped.

**P2 is closed** (2026-09-15): cached routing catalog with invalidation,
provider files split under the LOC cap, Prometheus-style counters, a
liveness/readiness split, and per-connection upstream timeouts.

**P3 is closed** (2026-09-15): MIT LICENSE, GitHub Actions CI, the provider
stack extracted to `crates/alnair-llm`, the dashboard embedded in the binary, a
multi-stage Dockerfile, and opt-in real-provider e2e tests. Remaining follow-ups
are tracked in the sections below (P3.3 publish decision, Docker smoke test in a
Docker-capable environment, and the deferred P1.3).

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
- [x] Usage filtering by API key, model (substring), provider type and connection (snapshotted per row), shared by the table and summary, with `/api/usage/facets` suggestions
- [x] Usage breakdowns: prompt/completion/cached/reasoning tokens and input/output/reasoning cost components with popovers on the Usage page
- [x] Model pricing: dashboard overrides, LiteLLM/models.dev catalog sync (opt-in), reasoning-token premium, leaf-based lookup (`vendor/` prefixes and relay paths), per-connection `pricing_model` pin and a `/api/pricing/match` debug tool
- [x] Router-issued API keys hashed with SHA-256
- [x] Upstream credentials AES-256-GCM encrypted at rest; mandatory `secrets.key`; boot re-encryption of legacy rows
- [x] Configurable retry contract with exponential backoff (`router.max_retries_per_tier`, `router.max_retry_delay_ms`)
- [x] Global and per-connection upstream concurrency caps (`limits.*`), held for the stream lifetime
- [x] Per-key rate limiting and monthly budgets (`off`/`warn`/`block`) with 429/402 enforcement
- [x] Key rules: per-key model allowlist (exact + `prefix/*`/`*` wildcards) and reusable `key_plans` ("set the rules once") merged into effective key policy
- [x] Anthropic non-streaming (`stream: false`) provider path; tool calls surfaced on all non-streaming endpoints
- [x] Own SQLite schema, migrations embedded via `sqlx::migrate!`
- [x] Provider stack extracted to the `crates/alnair-llm` crate with a one-file seam
- [x] Dashboard embedded in the binary and served at `/` (SPA fallback, `server.serve_dashboard`)
- [x] Desktop tray icon (Windows/macOS): Open dashboard + Quit, graceful shutdown, `server.tray` / `--no-tray`, icon from `assets/alnair-white.ico`
- [x] MIT license, CI, multi-stage Dockerfile, opt-in real-provider e2e tests

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

- [x] **P1.1 Concurrency limiting.** `limits.max_concurrent` and
      `limits.max_concurrent_per_connection` cap in-flight upstream calls across
      the executor and the media proxies. Permits are held for the lifetime of a
      stream (not just the first chunk), and a request that cannot get a slot in
      `limits.acquire_timeout_ms` fails with 429 + `Retry-After`.
- [x] **P1.2 Rate limiting per client key.** Token bucket per API key id
      (`src/limits.rs`), default via `rate_limit.requests_per_minute`, per-key
      override on `api_keys.rate_limit_per_minute`; 429 with `Retry-After`.
- [ ] **P1.3 OAuth subscription providers.** Deferred — design note below.
- [x] **P1.4 Streaming for the non-streaming Anthropic path.**
      `LlmProvider::complete` plus a real `stream: false` `/v1/messages` call for
      `AnthropicNativeProvider`, so non-streaming requests no longer pay SSE
      overhead or chunk reassembly. Providers without an override fall back to
      draining their stream.
- [x] **P1.5 Budget / quota enforcement.** Per-key `monthly_budget_usd` with
      `budget_mode` (`off`/`warn`/`block`). Warn mode adds an
      `x-router-budget-warning` header; block mode returns 402
      `insufficient_quota`. Spend is the calendar-month sum of `usage_records`.
- [x] **P1.6 Tool-call coverage in the non-streaming path.**
      `chat_backend::collect` aggregates `ToolCall` chunks instead of erroring:
      OpenAI gets `message.tool_calls`, Anthropic gets `tool_use` blocks,
      Responses gets `function_call` items. `finish_reason` reports
      `tool_calls` / `tool_use` accordingly.

### P1.3 design note — OAuth subscription providers (deferred)

Goal: route Claude Code / Codex / Copilot / Kiro subscription sessions as
upstreams that are not API-key based.

Pieces required:

1. **Provider type + auth mode.** Either new `provider_type` values
   (`anthropic-oauth`, `openai-codex`, `github-copilot`, `kiro`) or an
   `auth_mode` column; `connections.api_key` stops being the only credential.
2. **Login flows.** Device-code or PKCE per provider. Client ids/secrets are
   provider-specific and some are extracted from first-party CLIs — that is a
   ToS/legal decision before any code.
3. **Token storage + refresh.** Reuse `CredentialCipher` for access/refresh
   tokens (a JSON credential blob), plus a refresh task with rotation and
   expiry handling.
4. **Request signing.** Codex/Copilot add or exchange headers
   (`ChatGPT-Account-Id`, Copilot token exchange); the seam is
   `chat_backend.rs`, which already assembles per-connection headers.
5. **Operations.** Admin API + dashboard for login/refresh state and
   documented failure modes.

One provider end-to-end is a multi-day feature and cannot be verified offline;
pick a single provider when there is a live account to test against.

---

## P2 — Performance and operations

**All closed (2026-09-15).** Short notes on what landed.

- [x] **P2.1 Cache the routing catalog.** `CatalogCache` serves a shared
      `Catalog` + `Resolver` snapshot; every admin write to connections, aliases
      or combos invalidates it, and `router.catalog_ttl_ms` (default 1 s) is a
      backstop for out-of-band edits. The budget rollup for keys with budgets
      still runs one query per request — folding it into the cache would need
      counter invalidation and is left as a follow-up.
- [x] **P2.2 Split the oversized vendored files.** Both providers are module
      folders now, with no file over ~620 LOC: `openai/{mod,request,chunks,tests}`
      and `anthropic/{mod,stream,tests}`.
- [x] **P2.3 Structured request logging / metrics.** `src/metrics.rs` keeps
      coarse atomic counters (requests, attempts, failures, failover, tokens,
      cost, rate-limited, budget-blocked); `GET /api/metrics` renders Prometheus
      text (0.0.4) under the admin token.
- [x] **P2.4 Health/readiness split.** `/api/health` is a static liveness probe
      (no DB); `/api/ready` checks the database and, when
      `server.readiness_upstream_checks` is on, reports TCP reachability per
      enabled connection without failing readiness (failover covers upstreams).
      Both probes are public so healthchecks need no bearer token.
- [x] **P2.5 Graceful upstream timeouts.** `router.connect_timeout_ms` bounds
      connect + first byte per tier; `router.idle_timeout_ms` bounds silence
      between stream chunks. Both are per-connection overridable
      (`connections.connect_timeout_ms` / `idle_timeout_ms`, `0` disables) and
      enforced in the executor, so they apply to every provider.

---

## P3 — Repository and release hygiene

**All closed (2026-09-15).** Short notes on what landed.

- [x] **P3.1 Add a LICENSE file.** MIT `LICENSE` at the repo root; the workspace,
      `alnair-router`, `alnair-llm`, and the dashboard declare `MIT`.
- [x] **P3.2 CI.** `.github/workflows/ci.yml`: rustfmt check, clippy with
      `-D warnings`, and the workspace test suite on Linux + Windows
      (`Swatinem/rust-cache`), plus the web suite (`pnpm install`, Vitest,
      vue-tsc + build). Activate by pushing to a GitHub remote; the local repo
      has none yet.
- [x] **P3.3 Resolve vendored-code staleness.** Chose option (b): the provider
      stack moved to the `crates/alnair-llm` workspace crate (`publish = false`).
      The seam guard now asserts only `upstream/chat_backend.rs` references
      `alnair_llm`. Follow-up: decide when/if to publish the crate.
- [x] **P3.4 Container image.** Multi-stage `Dockerfile` (node → rust →
      debian-slim), non-root user, `/data` volume, `ALNAIR_ROUTER_HOME=/data`.
      Not built in this environment (no Docker CLI) — smoke-test once on a
      Docker-capable machine.
- [x] **P3.5 Dashboard / admin UI.** `rust-embed` serves `apps/web/dist` at `/`
      with an SPA fallback and a real 404 for missing assets; `build.rs` drops a
      placeholder so a fresh clone still compiles; `server.serve_dashboard`
      turns it off behind a reverse proxy. Docker and CI build the web app
      before the Rust step.
- [x] **P3.6 End-to-end tests against a real provider.** `tests/e2e_real.rs`,
      `#[ignore]`d, gated on `ALNAIR_ROUTER_E2E_{OPENAI,ANTHROPIC}_*` env vars;
      covers OpenAI non-streaming + streaming and Anthropic Messages.

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

Everything through P3 is closed. What remains:

1. P1.3 — OAuth providers, only with a provider decision and a live account.
2. Follow-ups: decide when to publish `alnair-llm` (P3.3), smoke-test the Docker
   image on a Docker-capable machine, fold the budget spend rollup into the
   catalog cache, and prune idle token buckets.
