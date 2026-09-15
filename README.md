# alnair-router

A standalone, OpenAI-compatible AI router: it resolves **prefixed model IDs** to
upstream providers, expands named **combos** into ordered fallback chains, and
tracks usage per attempt.

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

Upstream credentials are encrypted at rest, so the router needs a key before it
will start:

```bash
# Required. 32 bytes as 64 hex characters (or base64). Keep it safe.
export ALNAIR_ROUTER__SECRETS__KEY="$(openssl rand -hex 32)"

cargo run -p alnair-router
```

The server listens on `127.0.0.1:7878`. Then configure an upstream and a
fallback chain:

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

## Admin dashboard

The Vue 3 dashboard in [`apps/web`](apps/web) manages everything the admin API
exposes — connections, aliases, combos, API keys, and usage — and the built
assets are **embedded into the router binary**: with a production build, open
`http://127.0.0.1:7878/` and the dashboard is there. Set
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

The image builds the dashboard and the router (embedded assets), then runs as a
non-root user with `/data` as the state volume:

```bash
docker build -t alnair-router .
docker run --rm -p 7878:7878 \
  -e ALNAIR_ROUTER__SECRETS__KEY="$(openssl rand -hex 32)" \
  -v alnair-data:/data \
  alnair-router
```

## Endpoints

**OpenAI-compatible:**

| Endpoint | Notes |
|---|---|
| `POST /v1/chat/completions` | Core. Streaming (SSE) and non-streaming. |
| `POST /v1/responses` | OpenAI Responses shape. |
| `GET /v1/models` | Lists aliases, combos, and connections. |
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
`/api/usage/summary`, `/api/metrics`, `/api/activity`. `PATCH /api/keys/{id}`
edits a key's name, enabled state, rate limit, monthly budget, model allowlist
and plan; `/api/plans` manages the reusable rule sets. Upstream diagnostics used
by the dashboard:
`GET /api/connections/{id}/models` lists the models an upstream offers,
`POST /api/connections/{id}/test` checks connectivity, and
`POST /api/aliases/{id}/test` verifies an alias' connection and model override;
`POST /api/aliases/{id}/test-chat` runs one real completion through the alias.
`GET /api/activity` is the in-memory live feed (in-flight attempts,
per-connection counters, recent events) behind the Usage live panel and the
Console page.

> Admin routes are open on loopback by default, because they mint the keys that
> authenticate `/v1/*`. Set `server.admin_token` to require
> `Authorization: Bearer <token>` on every `/api/*` request — this is enforced
> whenever the token is configured. A non-loopback bind refuses to start
> without one unless `server.allow_unauthenticated_admin = true` is set
> explicitly.

## Documentation

- [`docs/HANDOVER.md`](docs/HANDOVER.md) — architecture, design decisions, how to run it
- [`docs/ROADMAP.md`](docs/ROADMAP.md) — what is done, what is missing, what is unsafe

## Configuration

Reads `$ALNAIR_ROUTER_HOME/config.toml` (default `~/.alnair-router/config.toml`),
overridable by `ALNAIR_ROUTER__SECTION__KEY` env vars — e.g.
`ALNAIR_ROUTER__SERVER__PORT=9000`.

Key settings:

- `secrets.key` — **required**; AES-256-GCM key for upstream credentials at rest.
  Legacy plaintext rows are re-encrypted on boot.
- `server.admin_token` — optional bearer token enforced on `/api/*`.
- `router.max_retries_per_tier` (default 2) and `router.max_retry_delay_ms`
  (default 30000) — provider retries inside one tier before failover, with
  exponential backoff.
- `limits.max_concurrent` / `limits.max_concurrent_per_connection` — upstream
  concurrency caps; a request that cannot get a slot in time gets `429` with
  `Retry-After`.
- `rate_limit.requests_per_minute` — default per-key token bucket (0 = off);
  keys can override it, and can carry a monthly budget with `off`/`warn`/`block`
  enforcement.
