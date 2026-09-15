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
| **Connection** | An upstream endpoint: `openai-compatible` or `anthropic-native`, with a base URL and key. |

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
- A multi-stage `Dockerfile` builds web + router and runs non-root with a
  `/data` volume. Not built in this environment (no Docker CLI); verify on a
  machine with Docker.
- Opt-in real-provider tests live in `tests/e2e_real.rs` (`--ignored`).

---

## 3. Architecture

```
Cargo.toml                    # virtual workspace root (members, profiles)
Cargo.lock                    # resolves entirely from crates.io
LICENSE                       # MIT
Dockerfile                    # web + router image, non-root, /data volume
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
│   ├── config.rs            # file + ALNAIR_ROUTER__SECTION__KEY env
│   ├── crypto.rs            # AES-256-GCM credential encryption + key parsing
│   ├── limits.rs            # concurrency semaphores + token buckets + budget mode
│   ├── metrics.rs           # atomic counters + Prometheus text exposition
│   ├── telemetry.rs         # in-memory activity feed (attempts, HTTP, events)
│   ├── error.rs             # scoped Error → OpenAI-shaped JSON error body
│   ├── state.rs             # AppState: config, pool, cipher, executor, repos
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
│   │   └── media.rs         # HTTP proxying for non-chat endpoints
│   ├── protocol/            # OpenAI ⇄ Anthropic wire translation
│   └── handlers/            # chat, messages, responses, models, media, admin, web
└── tests/                   # resolve, storage, fallback, routes, e2e_real
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
| `api_keys` | Router-issued client keys. Stores SHA-256 `key_hash`, never the secret. |
| `usage_records` | One row per attempt — failures included, not just successes. |

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
   pinned addresses and follows no redirects itself. (`handlers/media.rs`)

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
    month-to-date spend (one rollup query, only for keys that carry a budget).
    `null` on `rate_limit_per_minute` / `monthly_budget_usd` in a PATCH body
    clears the field; an absent field leaves it alone (`repos::double_option`).

13. **Non-streaming still flows through the chunk pipeline.** Handlers pass the
    client's `stream` flag to `Executor::stream` → `chat_backend::stream`, which
    selects `provider.stream` or `provider.complete`. Only Anthropic overrides
    `complete` (a real `stream: false` request) today; everyone else drains their
    SSE stream. `collect` aggregates tool calls instead of erroring, which is
    what makes tool use work with `stream: false`.

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
    `Start-Process -RedirectStandardOutput` — are left untouched. Without a
    console and without inherited handles (the auto-start case) output is
    discarded. The dashboard opener also spawns `cmd` with `CREATE_NO_WINDOW`,
    otherwise `cmd` would allocate a console just to launch the browser.

21. **Key rules are one policy, merged key-first.** `policy.rs` resolves an
    `ApiKey` plus its optional `KeyPlan` into a `KeyPolicy`: any field the key
    sets wins, the plan fills the rest, and a key budget always travels with its
    own mode (a key amount with `mode = off` disables the plan cap). The model
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
    by both `/api/usage` and `/api/usage/summary`, and the time range now
    narrows the table too, not just the cards. Rows snapshot the serving
    `connection_name` at write time (migration `0005`), so filtering stays
    meaningful after a connection is renamed or deleted; `resolved_provider`
    remains the wire protocol, not the connection. `/api/usage/facets` returns
    the distinct models and connections (most used first) plus providers for the
    dashboard's suggestions. (`db/repos/usage.rs`, `handlers/admin.rs`,
    `UsageFilterBar.vue`)

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

---

## 5. The `alnair-llm` crate — read this

`crates/alnair-llm/` is a **vendored provider stack**, extracted from the router
so it can be versioned, tested, and swapped independently. It was originally a
trimmed copy of an upstream provider stack, kept in-repo so the workspace has no
path dependency outside itself.

**What was kept:** `types`, `model_config`, `provider`, and `providers/{mod,common,sse,anthropic,openai}`.

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
export ALNAIR_ROUTER__SECRETS__KEY="$(openssl rand -hex 32)"   # required
cargo run -p alnair-router    # 127.0.0.1:7878 (from repo root)
cargo test --workspace        # 223 tests, ~33s (retry backoff + provider tests)
cargo clippy --workspace --all-targets
```

On Windows and macOS `serve` also shows a system tray icon (Open dashboard,
Quit; left-click opens the dashboard on Windows) unless `server.tray = false`
or `--no-tray`. The icon comes from `assets/alnair-white.ico` (`assets/
alnair-white.svg` is the source artwork); Linux has no tray support.

**Dashboard** — built assets are embedded, so `http://127.0.0.1:7878/` serves the
dashboard. For live development use Vite instead:

```bash
cd apps/web
pnpm install
pnpm dev                      # :5173, proxies /api and /v1 to ALNAIR_ROUTER_URL (:7878)
pnpm run check                # vue-tsc --noEmit + production build (rebuild embeds it)
pnpm test                     # Vitest: api client, formatters, router
```

`build.rs` drops a placeholder `apps/web/dist/index.html` when the dashboard has
not been built, so `cargo build` never fails on a fresh clone.

**Docker** — multi-stage image (web + router), non-root, `/data` volume:

```bash
docker build -t alnair-router .
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
alnair-router status      # auto-start state and paths
alnair-router uninstall   # disable auto-start, keep data
```

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
plaintext rows are rewritten by `migrate_credentials`.

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
| `tests/routes.rs` (41) | Endpoint shapes, `/v1` and `/api` auth enforcement, 404 vs 400, SSRF guard, scheme rejection, probes, cache write-through, rate limit 429, budget 402/warn, key PATCH, metrics text, dashboard serving, upstream models/test probes (incl. HTML/missing-`/v1` diagnostics), alias chat probe, activity feed, the seam guard |
| `tests/fallback.rs` (4) | Failover ordering against an in-process mock upstream, connect/idle timeouts |
| `tests/e2e_real.rs` (3, `--ignored`) | Opt-in round trips against real OpenAI/Anthropic endpoints |
| `src/**` inline (49) | Crypto round-trips, retry policy, SSRF address checks, limiters, tool-call aggregation, catalog cache, metrics, upstream model matching, error-body summarization, activity tracker |
| `apps/web/src/**` (33) | API client error/transport handling, formatters, route table, theme store, alias prefix helpers, confirm-dialog regression, topology layout |

---

## 8. Repo hygiene checklist

- [x] `LICENSE` (MIT) present; manifests declare it.
- [x] `git init` done; `.gitignore` covers `target/`, `data/`, `*.sqlite*`.
- [x] CI added (`.github/workflows/ci.yml`) — activate by pushing to GitHub.
- [ ] Build the Docker image once on a machine with Docker and smoke-test it.
- [ ] Re-run `cargo build --offline` from the repo root as a sanity check.

**Nothing is coupled.** No absolute paths, no sibling-directory references, no
shared workspace inheritance — verified by grep and by the offline build.
