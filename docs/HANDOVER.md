# alnair-router — Handover

Everything a developer needs to pick this up in a **fresh workspace or repo**.
Read this first, then `docs/ROADMAP.md`.

---

## 1. What this is

A standalone, OpenAI-compatible **AI router**. Point any OpenAI or Anthropic SDK
at it; use a model reference; it resolves that reference to an upstream provider
and streams the response back, failing over across a chain if the first provider
does not work.

Three ideas carry the whole design:

| Concept | Meaning |
|---|---|
| **Alias** | A prefix like `glm` mapping to a configured connection. `glm/glm-4.6` → that connection, model `glm-4.6`. |
| **Combo** | A named, ordered fallback chain. `free-forever` → try entry 1, then 2, then 3. |
| **Connection** | An upstream endpoint: `openai-compatible`, `anthropic-native`, `command-code` or `codebuddy-intl`, with a base URL and key. |

The tiered-combo design is modelled on
[9router](https://github.com/decolua/9router) (MIT).

---

## 2. Status right now

- **Builds and tests standalone.** 223 tests green, `cargo clippy --workspace --all-targets` clean.
- **Self-contained by construction:** no path dependencies anywhere in the
  workspace. `Cargo.lock` resolves entirely from crates.io, so `target/` can be
  deleted and `cargo build --offline` still succeeds.

**The P0 blockers are closed** (see roadmap §P0 for the details):

1. Upstream credentials are **AES-256-GCM encrypted at rest** (`src/crypto.rs`).
   `secrets.key` is mandatory; legacy plaintext rows are re-encrypted on boot,
   and a wrong key aborts startup instead of serving garbage.
2. The admin API enforces **`server.admin_token`** whenever it is configured,
   and a non-loopback bind refuses to start without one unless the operator sets
   `server.allow_unauthenticated_admin = true`.
3. `/v1/web/fetch` re-validates **every redirect hop**, rejects non-HTTP(S)
   schemes, resolves hostnames up front and **pins the connection** to the
   validated addresses.
4. The dead `[queue]` config is gone, and provider retries are a documented,
   configurable contract (`router.max_retries_per_tier`,
   `router.max_retry_delay_ms`) with exponential backoff.

Still not production-ready in the "hardened service" sense: the admin API is
open on loopback by default and the dashboard is not yet served by the binary
(P3.5).

**P1 is done except OAuth providers** (P1.3, deferred with a design note in the
roadmap):

- `limits.max_concurrent` / `max_concurrent_per_connection` cap upstream calls;
  permits live as long as the stream.
- Per-key token buckets (`rate_limit.requests_per_minute` + per-key override)
  return 429 with `Retry-After`.
- Per-key monthly budgets with `off`/`warn`/`block`; warn adds
  `x-router-budget-warning`, block returns 402 `insufficient_quota`.
- Anthropic has a real `stream: false` path (`LlmProvider::complete`), and
  non-streaming responses on every endpoint now surface tool calls.
- Streaming responses carry the same payloads as non-streaming ones: tool calls
  (`delta.tool_calls` / `tool_use` blocks), reasoning (`reasoning_content` /
  `thinking` deltas), a translated `finish_reason` / `stop_reason`, and an
  opt-in usage chunk (`stream_options.include_usage`).

**P2 is done** (2026-09-15):

- The routing catalog is cached and invalidated by every admin write
  (`model/cache.rs`); `router.catalog_ttl_ms` is a backstop for out-of-band edits.
- Provider files are module folders, all under the 800-LOC cap.
- `src/metrics.rs` + `GET /api/metrics` expose Prometheus-style counters.
- `/api/health` is liveness, `/api/ready` checks the database and optionally
  upstream reachability; both probes are public.
- Per-connection connect/idle timeouts are enforced in the executor.

**P3 is done** (2026-09-15):

- `LICENSE` (MIT) ships with the repo; manifests and the dashboard declare it.
- CI (`.github/workflows/ci.yml`) runs fmt/clippy/test on Linux + Windows and
  the web suite; push it to a GitHub remote to activate.
- The provider stack was extracted to the `crates/alnair-llm` crate — the seam
  is now a crate dependency guarded by the same test.
- The dashboard is embedded in the binary (`rust-embed` + `build.rs`
  placeholder) and served at `/` unless `server.serve_dashboard = false`.
- A multi-stage `Dockerfile` builds web + router from source for a local
  `docker build`; the published image is produced by `Dockerfile.release`, which
  copies the prebuilt Linux binaries into a multi-arch (amd64 + arm64) image with
  no compiler. Both run non-root with a `/data` volume. Not built in this
  environment (no Docker CLI); the release workflow smoke-tests the image on a
  Docker-capable runner.
- Opt-in real-provider tests live in `tests/e2e_real.rs` (`--ignored`).

---

## 3. Architecture

```
Cargo.toml                    # virtual workspace root (members, profiles)
Cargo.lock                    # resolves entirely from crates.io
LICENSE                       # MIT
Dockerfile                    # source image recipe (web + router, non-root, /data)
Dockerfile.release            # CI image: prebuilt Linux binaries, multi-arch
.github/workflows/ci.yml      # fmt/clippy/test + web suite
apps/web/                     # admin dashboard (Vue 3 + Vite + Tailwind, pnpm)
crates/alnair-llm/            # provider stack crate (moved out of the router)
└── src/{lib,types,model_config,provider}.rs + providers/{...}
crates/alnair-router/
├── Cargo.toml
├── build.rs                 # ensures apps/web/dist exists for rust-embed
├── migrations/0001_init.sql # 6 tables
├── migrations/0002_api_key_limits.sql
├── migrations/0003_connection_timeouts.sql
├── router.example.toml       # every config option
├── src/
│   ├── main.rs              # load config → cipher → connect DB → migrate → serve
│   ├── lib.rs               # module tree + shallow re-exports
│   ├── cli/
│   │   ├── mod.rs           # argv parsing, help, status
│   │   ├── autostart.rs     # install/uninstall via the auto-launch crate
│   │   └── daemon/
│   │       ├── mod.rs       # detach, spawn, logs, start/stop/restart
│   │       ├── record.rs    # router.pid: pid + address, liveness, signals
│   │       └── control.rs   # control.token, stop request, health probe
│   ├── config.rs            # file + ALNAIR_ROUTER__SECTION__KEY env
│   ├── crypto.rs            # AES-256-GCM credential encryption + key parsing
│   ├── backup.rs            # VACUUM INTO snapshots + transactional restore
│   ├── limits.rs            # concurrency semaphores + token buckets + budget mode
│   ├── metrics.rs           # atomic counters + Prometheus text exposition
│   ├── telemetry.rs         # in-memory activity feed (attempts, HTTP, events)
│   ├── error.rs             # scoped Error → OpenAI-shaped JSON error body
│   ├── state.rs             # AppState: config, pool, cipher, executor, repos
│   ├── settings.rs          # dashboard setting overrides + `settings` table repo
│   ├── middleware.rs        # bearer auth + rate/budget checks for /v1/* and /api/*
│   ├── server.rs            # route table (public probes, guarded admin, SPA fallback)
│   ├── model/
│   │   ├── cache.rs         # catalog cache: TTL + explicit invalidation
│   │   ├── catalog.rs       # loads connections/aliases/combos from DB
│   │   └── resolver.rs      # pure: reference → ordered Vec<ResolvedTarget>
│   ├── db/
│   │   ├── mod.rs           # pool + embedded migrations + credential migration
│   │   └── repos/           # connections, aliases, combos, api_keys, usage
│   ├── upstream/
│   │   ├── chat_backend.rs  # ← THE SEAM. Only file that may touch alnair_llm
│   │   ├── executor.rs      # fallback walk, permits, timeouts, first-chunk peek
│   │   ├── probe.rs         # admin model listing / connection tests
│   │   └── media/          # HTTP proxying for non-chat endpoints
│   │       ├── mod.rs      # the handlers
│   │       ├── proxy.rs    # resolution, forwarding, usage attribution
│   │       ├── multipart.rs # byte-level form-field helpers
│   │       └── fetch.rs    # /v1/web/fetch and its SSRF guard
│   ├── protocol/            # OpenAI ⇄ Anthropic wire translation
│   ├── token_saver/         # deterministic pipeline: slimmer, headroom, directives
│   └── handlers/            # chat, messages, responses, models, catalog, media, admin, backup, public, web, token_saver, control
└── tests/                   # resolve, storage, fallback, routes, cli_lifecycle, e2e_real
```

### Request flow

```
POST /v1/chat/completions { "model": "free-forever" }
  │
  ├─ middleware::require_api_key      (skipped if require_api_key = false)
  │    └─ token bucket + monthly budget checks (429 / 402)
  ├─ state.resolver()                 → cached catalog snapshot (admin writes invalidate)
  ├─ Resolver::resolve("free-forever")→ Vec<ResolvedTarget>, ordered
  ├─ Executor::stream(targets, ...)
  │    └─ for each target: chat_backend::stream(...) → peek FIRST chunk
  │         ├─ first chunk is Err  → record failed attempt, try next target
  │         └─ first chunk is Ok   → re-attach it, return the live stream
  └─ handler translates chunks to OpenAI SSE, writes usage row
```

### Schema (`migrations/0001_init.sql`)

| Table | Purpose |
|---|---|
| `connections` | Upstream endpoints. `provider_type` is CHECK-constrained to the two supported values. |
| `aliases` | `prefix` → `connection_id`, optional `model_override`. Cascades on connection delete. |
| `combos` | Named fallback chains. |
| `combo_entries` | Ordered tiers. `model_ref` can be an alias ref, a bare model, or another combo. |
| `api_keys` | Router-issued client keys. Stores SHA-256 `key_hash` for lookup plus AES-256-GCM `secret_enc` for reveal, gated by `server.store_key_secrets`. |
| `usage_records` | One row per attempt — failures included, not just successes. |
| `settings` | Single JSON row of dashboard-managed overrides (migration `0010_settings.sql`). |

### Runtime settings

`config.toml` and `ALNAIR_ROUTER__*` env vars define the **base** configuration,
loaded once at startup and validated before the listener binds. The dashboard's
Settings page stores sparse overrides in the `settings` table; at startup they
are merged over the base and hot-applied, and `PATCH /api/settings` re-applies
them without a restart. `DELETE /api/settings` drops every override. The admin
token is write-only: `GET /api/settings` reports only whether one is set.

Managed fields: `server.require_api_key`, `server.admin_token`,
`server.readiness_upstream_checks`, all of `router.*` and `limits.*`,
`rate_limit.*`, `pricing.*` and `token_saver.*`. Applying an override pushes the
new values into the live components (`UpstreamLimiter`, `RateLimiter`,
`Executor`, `CatalogCache`) and swaps `AppState::config`; the pricing sync loop
re-reads the config each iteration and is woken by
`AppState::pricing_sync_trigger`. `token_saver` needs no extra plumbing: the seam
reads the setting per request via `AppState::token_saver_settings`, so a toggle
takes effect on the next call. Overrides are validated on the way in
(`SettingsOverrides::merge` → `config.validate`), which is what stops a
mutually-exclusive terse+caveman pair or a misspelled level from being saved.

Deployment-only values (`server.host`/`port`, `server.tray`,
`server.serve_dashboard`, `storage.url`, `secrets.key`) stay read-only in the
API and are surfaced in the `deployment` block.

### Provider presets

`src/providers/` ships a built-in catalog of one-click endpoints: `mod.rs` holds
the types and lookups, `catalog.rs` the tables, `tests.rs` the invariants. Each
preset pins the base URL, wire family, tier and optional default headers an
upstream needs, so a connection can be added without typing an endpoint.

Presets are grouped into three tiers (`ProviderCategory`): `api_key` (paid or
pay-as-you-go, 40 entries), `free_tier` (hosted providers with a free tier, 12)
and `local` (Ollama/LM Studio/vLLM). `presets()` concatenates the tiers and sorts
by `(category rank, label)`, so the JSON order already matches the dashboard's
picker sections.

`GET /api/providers` lists the catalog with `category`, `auth` and how many
connections use each preset. Creating a connection with `provider_id` fills
`provider_type`, `base_url` and default headers, while anything the user sent
still wins; migration `0015_connection_provider.sql` stores the label so the
dashboard can show a provider badge. Endpoint URLs were cross-checked against
9Router's provider registry and, where one exists, the vendor's own docs.

Two rules keep the catalog honest:

- A `base_url` is everything **before** the request path — the provider layer
  appends `/chat/completions` (`openai-compatible`) or `/messages`
  (`anthropic-native`). `anthropic` therefore ships `https://api.anthropic.com/v1`.
- A `command-code` preset stores only the host (`https://api.commandcode.ai`):
  that family builds both its Provider API path and its CLI path itself.
- Account-specific endpoints (`azure-openai`, `cloudflare`) ship a literal
  `<placeholder>`, and `ConnectionRepository::validate_base_url` refuses to store
  a base URL that still contains `<`/`>` — a forgotten placeholder fails at save
  time, not at the first request. `only_templated_presets_contain_placeholders`
  keeps the placeholder list to exactly those two.

OAuth providers (Claude Code, Codex, GitHub Copilot, …) are the next phase:
credentials will live in a dedicated table keyed per account (many accounts per
provider for rotation), with per-request refresh inside `chat_backend` and
reuse detection; Copilot keeps its dual GitHub→Copilot token exchange cached
until expiry.

### Extra keys per connection

`connection_accounts` (migration `0016`) attaches more API keys to one
connection — several keys or quota buckets behind the same endpoint. Writes go
through `/api/connections/{id}/accounts` and invalidate the routing catalog;
keys are AES-256-GCM encrypted like the primary one and never serialized back.
`Catalog::load` decrypts the enabled accounts into `Connection::extra_keys`, so
`ResolvedTarget` carries `api_keys` (primary first, then accounts) instead of a
single key. `Executor::stream` snapshots a per-connection cursor from
`KeyRotator` and tries keys from that offset in a circle: traffic spreads
round-robin and a key that fails before the first byte falls through to the
next key before the tier is abandoned. Media proxying uses `primary_key()` and
does not rotate. The Connections table shows a `+N keys` badge and the edit
dialog manages the list; adding from a preset auto-suffixes a free name
(`openai`, `openai-2`, …).

### Dashboard authentication
The dashboard signs in with a **password only** (no username), stored as an
Argon2 hash in the single-row `auth` table (migration `0014_auth.sql`). Until
that row exists, the router prints a one-time setup code at startup
(`alnair-router setup code: …`); `POST /api/auth/setup` requires the code and
then creates the password. Sessions are opaque token pairs: `auth_sessions`
keeps SHA-256 hashes of access (30 minutes) and refresh (7 days sliding, 30
days absolute) tokens. `POST /api/auth/refresh` rotates the pair and marks the
old refresh row; presenting a rotated token means a copy is in use, so
`AuthRepository::refresh` returns `Rotation::Reused` and the caller revokes the
whole family. Logout revokes the caller's family, and changing the password
revokes every session. `server.admin_token` remains a machine credential for
scripts and CI.

`middleware::require_admin_token` accepts, in order:
`allow_unauthenticated_admin`, a matching `admin_token`, or a live session.
With neither a password nor an admin token the documented localhost posture
applies; once `server.lan_access` is on (or `host` is non-loopback) every admin
route returns `401` until the password exists. `GET /api/auth/status` reports
`password_set`, `setup_required`, `authenticated`, `admin_token_set` and
`admin_open`, which is what the SPA guard and the login page read.

`server.lan_access` flips the listener to `0.0.0.0` without a restart:
`AppState::apply_overrides` notices the `listen_address()` change and notifies
`AppState::rebind`; the serve loop in `main.rs` lets `axum::serve` finish, then
binds the new address and serves again. Boot defaults still come from
`config.toml`/env, so the toggle only writes the `settings` row.

### Backup and restore

`GET /api/backup` writes a consistent SQLite snapshot with `VACUUM INTO` to a
temp file (streamed to the browser, deleted when the response ends). The
snapshot's `settings` row is deleted first, so a downloaded file never carries
the admin token. `POST /api/restore` streams the upload to a temp file (512 MiB
cap) and imports it: it verifies `quick_check`, the foreign-key graph, that the
backup's `_sqlx_migrations` set matches the running schema, and that every
connection credential decrypts with the current `secrets.key`. It then attaches
the file and, inside one transaction with `defer_foreign_keys`, deletes and
re-inserts every data table (`connections`, `aliases`, `combos`,
`combo_entries`, `key_plans`, `api_keys`, `usage_records`, `model_prices`,
`pricing_sync_runs`). `settings` is intentionally left alone, and the catalog
and pricing caches are invalidated afterwards. Note: `VACUUM INTO` is a no-op on
in-memory SQLite, so snapshots require the file-backed database the router
normally runs on.

### Public usage

`GET /api/public/usage` sits outside the admin-token guard. The caller
authenticates with a router-issued client key (the same bearer used on `/v1`)
via `handlers/public.rs::authorize`, which resolves the key without touching
rate limits, budgets or `last_used_at`. The response contains only that key's
rows: `UsageRepository::summary`, `UsageRepository::models` (a per-model
rollup) and `UsageRepository::timeseries` — one row per (bucket, model) so the
chart can stack models within the same bucket (`?bucket=hour|day`, defaulting
to `day`), optionally narrowed by `since`/`until` (`until` backs the month
selector). Buckets are the fixed-width RFC 3339 prefix of `created_at` rather
than `strftime`, so no date parsing of fractional seconds is involved.
`server.public_usage` (default true) gates the endpoint; disabled requests fail
with `403`. The dashboard exposes it as the self-service page at `/me`, which
renders the minimal public shell (`route.meta.public`), keeps the pasted key in
`sessionStorage` (not `localStorage`) and lazy-loads the Unovis trend chart so
the chart library stays out of the main bundle. The chart fills empty buckets
across the selected window (capped at 400) so a month renders as a full month,
stacks by model with a top-six plus "Other" legend and palette, and a month
selector pins an exact calendar window. The page polls every 30 seconds and on
tab focus (paused while hidden) so clients never refresh by hand. The Tokens
and Cost summary popovers add a "Share by model" section with percentages, and
the By model table's Tokens/Cost cells open the same breakdown popover for a
single model rather than printing raw columns. `GET /api/public/models` reuses
`CatalogEntry::collect` and filters it with `handlers/public.rs::accessible`,
which asks the resolved `KeyPolicy` (key rules merged with its plan) about the
alias prefix, pinned upstream or combo name; the customer table shows one row
per id with the four per-million rates, and the page prints the
OpenAI-compatible `/v1` base URL above it for copying.

---

## 4. Behaviours worth knowing before you change anything

These are the non-obvious decisions. Each one has a test; if you break one, the
suite should tell you.

1. **Failover only happens before the first byte.** If a tier emits any
   `Text`/`Thinking` chunk and *then* errors, the error surfaces to the client —
   the router does not silently switch mid-stream. This is why the executor
   peeks the first chunk. (`tests/fallback.rs`)

2. **A malformed `provider_type` aborts the whole request** instead of walking
   the chain. It is a config fault, not a transient upstream failure.
   (`executor.rs` matches `Error::UnsupportedProviderType`)

3. **Tier numbers reflect declared position, not ordinal.** `entry.position + 1`
   means disabling the 2nd of 4 tiers does not renumber the survivors.
   (`tests/resolve.rs`)

4. **A disabled combo is an explicit error**, not a fallthrough to a bare-model
   lookup that would silently resolve to the default connection.

5. **Usage is recorded once per request**, guarded by an `Arc<AtomicBool>` shared
   across stream-event clones. A `Usage` chunk followed by `Done` would otherwise
   insert two rows.

6. **Unresolvable model references are `404`, not `400`.** Deliberate: "you asked
   for something that does not exist" is different from "your request is malformed".

7. **Admin routes need `server.admin_token` when configured — even on loopback.**
   Without a token they stay open on loopback, matching the localhost posture.
   `require_admin_token` hashes both sides before comparing so token checks are
   not timing-observable. (`middleware.rs`, `tests/routes.rs`)

8. **`/v1/web/fetch` is SSRF-guarded end to end.** Every URL — initial and each
   redirect hop — must be http(s), pass `is_private_ip`, and for hostnames all
   resolved addresses must be public; the client then connects only to those
   pinned addresses and follows no redirects itself. (`handlers/media/fetch.rs`)

9. **Retry is a pinned contract.** The executor makes one attempt per tier; the
   provider retries `router.max_retries_per_tier` times (default 2) with 500 ms
   doubling backoff capped by `router.max_retry_delay_ms` (default 30 s). A
   `Retry-After` header wins. `RetryPolicy` is applied even when a request has no
   generation options. (`chat_backend.rs`, `providers/common.rs`)

10. **Credentials never touch SQLite in plaintext.** `ConnectionRepository`
    encrypts on write and decrypts on read; `Catalog::load` decrypts for the
    resolver. New `secrets.key` values make old rows fail loudly at boot via
    `Db::migrate_credentials`, which also rewrites legacy plaintext rows.

11. **Concurrency permits ride with the stream.** The executor acquires a global
    and a per-connection slot before opening each tier and attaches the permit to
    the returned chunk stream, so a slow consumer still holds its slot. A timeout
    waiting for a slot is a 429, not a tier failure — walking tiers would just hit
    the same cap again. (`limits.rs`, `executor.rs`)

12. **Rate limit and budget checks run after authentication.**
    `require_api_key` checks the token bucket first (in-memory) and then the
    spend rollup for every configured budget window (daily/weekly/monthly
    calendar windows plus a lifetime cap with no reset) and every token window
    (prompt + completion tokens). An expired key is
    rejected with 401; a key whose plan has expired fails closed with 403
    instead of falling back to its own fields. `null` on
    `rate_limit_per_minute`, any `*_budget_usd`, any `*_token_limit` or
    `expires_at` in a PATCH body clears the field; an absent field leaves it
    alone (`repos::double_option`).

13. **Non-streaming still flows through the chunk pipeline.** Handlers pass the
    client's `stream` flag to `Executor::stream` → `chat_backend::stream`, which
    selects `provider.stream` or `provider.complete`. Only Anthropic overrides
    `complete` (a real `stream: false` request) today; everyone else drains their
    SSE stream. `collect` aggregates tool calls instead of erroring, which is
    what makes tool use work with `stream: false`.

    **Streaming must stay at parity with it.** Both paths consume the same
    `StreamChunk` enum, so a variant that the streaming handler ignores is
    silently lost — that is how a coding agent ended up seeing a `200` with no
    tool call and retrying in a loop. When you add a chunk variant, translate it
    in `handlers/chat.rs::stream_events` *and* `handlers/messages.rs` as well as
    in `collect`. The terminal `StreamChunk::Done` carries the provider's finish
    reason; never hardcode it, and normalize it through
    `chat_backend::{openai_finish_reason, anthropic_stop_reason}` so an
    Anthropic-flavoured reason never reaches an OpenAI client.

14. **Catalog invalidation is explicit, the TTL is a backstop.** Every admin
    mutation calls `AppState::invalidate_catalog`; `router.catalog_ttl_ms`
    exists for out-of-band edits. If you add a new write path that changes
    connections, aliases or combos, invalidate there too. (`model/cache.rs`,
    `handlers/admin.rs`)

15. **Timeouts live in the executor, not the providers.** A tier must produce
    its first chunk within `connect_timeout_ms` (per-connection override beats
    the router default); after that, silence longer than `idle_timeout_ms`
    fails the stream. Both are `0`-disabled by default *per connection*, and
    enforced at the seam so every provider gets them for free. (`executor.rs`)

16. **Metrics are deliberately label-free.** Coarse counters only (requests,
    attempts, failures, failover, tokens, cost, rejections) so cardinality
    cannot explode; `/api/metrics` is Prometheus text and sits under the admin
    token. (`metrics.rs`)

17. **A bare alias prefix resolves when it pins a model.** `kr` works like
    `kr/anything` when the alias has a `model_override`; without one, a bare
    name still goes to `default_connection`. This is what makes imported
    aliases (`/models` → one alias per model) usable directly. The admin chat
    probe (`POST /api/aliases/{id}/test-chat`) runs the same resolver +
    executor path and records a usage row. (`resolver.rs`, `handlers/admin.rs`)

18. **The activity feed is ephemeral and cheap.** `telemetry.rs` keeps a
    500-event ring buffer, per-connection counters and an in-flight gauge under
    a single mutex; the executor opens/closes an attempt, `require_api_key`
    records every `/v1` HTTP call and every rejection, handlers add token
    counters. `GET /api/activity` serves it to the Usage live panel and the
    Console page. Nothing is persisted — restarting the router clears it, and
    usage rows remain the durable record.

19. **The tray owns the main thread.** `tray-icon` needs a GUI event loop on the
    thread that owns the icon, and macOS requires it on the main thread before
    the icon is created. `serve` therefore keeps `tao`'s event loop on the main
    thread (`src/desktop/`) and runs the Tokio server on a worker thread; Quit
    notifies the server, and when the server stops on its own (Ctrl+C, fatal
    error) it posts an event so the tray exits too. Windows only: left-click
    opens the dashboard (menu on right-click); macOS keeps the standard
    menu-on-click and renders the icon as a template image so it adapts to
    light/dark menu bars. Tray failure is non-fatal: the server keeps running
    without an icon. `server.tray` (default true) and `--tray`/`--no-tray`
    control it; Linux always serves headless.

20. **Windows builds are GUI-subsystem: no console window, ever.** `main.rs`
    sets `windows_subsystem = "windows"` so auto-start and double-click never
    pop a terminal. `bind_parent_console` re-attaches stdio when the binary is
    invoked from a real terminal (`AttachConsole` + `CONOUT$`/`CONIN$`) so CLI
    output and logs stay visible; handles Windows already supplied — e.g.
    `Start-Process -RedirectStandardOutput` — are left untouched. Output with
    neither a console nor inherited handles goes nowhere, which is why a
    background run is given the log file as its stdio (see 33). The dashboard
    opener also spawns `cmd` with `CREATE_NO_WINDOW`, otherwise `cmd` would
    allocate a console just to launch the browser.

21. **Key rules are one policy, merged key-first.** `policy.rs` resolves an
    `ApiKey` plus its optional `KeyPlan` into a `KeyPolicy`: any field the key
    sets wins, the plan fills the rest, and budgets merge per window (a key can
    add a daily cap on top of the plan's monthly one). The budget mode travels
    with the key's own budgets: once the key sets any amount, its mode decides,
    and a key amount with `mode = off` disables every cap, including the
    plan's. The model
    allowlist matches case-insensitively with `*` (everything) and `prefix/*`
    (the prefix's children only); an empty list allows any model. Enforcement
    happens in the handlers right after the model is read — chat, messages
    (incl. count_tokens), responses, embeddings, images, audio and video — and
    fails with `403 permission_error`. Rate limiting runs before the allowlist,
    so a denied request still consumes a token. `/v1/models` is not filtered by
    the allowlist yet. (`policy.rs`, `middleware.rs`, `handlers/*`)

22. **Plans are reusable rule sets with one clear blast radius.** Deleting a
    plan detaches it from every key (`UPDATE api_keys SET plan_id = NULL`) and
    the keys fall back to their own fields and the server defaults. Duplicate
    plan names are rejected with a 400 before SQLite sees them, and attaching a
    key to an unknown plan is a 404, not a dangling reference. Wire shapes live
    in `db/repos/key_plans.rs` and mirror the dashboard types.

23. **Usage reads share one filter set.** `UsageFilter` (blank values count as
    unset; `model` is a case-insensitive substring, the rest exact) is applied
    by `/api/usage`, `/api/usage/summary`, `/api/usage/models` and
    `/api/usage/timeseries`, and the time range now narrows the table too, not
    just the cards. Rows snapshot the serving `connection_name` at write time
    (migration `0005`), so filtering stays meaningful after a connection is
    renamed or deleted;
    `resolved_provider` remains the wire protocol, not the connection.
    `/api/usage/models` is the admin-side per-model rollup (the dashboard's
    Overview feeds it to `UsageSummaryCards`), mirroring what
    `/api/public/usage` returns for a single key; `/api/usage/timeseries`
    exposes the same bucketed-per-model rollup the self-service page already
    charts, so the admin Usage page reuses `UsageTrendChart.vue`. Ordering is
    opt-in through `sort`/`order`, validated against a `SortField` whitelist so
    no query value reaches the `ORDER BY` string. `/api/usage/facets` returns
    the distinct models and connections (most used first) plus providers for the
    dashboard's suggestions. (`db/repos/usage.rs`, `handlers/admin.rs`,
    `UsageFilterBar.vue`, `UsageTable.vue`, `lib/usageView.ts`)

24. **Pricing is an override → crawl → built-in chain.** `model_prices` keeps
    two rows per model at most: `override` (written from the dashboard) and
    `sync` (replaced wholesale by each crawl). Crawls accept LiteLLM or
    models.dev payloads (`pricing/sources.rs`); when several providers list the
    same model in models.dev, the cheapest input rate wins so the stored row is
    deterministic. Lookups merge the two sources override-last and resolve in
    two steps: exact id, then the **last path segment** — so
    `azure/gpt-5.6-luna` answers a `gpt-5.6-luna` request and a relay like
    `ocg/openai/gpt-5.6-luna` still prices correctly. Rows sharing a leaf are
    ranked canonical (no `vendor/` prefix) → cheapest input → alphabetical.
    The merged index is cached (`PricingCache`) until a pricing write or sync
    invalidates it, and `GET /api/pricing/match?model=…` plus the Pricing page's
    "Test a model id" tool show which key answered. Connections may pin a
    `pricing_model` for relays whose ids match nothing; a model with no row at
    all falls back to the built-in `known_cost_rates` table. Rates reach the
    provider layer as `ModelConfig.cost_rates` inside `chat_backend` — the only
    file allowed to touch `alnair_llm`. Crawling is opt-in
    (`pricing.sync_enabled`, interval clamped to ≥ 60 s) and runs as a background
    task. Reasoning tokens are parsed by the OpenAI provider and billed as a
    premium over the output rate, because `completion_tokens` already includes
    them.

25. **Cost is stored split three ways.** `usage_records` keeps
    `cost_input_usd`, `cost_output_usd` and `cost_reasoning_usd` next to the
    `cost_usd` total, and the summary sums all three, so
    `cost_usd = input + output + reasoning` exactly. The provider layer reports
    the reasoning premium as its own field (`TokenCosts::reasoning_usd`), which
    keeps the output share free of reasoning; the Usage page turns this into
    token and cost breakdown popovers on the summary cards and the
    Tokens/Cost cells. Rows written before migration `0009` fall back to a
    single "recorded total" line in the breakdown.

26. **CORS is a live, explicit allowlist.** `server.cors_origins` starts empty
    and then emits no CORS headers at all; `"*"` allows any origin; otherwise
    only the listed origins are answered. `middleware::cors` reads the current
    config on every request and answers preflight `OPTIONS` itself, so the
    Settings page can change the policy without a restart (entries are
    validated as `scheme://host[:port]`). Bind and browser addresses go through
    `ServerConfig::bind_address` / `browser_address`, which bracket IPv6
    literals and turn an unspecified bind (`0.0.0.0`, `::`) into `127.0.0.1`
    for the tray link. (`middleware.rs`, `handlers/settings.rs`, `config.rs`,
    `main.rs`)

27. **Command Code resolves its transport once per credential.** A
    `command-code` connection prefers the documented Provider API
    (`/provider/v1/chat/completions`, served by the OpenAI provider) and falls
    back to the CLI envelope (`/alpha/generate`, NDJSON) when the credential may
    not use the API — a Go plan has none, which is exactly the case that
    fallback exists for. Two signals switch it: the `/provider/v1/models` probe
    answering 403/404, *and* a 403/404 on the real chat request, because the
    models endpoint is not gated the same way on every plan. Either way the
    decision is memoized per base URL plus credential fingerprint, so a rejected
    request is paid once per process and the request that discovers it is
    retried over the CLI instead of failing. The CLI path lifts system prompts
    into `params.system`, forwards only tool calls that have a matching tool
    result (and vice versa), renames the reserved `tool_search` tool and maps it
    back on the way out, and pins the CLI protocol version — overridable per
    connection through `custom_headers`. A base URL that still carries the
    Provider API path (what the preset shipped before this family existed) is
    normalized to its host, so switching an older connection's provider type is
    enough to bring it over. (`providers/commandcode/`)

28. **Provider types live in a CHECK constraint.** `connections.provider_type`
    is constrained to the supported families, so a new wire family needs a
    table rebuild (SQLite cannot alter a CHECK). With foreign keys on — the
    application-wide setting — `DROP TABLE connections` cascades into `aliases`
    and `connection_accounts`, so migration `0017` copies both aside and
    restores them before it commits; `provider_type_rebuild_keeps_children`
    runs the migration against a seeded database to keep that honest.

29. **The inbound body limit is disabled.** `build_router` layers
    `DefaultBodyLimit::disable()`, so the axum default 2 MiB cap no longer
    applies to the buffering extractors (`Json<…>`, `Bytes`) on `/v1/*`.
    Large-context models ship multi-megabyte requests — long prompts, inline
    base64 media, audio uploads — and the old cap rejected them with a `413`
    before any handler ran. `chat_completion_accepts_a_multi_megabyte_body`
    pins this. (`server.rs`)

30. **Router-issued keys are stored hashed *and* encrypted.** `api_keys` keeps
    the SHA-256 `key_hash` that authenticates inbound calls, and additionally
    an AES-256-GCM `secret_enc` under the same `secrets.key` as upstream
    credentials — without it the plaintext key was unrecoverable the moment it
    was minted, so the dashboard could only ever show an 18-character prefix.
    `secret_enc` is never serialized (`#[serde(skip_serializing)]`), so
    `GET /api/keys` cannot leak it; only `GET /api/keys/{id}/secret` decrypts,
    and it is admin-guarded. The toggle is `server.store_key_secrets` (default
    on), applied live through Settings, so turning it off restores the
    hash-only posture for keys minted from then on. Two consequences worth
    remembering: keys created before migration `0018` have no recoverable
    secret and must be rotated (`POST /api/keys/{id}/rotate`) to get a copyable
    one, and `secrets.key` now protects client keys as well, so backups must
    keep it alongside the database — `verify_credentials` refuses a restore
    whose `secret_enc` values do not decrypt. (`db/repos/api_keys.rs`,
    `handlers/admin/keys.rs`, `backup.rs`)

31. **The token saver is one function, and the playground runs that same
    function.** `token_saver::apply` is called once by each inbound chat handler
    (`chat.rs`, `messages.rs`, `responses/`) after the model reference is read
    but before `Executor::stream` resolves targets — so the rewrite happens once
    per request and every provider gets identical savings, rather than each
    attempt re-compressing the same messages. Order is fixed: slimmer (RTK) →
    headroom → terse/caveman → ponytail. Terse and caveman both write a system
    directive and are therefore mutually exclusive; `OutputSaver` makes that
    unrepresentable and `TokenSaverConfig::validate` rejects the combination at
    startup *and* on hot-apply, because `SettingsOverrides::merge` calls
    `config.validate()`. Every saver fails open: a Headroom outage records a
    note and forwards the original messages untouched, so the pipeline can never
    fail a request. Input savings are measured; output savings are estimated
    from a documented ratio applied to the completion, and
    `UsageRepository::savings()` keeps the two apart rather than blending a
    guess into a fact. `token_saver::run` returns the same result plus a
    `StepTrace` per step and backs the playground's Token Saver tab
    (`POST /api/token-saver/playground`), which reports the prompt token delta
    **measured from the rewritten messages** rather than trusting each saver's
    own reported figure — that is what makes it evidence instead of a
    restatement. A directive step reports a *positive* delta, since it adds the
    instruction it injects. Per-run overrides go through the same validation as
    the Configuration tab on the Token Saving page and are never persisted.
    The playground's Chat tab (`POST /api/playground/chat`) streams through the
    identical path — `token_saver::apply`, then `Executor::stream` — so its
    transcript is what a client would get, and it reports the answering tier and
    the pipeline's savings per turn. It is deliberately admin-guarded rather than
    key-guarded, so the playground still works when `server.require_api_key` is
    on and needs no pasted router key. The two handlers share
    `handlers::shared::playground_settings`, so an override the request path
    would reject cannot be demonstrated here.
    (`token_saver/`, `handlers/chat.rs`, `handlers/shared.rs`,
    `handlers/token_saver.rs`, `handlers/playground.rs`, `config.rs`,
    `db/repos/usage.rs`)

32. **The admin probe tolerates a provider without `/models`.** The models probe
    (`crates/alnair-router/src/upstream/probe.rs`) normally lists a connection's
    upstream via `GET {base_url}/models`, and both `POST /api/connections/{id}/test`
    and `POST /api/aliases/{id}/test` are built on it. Some OpenAI-compatible
    providers — CodeBuddy Intl is the shipped example — expose only
    `/chat/completions` and 404 on `/models`, which used to turn a working
    connection into a `502 upstream error`. When the `/models` probe 404s, the
    probe now POSTs a one-token request to the connection's chat route
    (`/chat/completions`, or `/messages` for `anthropic-native`): any response
    other than 404/5xx proves the route exists (401/403 mean the key was
    rejected, not the path), so the outcome is returned with `enumerable: false`
    and an empty model list. A 404 on the chat route, a 5xx, or an HTML body on
    `/models` still fails loudly — those mean a wrong `base_url`, not a provider
    that merely omits model listing. The flag rides along in the JSON
    (`enumerable` on the connection test, models list and alias test) so the
    dashboard says "model list not available" instead of "0 models" and the
    import dialog points the operator at manual alias entry. (`probe.rs`,
    `handlers/admin/mod.rs`, `apps/web/src/components/aliases/`)

33. **`serve` detaches when a terminal is waiting, and stops through a local
    control token.** The router used to block the shell that started it, so the
    terminal had to stay open for the router's whole life. `serve_command` now
    asks `daemon::should_detach(mode, console_attached)`: with a terminal it
    re-executes itself as `serve --foreground` with stdio pointed at
    `$ALNAIR_ROUTER_HOME/logs/router.log`, waits for `/api/health`, prints the
    pid/url/log and returns. `--foreground` / `ALNAIR_ROUTER_FOREGROUND=1` keep
    the old behaviour, `--detach` forces the hand-off, and a launch with no
    console (auto-start, double-click, a container) serves in place — PID 1
    never detaches, or the container would exit immediately. The tray is
    unaffected: the detached child is a GUI process with no console, so the icon
    appears and its Quit reaches the same signal as before. The serving process
    writes `router.pid` — two lines, the PID then the `host:port` it serves on,
    renamed into place so a reader never sees half a record — and a `PidFile`
    guard removes it on every exit path. Recording the address is what lets
    `stop`/`status` find the router however its port was chosen, including a
    `--port` flag that never reached `config.toml` (`load_config` applies that
    override in memory only, and `start` forwards it to the child). Liveness for
    `stop`/`restart` is the PID alone, so a hung router is still stoppable with
    `--force`; on Linux that means `kill -0` *plus* a zombie check, because a
    terminated process answers `kill -0` until its parent reaps it and `stop`
    would otherwise wait out its timeouts and misreport a clean shutdown.
    `status` additionally probes `/api/health` and says so when the process
    exists but does not answer. The process also generates
    `control.token`, a 32-byte secret that `stop`/`restart` send as
    `x-alnair-control` to `POST /api/admin/control/shutdown`. That route
    deliberately does **not** sit behind `require_admin_token`: it must work
    before any password or admin token exists, and a browser cannot read the
    token file, so loopback alone is not enough to stop the router. The handler
    answers `202` after notifying; `with_graceful_shutdown` drains the in-flight
    request, so the acknowledgement still reaches the caller. `stop` escalates to
    SIGTERM and then `--force` (SIGKILL / `TerminateProcess`). Two platform
    details are load-bearing: on Unix the child gets its own process group so
    closing the terminal cannot deliver SIGHUP, and on Windows the parent first
    clears `HANDLE_FLAG_INHERIT` on every handle it owns, because a shell hands
    its output over as an inheritable handle and a leaked copy would keep
    `alnair-router serve | more` from ever seeing end-of-input. (`cli/daemon/`,
    `cli/mod.rs`, `handlers/control.rs`, `state.rs`,
    `tests/cli_lifecycle.rs`)

---

## 5. The `alnair-llm` crate — read this

`crates/alnair-llm/` is a **vendored provider stack**, extracted from the router
so it can be versioned, tested, and swapped independently. It was originally a
trimmed copy of an upstream provider stack, kept in-repo so the workspace has no
path dependency outside itself.

**What was kept:** `types`, `model_config`, `provider`, and
`providers/{mod,common,sse,anthropic,openai,commandcode}`.

**What was dropped:** `normalize.rs`, `streaming.rs`, `handlers.rs`, `queue.rs`,
`router.rs` (unused by the provider stack), and the entire Ollama provider
(module, `ProviderType::Ollama` variant, `to_ollama_*` methods, tests).

**Consequences:**

- **Staleness.** There is no external upstream to sync fixes from; this crate is
  the source of truth. Roadmap P3.3 chose extraction over vendoring in-tree.
- **Size.** Providers are module folders, every file under the 800-LOC cap:
  `openai/{mod,request,chunks,tests}` and `anthropic/{mod,stream,tests}`. Keep
  it that way when adding code.

### The seam (this is the important part)

**Only `src/upstream/chat_backend.rs` may reference `alnair_llm`.** Everything
else goes through that module, which re-exports router-owned types
(`RouterMessage`, `StreamChunk`, `TokenUsage`, `GenerationOptions`, …).

A test enforces it:

```bash
cargo test -p alnair-router vendored_llm_layer_is_imported_from_exactly_one_file
```

The guard was verified to actually fail (not pass vacuously) by planting a
violating file during development. Keep it that way.

**Why it matters:** swapping provider implementations is a one-file change plus
one `Cargo.toml` line. `alnair-llm` is published as `publish = false`; make it
public when it stabilises.

---

## 6. Running it

```bash
cargo run -p alnair-router    # 127.0.0.1:7878 (from repo root)
# No config needed: secrets.key is generated under $ALNAIR_ROUTER_HOME on first
# run and a dashboard setup code is printed; set/override values with
# ALNAIR_ROUTER__SECTION__KEY when deploying (e.g. ALNAIR_ROUTER__SECRETS__KEY).
# From a terminal this detaches (see 33) and returns; --foreground stays here.
cargo test --workspace        # ~316 tests, ~35s (retry backoff + provider tests)
cargo clippy --workspace --all-targets
```

On Windows and macOS `serve` also shows a system tray icon (Open dashboard,
Quit; left-click opens the dashboard on Windows) unless `server.tray = false`
or `--no-tray`. The icon comes from `assets/alnair-white.ico` (`assets/
alnair-white.svg` is the source artwork); Linux has no tray support. A
background run keeps its icon: the detached child is a normal GUI process, and
Quit reaches the same `Notify` that `stop` does.

**Dashboard** — built assets are embedded, so `http://127.0.0.1:7878/` serves the
dashboard. For live development use Vite instead:

```bash
cd apps/web
pnpm install
pnpm dev                      # :5173, proxies /api and /v1 to the running router
                              # (ALNAIR_ROUTER_URL, else router.pid, else :7878)
pnpm run check                # vue-tsc --noEmit + production build (rebuild embeds it)
pnpm test                     # Vitest: api client, formatters, router
```

`build.rs` drops a placeholder `apps/web/dist/index.html` when the dashboard has
not been built, so `cargo build` never fails on a fresh clone.

**Docker** — the published image is multi-arch (amd64 + arm64) and assembled
from the release binaries by `Dockerfile.release`; `Dockerfile` is the
self-contained source recipe for a local build. Both run non-root with a `/data`
volume:

```bash
docker build -t alnair-router .                                        # from source
docker build -f Dockerfile.release -t alnair-router .                  # prebuilt binaries staged as linux-amd64/ + linux-arm64/
docker run --rm -p 7878:7878 -e ALNAIR_ROUTER__SECRETS__KEY=... -v alnair-data:/data alnair-router
```

**Real-provider tests** (opt-in, `#[ignore]`d):

```bash
ALNAIR_ROUTER_E2E_OPENAI_API_KEY=sk-... \
  cargo test -p alnair-router --test e2e_real -- --ignored
```

**Releasing** — root tooling, `standard-version`:

```bash
npm install
npm run release:dry     # preview
npm run release         # bump + CHANGELOG + manifest sync + commit + tag
git push --follow-tags origin main
```

`scripts/sync-version.mjs` (`postbump`) syncs `crates/*/Cargo.toml`,
`apps/web/package.json`, and `Cargo.lock`, and stages them into the release
commit. A pushed `v*` tag runs `.github/workflows/release.yml`, which builds the
dashboard + router for Linux/Windows/macOS and attaches the archives and
`SHA256SUMS.txt` to the GitHub Release. Local binary: `npm run build:binary`.

**Configuration** — `$ALNAIR_ROUTER_HOME/config.toml`, default `~/.alnair-router/`.
Every value is overridable via `ALNAIR_ROUTER__SECTION__KEY`, e.g.
`ALNAIR_ROUTER__SERVER__PORT=9000`. `secrets.key` is mandatory; the router
refuses to start without it. See `crates/alnair-router/router.example.toml`.

**Installing** — the binary is its own installer:

```bash
alnair-router install     # config with a generated secrets key + auto-start
alnair-router start       # start in the background (status reports the pid)
alnair-router status      # process, auto-start state and paths
alnair-router stop        # graceful stop; --force kills if it will not stop
alnair-router restart     # stop, then start again
alnair-router uninstall   # disable auto-start, keep data
```

`serve`, `start` and `restart` also take `--port <n>`, which overrides
`server.port` for that run only — nothing is written back to `config.toml`, and
the detached child inherits the flag. `stop`/`status` take no port: they read the
address the serving process recorded in `router.pid`.

`install`/`uninstall`/`status` use the `auto-launch` crate (Windows Run key,
macOS LaunchAgent, Linux XDG autostart). On Linux/macOS `install.sh` (also
served as `curl -fsSL .../main/install.sh | sh`) downloads the latest release,
verifies checksums, and installs it; on Windows `install.ps1` (also served as
`irm .../main/install.ps1 | iex`) downloads the release, unpacks it to
`%LOCALAPPDATA%\alnair-router\bin`, adds that to the user PATH, and runs
`install`. An existing config is never overwritten — a config without
`secrets.key` makes `install` fail loudly instead of regenerating it.

**State** — SQLite at `$ALNAIR_ROUTER_HOME/db/router.sqlite`, created and
migrated on first boot. Credentials are encrypted on the way in; legacy
plaintext rows are rewritten by `migrate_credentials`. `router.pid`,
`logs/router.log` and `control.token` also live here; see item 21.

### Configuring a working setup

```bash
# 1. Upstream
curl -X POST http://127.0.0.1:7878/api/connections -H 'content-type: application/json' \
  -d '{"name":"openai-main","provider_type":"openai-compatible",
       "base_url":"https://api.openai.com/v1","api_key":"sk-..."}'

# 2. Prefix → connection   (use the id returned above)
curl -X POST http://127.0.0.1:7878/api/aliases -H 'content-type: application/json' \
  -d '{"prefix":"oa","connection_id":"<id>"}'

# 3. Fallback chain
curl -X POST http://127.0.0.1:7878/api/combos -H 'content-type: application/json' \
  -d '{"name":"free-forever","entries":["oa/gpt-4o-mini","oa/gpt-4o"]}'

# 4. Use it
curl http://127.0.0.1:7878/v1/chat/completions -H 'content-type: application/json' \
  -d '{"model":"free-forever","messages":[{"role":"user","content":"hi"}]}'
```

Response headers report the routing decision:
`x-router-model`, `x-router-provider`, `x-router-attempt`, `x-router-source`.

---

## 7. Test map

| File | Covers |
|---|---|
| `crates/alnair-llm/src/**` (81) | Provider internals: OpenAI/Anthropic conversion, SSE parsing, tool-call repair, retry/backoff |
| `tests/resolve.rs` (24) | Prefix/alias/combo resolution, cycle detection, depth cap, disabled entries, tier numbering, bare alias-with-override names |
| `tests/storage.rs` (24) | Repository behaviour against real in-memory SQLite, cascade deletes, key hashing, Ollama rejection, credential encryption + boot migration, key limits/budget, spend rollups |
| `tests/routes.rs` | Endpoint shapes, `/v1` and `/api` auth enforcement, 404 vs 400, multi-megabyte bodies, SSRF guard, scheme rejection, probes, cache write-through, rate limit 429, budget 402/warn, key PATCH, metrics text, dashboard serving, upstream models/test probes (incl. HTML/missing-`/v1` diagnostics), alias chat probe, activity feed, the seam guard, Headroom probe, usage savings block, the token-saver playground (measured shrinkage, idle pipeline, directive cost, output-estimate opt-in, override validation, Headroom fail-open), and the playground chat stream (router/delta/usage frames, unknown-model error frame, override validation, admin guard) |
| `tests/fallback.rs` (4) | Failover ordering against an in-process mock upstream, connect/idle timeouts |
| `tests/streaming.rs` (5) | SSE translation: streamed tool calls + `finish_reason`, reasoning, opt-in usage chunk, and the Anthropic `tool_use`/`thinking` block sequence |
| `tests/e2e_real.rs` (3, `--ignored`) | Opt-in round trips against real OpenAI/Anthropic endpoints |
| `tests/cli_lifecycle.rs` (10) | The real binary: `serve --foreground` writes its pid + control token, a `--port` override is used, recorded, and still stoppable without repeating the flag, the control route refuses anything but the local token (including an admin bearer), `stop` shuts a foreground instance down and clears the pid file, `stop`/`status` are safe when nothing runs, a stale pid file is cleaned up, conflicting flags are rejected, and detaching does not hold the caller's stdout open (the `| more` regression) |
| `src/**` inline (49) | Crypto round-trips, retry policy, SSRF address checks, limiters, tool-call aggregation, catalog cache, metrics, upstream model matching, error-body summarization, activity tracker |
| `apps/web/src/**` | API client error/transport handling, formatters, route table (incl. the legacy-playground redirect), theme store, alias prefix helpers, confirm-dialog regression, topology layout, and the playground hub (page tab shell sharing overrides across tabs, the token-saver tab's run/measured-reduction/rejection/malformed-JSON/per-run toggles incl. the terse/caveman exclusion, the chat tab's streaming, error frame, system prompt and savings, and the SSE client's frame dispatch, split-frame reassembly, non-OK and in-band errors) |

---

## 8. Repo hygiene checklist

- [x] `LICENSE` (MIT) present; manifests declare it.
- [x] `git init` done; `.gitignore` covers `target/`, `data/`, `*.sqlite*`.
- [x] CI added (`.github/workflows/ci.yml`) — activate by pushing to GitHub.
- [x] The release workflow builds the multi-arch image, pushes it, and smoke-boots
      the amd64 image against `/api/health`; a local build still needs Docker.
- [ ] Re-run `cargo build --offline` from the repo root as a sanity check.

**Nothing is coupled.** No absolute paths, no sibling-directory references, no
shared workspace inheritance — verified by grep and by the offline build.
