# alnair-router

A standalone, OpenAI-compatible AI router: it resolves **prefixed model IDs** to
upstream providers, expands named **combos** into ordered fallback chains, and
tracks usage per attempt.

This repository is a standalone Cargo monorepo. A virtual workspace at the root
owns the lock file and build profiles, and the router crate lives in
`crates/alnair-router/`. It depends only on crates.io — no path dependencies —
with its own SQLite database and its own HTTP server.

```
.
├── Cargo.toml           # virtual workspace root
├── apps/
│   └── web/             # admin dashboard (Vue 3 + Vite)
├── crates/
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

A Vue 3 dashboard in [`apps/web`](apps/web) manages everything the admin API
exposes — connections, aliases, combos, API keys, and usage — without curl.

```bash
cd apps/web
npm ci
npm run dev        # http://localhost:5173, proxies /api and /v1 to :7878
```

Set `ALNAIR_ROUTER_URL` to point the dev proxy at a different router. See
[`apps/web/README.md`](apps/web/README.md).

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
`/api/aliases`, `/api/combos`, `/api/keys`, `/api/usage`, `/api/usage/summary`.

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
├── llm/                 # vendored provider stack (OpenAI + Anthropic native)
├── crypto.rs            # AES-256-GCM credential encryption at rest
├── model/resolver.rs    # pure resolution: reference → ordered targets
├── upstream/
│   ├── chat_backend.rs  # ← the ONLY file that may touch `crate::llm`
│   ├── executor.rs      # fallback walk + first-chunk peek
│   └── media.rs         # HTTP proxying for non-chat endpoints
├── protocol/            # OpenAI ⇄ Anthropic wire translation
├── handlers/            # HTTP handlers
├── db/repos/            # SQLite repositories
└── server.rs            # route table
```

### Standalone by design

The workspace builds with **no path dependency on anything outside this
repository** — `Cargo.lock` resolves entirely from crates.io. The provider stack
it needs (OpenAI-compatible + Anthropic-native) is vendored under `src/llm`,
with the Ollama provider and the RAG/queue/handler layers removed.

Every coupling point to that layer is funnelled through the single module
`src/upstream/chat_backend.rs`; no other file names `crate::llm`. A test enforces
this:

```bash
cargo test -p alnair-router vendored_llm_layer_is_imported_from_exactly_one_file
```

That keeps the provider layer swappable: replacing the vendored copy with a
published crate is a one-file change plus a single `Cargo.toml` line.

## Development

```bash
# Router
cargo check
cargo test

# Dashboard
cd apps/web
npm run check      # vue-tsc + production build
npm test
```

The Rust suite covers pure resolution, repository behaviour against a real
in-memory SQLite, fallback ordering against an in-process mock upstream, and
endpoint shape/auth over the real Axum app. The web suite covers the API client,
formatters, and routing.
