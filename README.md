# alnair-router

**One OpenAI-compatible endpoint in front of every model you use.** Prefixes
route to upstream connections, named combos expand into ordered fallback chains,
and every attempt is metered — with an embedded dashboard, no external database,
and a single binary.

[![CI](https://github.com/xFlawlessDev/alnair-router/actions/workflows/ci.yml/badge.svg)](https://github.com/xFlawlessDev/alnair-router/actions/workflows/ci.yml)
[![Release](https://github.com/xFlawlessDev/alnair-router/actions/workflows/release.yml/badge.svg)](https://github.com/xFlawlessDev/alnair-router/actions/workflows/release.yml)
[![npm](https://img.shields.io/npm/v/@xflawlessdev/alnair-router?label=npm)](https://www.npmjs.com/package/@xflawlessdev/alnair-router)
[![ghcr.io](https://img.shields.io/badge/ghcr.io-alnair--router-blue)](https://github.com/xFlawlessDev/alnair-router/pkgs/container/alnair-router)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](#license)

- **Model references** — `glm/glm-4.6`, a bare `gpt-4o`, or a combo name.
- **Fallback combos** — a tier that fails before emitting content moves to the next, transparently.
- **Aliases & connections** — one prefix per upstream, with round-robin API keys per connection.
- **Embedded dashboard** — connections, aliases, combos, catalog, keys, usage, settings, backup/restore.
- **Key controls** — rate limits, daily/weekly/monthly/lifetime budgets, model allowlists and plans.
- **Both wire formats** — OpenAI (`/v1/chat/completions`, `/v1/responses`) and Anthropic (`/v1/messages`).

## Install

The binary doubles as its own installer: `install` creates a config with a
generated `secrets.key` and registers auto-start, so an installed router comes
back by itself after a reboot.

**Linux / macOS** — Linux x86_64 or Apple Silicon:

```bash
curl -fsSL https://raw.githubusercontent.com/xFlawlessDev/alnair-router/main/install.sh | sh
```

`install.sh` resolves the latest GitHub Release for your platform, verifies
`SHA256SUMS.txt`, installs to `~/.local/bin`, and runs `install`.

**Windows** — x64:

```powershell
irm https://raw.githubusercontent.com/xFlawlessDev/alnair-router/main/install.ps1 | iex
```

`install.ps1` verifies `SHA256SUMS.txt`, installs to
`%LOCALAPPDATA%\alnair-router\bin`, adds that to your user PATH, and runs
`install`. From a checkout it prefers a local `target\release` (or
`target\debug`) build.

**npm** — Node 18+:

```bash
npm install -g @xflawlessdev/alnair-router
# or run it without installing:
npx @xflawlessdev/alnair-router
```

The wrapper ships prebuilt binaries for Linux x64 (glibc), Windows x64, and
Apple Silicon through optional platform packages — no postinstall download,
nothing is fetched at runtime. Alpine/musl is not covered; use the install
script or build from source there.

**Docker:**

```bash
export ALNAIR_ROUTER__SECRETS__KEY="$(openssl rand -hex 32)"
docker compose up -d
docker compose logs         # copy the setup code, then open /login
```

Overrides: `ALNAIR_ROUTER_REPO`, `ALNAIR_ROUTER_VERSION`,
`ALNAIR_ROUTER_INSTALL_DIR`, and `ALNAIR_ROUTER_NO_AUTOSTART=1` on the shell
scripts (`-Version`, `-Repo`, `-InstallDir`, `-NoAutoStart` on PowerShell).

Manage it with:

```bash
alnair-router status      # auto-start state and paths
alnair-router uninstall   # disable auto-start (config and data are kept)
```

`install`/`uninstall`/`status` work on every platform (auto-launch writes a Run
key, LaunchAgent, or XDG autostart entry); the install scripts only place the
binary and delegate to `install`. `uninstall` keeps config and data by design.
To remove the app completely, `alnair-router uninstall`, delete the install dir
(`%LOCALAPPDATA%\alnair-router` or `~/.local/bin/alnair-router`), the router home
(`~/.alnair-router`), and its PATH entry if you added one.

## What it does

Point any OpenAI-compatible client at the router and use a model reference:

| Reference | Resolves to |
|---|---|
| `glm/glm-4.6` | the `glm` alias → its connection, with model `glm-4.6` |
| `free-forever` | a combo → each entry in order, as fallback tiers |
| `gpt-4o` | the configured `default_connection` |

An alias that pins a model via its override can also be used as a bare model
name: with `kr → claude-4.5-sonnet`, `{"model": "kr"}` routes to that model
directly. Aliases without an override still need `prefix/model`.

When a tier fails **before any content is emitted**, the router transparently
moves to the next one. Responses report which tier answered via
`x-router-model`, `x-router-provider`, `x-router-attempt`, and
`x-router-source` headers.

## Quick start

No configuration is required: the router generates its encryption key at
`$ALNAIR_ROUTER_HOME/secrets.key` (default `~/.alnair-router/secrets.key`) on
first run and prints a dashboard setup code:

```bash
cargo run -p alnair-router
# alnair-router setup code: 8f3a-2b91-c4d7-e5f6
```

Open `http://127.0.0.1:7878/login`, paste the setup code and choose the
dashboard password. To override the generated key (or any other value), use
`config.toml` or env vars — e.g.
`ALNAIR_ROUTER__SECRETS__KEY="$(openssl rand -hex 32)"`.

The server listens on `127.0.0.1:7878`. Then configure an upstream and a
fallback chain (the admin API is open on loopback until a password is set):

```bash
# 1. An upstream endpoint
curl -X POST http://127.0.0.1:7878/api/connections \
  -H 'content-type: application/json' \
  -d '{
    "name": "openai-main",
    "provider_type": "openai-compatible",
    "base_url": "https://api.openai.com/v1",
    "api_key": "sk-..."
  }'

# 2. A prefix that maps to it
curl -X POST http://127.0.0.1:7878/api/aliases \
  -H 'content-type: application/json' \
  -d '{ "prefix": "oa", "connection_id": "<id-from-step-1>" }'

# 3. A fallback combo
curl -X POST http://127.0.0.1:7878/api/combos \
  -H 'content-type: application/json' \
  -d '{ "name": "free-forever", "entries": ["oa/gpt-4o-mini", "oa/gpt-4o"] }'
```

Now use it exactly like OpenAI:

```bash
curl http://127.0.0.1:7878/v1/chat/completions \
  -H 'content-type: application/json' \
  -d '{ "model": "free-forever", "messages": [{ "role": "user", "content": "hi" }] }'
```

## Repository layout

This repository is a standalone Cargo monorepo. A virtual workspace at the root
owns the lock file and build profiles; the router crate lives in
`crates/alnair-router/` and the provider stack in `crates/alnair-llm/`. It
depends only on crates.io — no path dependency outside the workspace — with its
own SQLite database and its own HTTP server.

```
.
├── Cargo.toml           # virtual workspace root
├── apps/
│   └── web/             # admin dashboard (Vue 3 + Vite)
├── crates/
│   ├── alnair-llm/      # provider stack (OpenAI-compatible + Anthropic-native)
│   └── alnair-router/   # the router crate (binary + library)
└── docs/                # HANDOVER.md, ROADMAP.md
```

## Admin dashboard

The Vue 3 dashboard in [`apps/web`](apps/web) manages everything the admin API
exposes — connections, aliases, combos, a model catalog with copyable ids and
prices, API keys, usage, runtime settings, and database backup/restore — and the
built assets are **embedded into the router binary**: with a production build,
open `http://127.0.0.1:7878/` and the dashboard is there. Set
`server.serve_dashboard = false` when a reverse proxy serves it instead.

For development, run it with Vite against the live router:

```bash
cd apps/web
pnpm install
pnpm dev           # http://localhost:5173, proxies /api and /v1 to :7878
```

Set `ALNAIR_ROUTER_URL` to point the dev proxy at a different router. See
[`apps/web/README.md`](apps/web/README.md).

### System tray

On Windows and macOS `alnair-router` runs with a tray icon: **Open dashboard**
and **Quit** (graceful shutdown), and on Windows left-click opens the dashboard
directly. Disable it with `--no-tray` or `server.tray = false`; Linux always
serves headless.

Windows builds are GUI-subsystem binaries, so no console window appears on
auto-start or double-click. Run the binary from a terminal and CLI output plus
logs attach to that terminal as usual.

## Docker

Every `v*` tag publishes an image to GHCR (`ghcr.io/xflawlessdev/alnair-router`).
`docker-compose.yml` pulls the latest release and runs it as a non-root user
with `/data` as the state volume:

```bash
export ALNAIR_ROUTER__SECRETS__KEY="$(openssl rand -hex 32)"
docker compose up -d
docker compose logs         # copy the setup code, then open /login
docker compose pull         # pick up a newer release
```

The compose file enables LAN access inside the container (a published port
cannot reach a loopback bind) and keeps `/v1` closed to anyone without a key.
The first time, `docker compose logs` prints `alnair-router setup code: …`; use
it at `/login` to create the dashboard password. See
[Exposing beyond loopback](#exposing-beyond-loopback).

Pin a version with `image: ghcr.io/xflawlessdev/alnair-router:vX.Y.Z`, or build
from source instead with `docker build -t alnair-router .`.

The GHCR package starts private; make it public in the repository's package
settings for anonymous pulls.

## Exposing beyond loopback

The dashboard is protected by a **password** (no username). First-run flow:

1. Start the router; when no password exists the log prints a one-time code:
   `alnair-router setup code: 8f3a-2b91-c4d7-e5f6`.
2. Open `/login`, paste the code and choose a password (8+ characters). Sessions
   use rotating access/refresh tokens stored hashed in SQLite; replaying a
   rotated refresh token revokes that whole session family.
3. In **Settings → Security**, turn on **LAN access**. The listener re-binds to
   every interface immediately and admin routes start requiring sign-in. The
   login page and `/api/auth/status` stay reachable so other devices can sign
   in once someone enables it.

`/v1` is separate: it takes router-issued client keys. Set
`server.require_api_key = true` (Settings or config) before exposing the
network, or anyone who can reach the port can spend upstream credits. Keys are
minted on the API Keys page (hashed for lookup, with an encrypted copy kept so
the dashboard can reveal them, revocable, rate limits and budgets per key).

Deployment notes:

- `server.admin_token` still works for scripts and CI; when set it is accepted
  alongside password sessions. `server.allow_unauthenticated_admin` opts out of
  both (trusted networks only).
- TLS: the router speaks HTTP only. Terminate TLS in front of it — Caddy or
  nginx on the same host (`reverse_proxy 127.0.0.1:7878`, keeping the router on
  loopback), a Cloudflare tunnel
  (`cloudflared tunnel --url http://127.0.0.1:7878`), or Tailscale/WireGuard for
  private access.
- `server.cors_origins` gates browser cross-origin calls: empty (default) emits
  no CORS headers, `["*"]` allows any origin, otherwise an explicit allowlist.
  It can be edited on the Settings page without a restart. Non-browser clients
  are unaffected.
- `/api/health` and `/api/ready` stay public and carry no secrets.
- `server.public_usage` (default true) exposes `/me` and `/api/public/*`, which
  still require a valid client key; turn it off if clients should not
  self-serve.
- `server.host`/`server.port`, `server.serve_dashboard` and `secrets.key` remain
  file/env values that need a restart.

Behind a same-host proxy the router can keep `host = "127.0.0.1"`, so only the
proxy is reachable from the network. In Docker, publish the port explicitly
(`-p 127.0.0.1:7878:7878` for a host proxy, `-p 7878:7878` for LAN) and set
`ALNAIR_ROUTER__SERVER__HOST=0.0.0.0` inside the container.

## Endpoints

**OpenAI-compatible:**

| Endpoint | Notes |
|---|---|
| `POST /v1/chat/completions` | Core. Streaming (SSE) and non-streaming. |
| `POST /v1/responses` | OpenAI Responses shape. |
| `GET /v1/models` | Lists configured aliases and combos. |
| `GET /v1/models/info` | Per-reference metadata, including resolved combo tiers. |
| `POST /v1/embeddings` | Proxied to the resolved connection. |
| `POST /v1/images/generations` | Proxied. |
| `POST /v1/audio/speech`, `/v1/audio/transcriptions` | Proxied (multipart pass-through). |
| `POST /v1/videos/generations`, `GET /v1/videos/{id}` | Proxied, including async job polling. |
| `POST /v1/search`, `POST /v1/web/fetch` | Proxied / server-side fetch (SSRF-guarded; see caveat below). |

**Anthropic-compatible:**

| Endpoint | Notes |
|---|---|
| `POST /v1/messages` | Translates to the shared executor; streams with proper Anthropic event ordering. |
| `POST /v1/messages/count_tokens` | Heuristic estimate (no tokenizer dependency). |

**Admin:** `/api/health`, `/api/version`, `/api/init`, `/api/connections`,
`/api/aliases`, `/api/combos`, `/api/keys`, `/api/plans`, `/api/usage`,
`/api/usage/summary`, `/api/usage/facets`, `/api/usage/keys` (spend per key,
split by budget window, for the dashboard's budget monitor), `/api/models`
(provider + model + price catalog), `/api/pricing`, `/api/pricing/sync`,
`/api/settings`, `/api/backup`, `/api/restore`, `/api/metrics`,
`/api/activity`, `/api/providers` (built-in endpoint presets).
`/api/connections/{id}/accounts` manages extra API keys for one connection:
the primary key and enabled accounts rotate round-robin per request, and a
failing key falls through to the next before the tier is abandoned.
**Auth:** `/api/auth/status`, `/api/auth/setup`, `/api/auth/login` and
`/api/auth/refresh` are public; `/api/auth/logout` and
`PATCH /api/auth/password` need a session. The dashboard signs in with a
password only (no username) and rotates access/refresh tokens server-side.
`PATCH /api/keys/{id}` edits a key's name, enabled state, rate limit,
daily/weekly/monthly/lifetime budgets, model allowlist, plan and expiry;
`/api/plans` manages the reusable rule sets.
Usage reads accept `api_key_id`, `model` (case-insensitive substring),
`provider` (the wire protocol: `openai-compatible` / `anthropic-native`),
`connection` (the upstream that served the attempt) and `since`, and
`/api/usage/facets` lists the distinct models, providers and connections the
dashboard offers as filter options. Pricing rate lookups strip a `vendor/`
prefix and fall back to a connection's `pricing_model` when the upstream id is a
relay path; `GET /api/pricing/match?model=…` reports which catalog key answers
an id. Usage rows keep prompt/completion/cached/reasoning tokens plus
input/output/reasoning cost components, which the dashboard shows as breakdown
popovers. Upstream diagnostics used by
the dashboard:
`GET /api/connections/{id}/models` lists the models an upstream offers,
`POST /api/connections/{id}/test` checks connectivity, and
`POST /api/aliases/{id}/test` verifies an alias' connection and model override;
`POST /api/aliases/{id}/test-chat` runs one real completion through the alias.
`GET /api/activity` is the in-memory live feed (in-flight attempts,
per-connection counters, recent events) behind the Usage live panel and the
Console page.

**Public (client key):** `GET /api/public/usage` and `GET /api/public/models` —
outside the admin-token guard. The caller authenticates with a router-issued
key (`Authorization: Bearer sk-router-…`). Usage returns only its own rollup:
the summary, a per-model breakdown and a per-(bucket, model) time series
(`?bucket=hour|day`, default `day`, plus optional `since`/`until`). Models
returns the catalog rows the key's allowlist (key or plan) can reach, with
rates, and the page shows the OpenAI-compatible `/v1` base URL above the table
for easy copying. They back the self-service page at `/me`,
which stacks models in one bar chart, offers quick ranges and a month selector,
and refreshes itself every 30 seconds. Disabled with
`server.public_usage = false`.

> Admin routes are open on loopback by default, because they mint the keys that
> authenticate `/v1/*`. Set `server.admin_token` to require
> `Authorization: Bearer <token>` on every `/api/*` request — this is enforced
> whenever the token is configured. A non-loopback bind refuses to start
> without one unless `server.allow_unauthenticated_admin = true` is set
> explicitly.

## Configuration

Reads `$ALNAIR_ROUTER_HOME/config.toml` (default `~/.alnair-router/config.toml`),
overridable by `ALNAIR_ROUTER__SECTION__KEY` env vars — e.g.
`ALNAIR_ROUTER__SERVER__PORT=9000`.

Key settings:

- `secrets.key` — AES-256-GCM key for upstream credentials at rest. Optional:
  when unset the router generates `$ALNAIR_ROUTER_HOME/secrets.key` on first
  run and reuses it. Legacy plaintext rows are re-encrypted on boot.
- `server.store_key_secrets` (default `true`) — keep a reversible copy of every
  router-issued client key so the dashboard can reveal it. Off means a key is
  only readable at creation time; use it when you would rather the router not
  hold client keys in recoverable form. This also protects client keys, so back
  up `secrets.key` together with the database.
- `server.admin_token` — optional bearer token for scripts and CI. The
  dashboard itself signs in with a password (see
  [Exposing beyond loopback](#exposing-beyond-loopback)).
- `router.max_retries_per_tier` (default 2) and `router.max_retry_delay_ms`
  (default 30000) — provider retries inside one tier before failover, with
  exponential backoff.
- `limits.max_concurrent` / `limits.max_concurrent_per_connection` — upstream
  concurrency caps; a request that cannot get a slot in time gets `429` with
  `Retry-After`.
- `rate_limit.requests_per_minute` — default per-key token bucket (0 = off);
  keys can override it, and can carry daily, weekly, monthly and lifetime
  budgets in USD and token limits (calendar windows, UTC; lifetime never
  resets) with `off`/`warn`/`block` enforcement. Token limits count prompt +
  completion tokens.
- Key **rules** — each key can restrict the models it may call (exact names or
  `openai/*` / `*` wildcards, enforced with `403` on every `/v1` endpoint that
  carries a model). Rules set on a key win over its plan; bundle allowlist,
  rate limit and budget into a reusable **plan** and apply it to any key from
  the dashboard's API Keys page (`/api/plans`), where the allowlist editor
  searches the available aliases and combos. Keys and plans can also carry an
  `expires_at`: an expired key is rejected with `401`, and keys attached to an
  expired plan fail closed with `403` until the plan is extended.
- `router.catalog_ttl_ms` (default 1000) — routing-catalog cache; admin writes
  invalidate it immediately.
- `router.connect_timeout_ms` / `router.idle_timeout_ms` — default upstream
  first-byte and stream-idle timeouts; each connection can override or disable
  them (`0`).
- `server.readiness_upstream_checks` — makes `/api/ready` report TCP
  reachability counts for enabled connections.
- `server.public_usage` (default true) — exposes the self-service page at `/me`
  and `GET /api/public/usage`, where a client reads its own rollup (summary
  plus per-model totals) with a router-issued API key.
- `server.lan_access` (default false) — bind every interface so the LAN can
  reach the router; flippable from Settings and the listener re-binds without a
  restart. Admin routes then require the dashboard password.
- `server.cors_origins` — browser cross-origin allowlist (editable on the
  Settings page); empty (default) emits no CORS headers at all, `["*"]` allows
  any origin, otherwise only the listed origins are answered.
- `server.tray` (default true) — system tray icon with **Open dashboard** and
  **Quit** on Windows and macOS; `alnair-router --no-tray` disables it for one
  run.
- `pricing.sync_enabled` (default false) + `pricing.sync_interval_secs`
  (86400) — crawl `pricing.source_url` (LiteLLM or models.dev payload) for
  model rates. Dashboard overrides win over crawled rows, which in turn shadow
  the built-in rate table.

`GET /api/health` is a liveness probe (no database touch); `GET /api/ready`
checks the database. `GET /api/metrics` exposes Prometheus-style counters and is
guarded like the rest of `/api/*`.

A subset of the settings above — client/admin auth, routing, limits, rate limits
and pricing — can be edited from the dashboard's **Settings** page. Overrides
live in the router database, apply immediately without a restart, and take
precedence over `config.toml`/env until you reset them.

The same page backs up and restores data: **Download backup** streams a
consistent SQLite snapshot (`GET /api/backup`) and **Import backup** replaces
every data table inside one transaction (`POST /api/restore`). Imports validate
the file and its credentials, and leave runtime settings and the admin token
untouched.

See `crates/alnair-router/router.example.toml` for every option.

## Provider support

| `provider_type` | Status |
|---|---|
| `openai-compatible` | Supported |
| `anthropic-native` | Supported |
| `command-code` | Supported — Command Code's Provider API, falling back to its CLI transport when the plan has no API access (Go) |
| anything else (including `ollama`) | **Rejected at write time** with `400 unsupported_provider_type` |

## Architecture

```
crates/alnair-router/src/
├── crypto.rs            # AES-256-GCM credential encryption at rest
├── desktop/             # system tray (Windows/macOS): event loop, menu, icon
├── model/cache.rs       # cached routing catalog (TTL + invalidation)
├── model/resolver.rs    # pure resolution: reference → ordered targets
├── upstream/
│   ├── chat_backend.rs  # ← the ONLY file that may touch `alnair_llm`
│   ├── executor.rs      # fallback walk, permits, timeouts, first-chunk peek
│   └── media.rs         # HTTP proxying for non-chat endpoints
├── protocol/            # OpenAI ⇄ Anthropic wire translation
├── handlers/            # HTTP handlers + embedded dashboard serving
├── db/repos/            # SQLite repositories
└── server.rs            # route table
```

### Standalone by design

The workspace builds with **no path dependency on anything outside this
repository** — `Cargo.lock` resolves entirely from crates.io. The provider stack
(OpenAI-compatible + Anthropic-native) lives in `crates/alnair-llm`, with the
Ollama provider and the RAG/queue/handler layers removed.

Every coupling point to that crate is funnelled through the single module
`src/upstream/chat_backend.rs`; no other file names `alnair_llm`. A test enforces
this:

```bash
cargo test -p alnair-router vendored_llm_layer_is_imported_from_exactly_one_file
```

That keeps the provider layer swappable: replacing it is a one-file change plus
a single `Cargo.toml` line.

## Development

```bash
# Router
cargo check --workspace
cargo test --workspace

# Dashboard
cd apps/web
pnpm run check     # vue-tsc + production build
pnpm test
```

The Rust suite covers pure resolution, repository behaviour against a real
in-memory SQLite, fallback ordering against an in-process mock upstream, and
endpoint shape/auth over the real Axum app. The web suite covers the API client,
formatters, and routing.

Opt-in tests against real providers live in `tests/e2e_real.rs` and are
`#[ignore]`d:

```bash
ALNAIR_ROUTER_E2E_OPENAI_API_KEY=sk-... \
  cargo test -p alnair-router --test e2e_real -- --ignored
```

## Documentation

- [`docs/HANDOVER.md`](docs/HANDOVER.md) — architecture, design decisions, how to run it
- [`docs/ROADMAP.md`](docs/ROADMAP.md) — what is done, what is missing, what is unsafe

## Releasing

Versioning is driven by [standard-version](https://github.com/conventional-changelog/standard-version)
from conventional commits (root tooling only — the shipped artifact is the Rust
binary):

```bash
npm install            # root release tooling
npm run release:dry    # preview the bump and changelog
npm run release        # bump, changelog, sync manifests, commit, tag
git push --follow-tags origin main
```

`scripts/sync-version.mjs` (the `postbump` hook) keeps `crates/*/Cargo.toml`,
`apps/web/package.json`, the `npm/*/package.json` manifests (including the
platform `optionalDependencies`), and `Cargo.lock` in lockstep and stages them
so the release commit carries every manifest.

Pushing a `v*` tag runs `.github/workflows/release.yml`: it builds the dashboard
and the router for Linux x86_64, Windows x86_64, and macOS arm64, packages each
target (`tar.gz`/`zip`), and attaches the archives plus `SHA256SUMS.txt` to the
GitHub Release for the tag. The same run publishes the container image to GHCR
and the npm wrapper (`@xflawlessdev/alnair-router` plus one binary package per
platform) with provenance; the npm job needs an `NPM_TOKEN` repository secret
with publish rights to the `@xflawlessdev` scope.

For a local release binary with the embedded dashboard:

```bash
npm run build:binary   # pnpm -C apps/web run build && cargo build --release --locked -p alnair-router
```

## License

[MIT](LICENSE). The dashboard under `apps/web` is scaffolded from the EvoFast
`vue-tailwind-vite` template, also MIT.