- Key **rules** — each key can restrict the models it may call (exact names or
  `openai/*` / `*` wildcards, enforced with `403` on every `/v1` endpoint that
  carries a model). Rules set on a key win over its plan; bundle allowlist,
  rate limit and budget into a reusable **plan** and apply it to any key from
  the dashboard's API Keys page (`/api/plans`), where the allowlist editor
  searches the available aliases and combos.
- `router.catalog_ttl_ms` (default 1000) — routing-catalog cache; admin writes
  invalidate it immediately.
- `router.connect_timeout_ms` / `router.idle_timeout_ms` — default upstream
  first-byte and stream-idle timeouts; each connection can override or disable
  them (`0`).
- `server.readiness_upstream_checks` — makes `/api/ready` report TCP
  reachability counts for enabled connections.
- `server.tray` (default true) — system tray icon with **Open dashboard** and
  **Quit** on Windows and macOS; `alnair-router --no-tray` disables it for one
  run.

`GET /api/health` is a liveness probe (no database touch); `GET /api/ready`
checks the database. `GET /api/metrics` exposes Prometheus-style counters and is
guarded like the rest of `/api/*`.

See `crates/alnair-router/router.example.toml` for every option.

## Provider support

| `provider_type` | Status |
|---|---|
| `openai-compatible` | Supported |
| `anthropic-native` | Supported |
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

## Install and auto-start

The binary doubles as its own installer: `install` creates a config with a
generated `secrets.key` and registers auto-start, so an installed router comes
back by itself after a reboot.

**Linux / macOS:**

```bash
curl -fsSL https://raw.githubusercontent.com/xFlawlessDev/alnair-router/main/install.sh | sh
```

`install.sh` resolves the latest GitHub Release for your platform (Linux x86_64
or Apple Silicon), verifies `SHA256SUMS.txt`, installs to `~/.local/bin`, and
runs `install`. Overrides: `ALNAIR_ROUTER_REPO`, `ALNAIR_ROUTER_VERSION`,
`ALNAIR_ROUTER_INSTALL_DIR`, and `ALNAIR_ROUTER_NO_AUTOSTART=1`.

**Windows:**

```powershell
irm https://raw.githubusercontent.com/xFlawlessDev/alnair-router/main/install.ps1 | iex
```

`install.ps1` downloads the latest release for Windows x64, verifies
`SHA256SUMS.txt`, installs to `%LOCALAPPDATA%\alnair-router\bin`, adds that to
your user PATH, and runs `install`. From a checkout it prefers a local
`target\release` (or `target\debug`) build; same overrides as above
(`-Version`, `-Repo`, `-InstallDir`, `-NoAutoStart`).

Manage it with:

```powershell
alnair-router status      # auto-start state and paths
alnair-router uninstall   # disable auto-start (config and data are kept)
```

`uninstall` keeps config and data by design. To remove the app completely,
`alnair-router uninstall`, delete the install dir (`%LOCALAPPDATA%\alnair-router`
or `~/.local/bin/alnair-router`), the router home (`~/.alnair-router`), and its
PATH entry if you added one.

`install`/`uninstall`/`status` work on every platform (auto-launch writes a Run
key, LaunchAgent, or XDG autostart entry); the install scripts only place the
binary and delegate to `install`.

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
`apps/web/package.json`, and `Cargo.lock` in lockstep and stages them so the
release commit carries every manifest.

Pushing a `v*` tag runs `.github/workflows/release.yml`: it builds the dashboard
and the router for Linux x86_64, Windows x86_64, and macOS arm64, packages each
target (`tar.gz`/`zip`), and attaches the archives plus `SHA256SUMS.txt` to the
GitHub Release for the tag.

For a local release binary with the embedded dashboard:

```bash
npm run build:binary   # pnpm -C apps/web run build && cargo build --release --locked -p alnair-router
```

## License

[MIT](LICENSE). The dashboard under `apps/web` is scaffolded from the EvoFast
`vue-tailwind-vite` template, also MIT.
