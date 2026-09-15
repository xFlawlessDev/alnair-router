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

- **Builds and tests standalone.** 172 tests green, `cargo clippy --all-targets` clean.
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
open on loopback by default, there is no rate limiting (P1.2) or concurrency
limit (P1.1), and the dashboard is not yet served by the binary (P3.5).

---

## 3. Architecture

```
Cargo.toml                    # virtual workspace root (members, profiles)
Cargo.lock                    # resolves entirely from crates.io
apps/web/                     # admin dashboard (Vue 3 + Vite + Tailwind, npm)
crates/alnair-router/
├── Cargo.toml
├── migrations/0001_init.sql  # 6 tables
├── router.example.toml       # every config option
├── src/
│   ├── main.rs              # load config → cipher → connect DB → migrate → serve
│   ├── lib.rs               # module tree + shallow re-exports
│   ├── config.rs            # file + ALNAIR_ROUTER__SECTION__KEY env
│   ├── crypto.rs            # AES-256-GCM credential encryption + key parsing
│   ├── error.rs             # scoped Error → OpenAI-shaped JSON error body
│   ├── state.rs             # AppState: config, pool, cipher, executor, repos
│   ├── middleware.rs        # bearer auth for /v1/* and /api/*
│   ├── server.rs            # route table
│   ├── model/
│   │   ├── catalog.rs       # loads connections/aliases/combos from DB
│   │   └── resolver.rs      # pure: reference → ordered Vec<ResolvedTarget>
│   ├── db/
│   │   ├── mod.rs           # pool + embedded migrations
│   │   └── repos/           # connections, aliases, combos, api_keys, usage
│   ├── upstream/
│   │   ├── chat_backend.rs  # ← THE SEAM. Only file that may touch crate::llm
│   │   ├── executor.rs      # fallback walk + first-chunk peek
│   │   └── media.rs         # HTTP proxying for non-chat endpoints
│   ├── protocol/            # OpenAI ⇄ Anthropic wire translation
│   ├── handlers/            # chat, messages, responses, models, media, admin, shared
│   └── llm/                 # VENDORED provider stack — see §5
└── tests/                   # resolve, storage, fallback, routes
```

### Request flow

```
POST /v1/chat/completions { "model": "free-forever" }
  │
  ├─ middleware::require_api_key      (skipped if require_api_key = false)
  ├─ state.resolver()                 → loads catalog from SQLite EVERY REQUEST
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

---

## 5. The vendored `src/llm/` layer — read this

`src/llm/` is a **vendored provider stack**, copied in so the crate has no path
dependency on anything outside this repository.

**What was kept:** `types`, `model_config`, `provider`, and `providers/{mod,common,sse,anthropic,openai}`.

**What was dropped:** `normalize.rs`, `streaming.rs`, `handlers.rs`, `queue.rs`,
`router.rs` (unused by the provider stack), and the entire Ollama provider
(module, `ProviderType::Ollama` variant, `to_ollama_*` methods, tests).

**Two consequences you must accept or fix:**

- **Staleness.** There is no external upstream to sync fixes from; this copy is
  the source of truth. See roadmap P3.3.
- **Size.** `src/llm/providers/openai.rs` (~1745 LOC) and `anthropic.rs` (~1006 LOC)
  are the two largest files in the package and exceed the project's 800-LOC guideline.

### The seam (this is the important part)

**Only `src/upstream/chat_backend.rs` may reference `crate::llm`.** Everything
else goes through that module, which re-exports router-owned types
(`RouterMessage`, `StreamChunk`, `TokenUsage`, `GenerationOptions`, …).

A test enforces it:

```bash
cargo test vendored_llm_layer_is_imported_from_exactly_one_file
```

The guard was verified to actually fail (not pass vacuously) by planting a
violating file during development. Keep it that way.

**Why it matters:** if you later publish the provider layer as its own crate, it
is a one-file change plus one `Cargo.toml` line.

---

## 6. Running it

```bash
export ALNAIR_ROUTER__SECRETS__KEY="$(openssl rand -hex 32)"   # required
cargo run -p alnair-router    # 127.0.0.1:7878 (from repo root)
cargo test                    # 172 tests, ~35s (retry backoff + vendored provider tests)
cargo clippy --all-targets
```

**Admin dashboard** — the Vue app in `apps/web` talks to `/api/*`:

```bash
cd apps/web
npm ci
npm run dev                   # :5173, proxies /api and /v1 to ALNAIR_ROUTER_URL (:7878)
npm run check                 # vue-tsc --noEmit + production build
npm test                      # Vitest: api client, formatters, router
```

**Configuration** — `$ALNAIR_ROUTER_HOME/config.toml`, default `~/.alnair-router/`.
Every value is overridable via `ALNAIR_ROUTER__SECTION__KEY`, e.g.
`ALNAIR_ROUTER__SERVER__PORT=9000`. `secrets.key` is mandatory; the router
refuses to start without it. See `crates/alnair-router/router.example.toml`.

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
| `tests/resolve.rs` (21) | Prefix/alias/combo resolution, cycle detection, depth cap, disabled entries, tier numbering |
| `tests/storage.rs` (21) | Repository behaviour against real in-memory SQLite, cascade deletes, key hashing, Ollama rejection, credential encryption + boot migration |
| `tests/routes.rs` (20) | Endpoint shapes, `/v1` and `/api` auth enforcement, 404 vs 400, SSRF guard, scheme rejection, the vendored-layer seam guard |
| `tests/fallback.rs` (2) | Failover ordering against an in-process mock upstream |
| `src/**` inline (108) | Vendored provider internals, crypto round-trips, retry policy, SSRF address checks |
| `apps/web/src/**` (21) | API client error/transport handling, formatters, route table, theme store |

---

## 8. Repo hygiene checklist

- [ ] Add a `LICENSE` file. The workspace and crate declare `MIT OR Apache-2.0`
      but no license text ships with it.
- [ ] `git init`, and confirm `.gitignore` covers `target/`, `data/`, `*.sqlite*`
      (it already does).
- [ ] Add CI (roadmap P3.2) — the vendored provider tests need caching.
- [ ] Re-run `cargo build --offline` from the repo root as a sanity check.

**Nothing is coupled.** No absolute paths, no sibling-directory references, no
shared workspace inheritance — verified by grep and by the offline build.
