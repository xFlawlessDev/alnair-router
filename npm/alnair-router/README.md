# alnair-router

**One OpenAI-compatible endpoint in front of every model you use.** Prefixes
route to upstream connections, named combos expand into ordered fallback chains,
and every attempt is metered — with an embedded dashboard, no external database,
and a single Rust binary that stays lightweight and fast.

[![CI](https://github.com/xFlawlessDev/alnair-router/actions/workflows/ci.yml/badge.svg)](https://github.com/xFlawlessDev/alnair-router/actions/workflows/ci.yml)
[![Release](https://github.com/xFlawlessDev/alnair-router/actions/workflows/release.yml/badge.svg)](https://github.com/xFlawlessDev/alnair-router/actions/workflows/release.yml)
[![ghcr.io](https://img.shields.io/badge/ghcr.io-alnair--router-blue)](https://github.com/xFlawlessDev/alnair-router/pkgs/container/alnair-router)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](#license)

- **Model references** — `glm/glm-4.6`, a bare `gpt-4o`, or a combo name.
- **Fallback combos** — a tier that fails before emitting content moves to the next, transparently.
- **Aliases & connections** — one prefix per upstream, with round-robin API keys per connection.
- **Embedded dashboard** — connections, aliases, combos, catalog, keys, usage, settings, backup/restore.
- **Key controls** — rate limits, daily/weekly/monthly/lifetime budgets, model allowlists and plans.
- **Token saving** — a deterministic pipeline compresses bulky tool output and injects concise-output directives, with savings measured and priced per request.
- **Both wire formats** — OpenAI (`/v1/chat/completions`, `/v1/responses`) and Anthropic (`/v1/messages`).

## Install

Node 18 or newer:

```bash
npm install -g @xflawlessdev/alnair-router
```

Or run it without installing:

```bash
npx @xflawlessdev/alnair-router
```

Prebuilt binaries ship for Linux x64 (glibc), macOS arm64 (Apple Silicon) and
Windows x64 as optional platform packages, so nothing is downloaded at install
time and nothing is fetched at runtime. Alpine/musl is not covered — use the
[install script](https://github.com/xFlawlessDev/alnair-router#install) or build
from source there.

## Quick start

The binary is the whole CLI; the npm package is just a launcher for it.

```bash
alnair-router                     # serve in the background (default)
alnair-router --port 9000         # listen on another port for this run
alnair-router status              # is it running, and where
alnair-router stop                # graceful shutdown
alnair-router serve --foreground  # stay in this terminal instead
alnair-router install             # start automatically at login
```

A run started from a terminal detaches, so the shell comes back as soon as the
router is listening and closing the terminal does not stop it. Everything it
writes lives under `~/.alnair-router` (or `$ALNAIR_ROUTER_HOME`):

| File | Purpose |
|---|---|
| `config.toml` | Settings; every value has an `ALNAIR_ROUTER__SECTION__KEY` override |
| `secrets.key` | Generated on first run; encrypts upstream credentials at rest |
| `logs/router.log` | Output of a background run |
| `router.pid` | PID and address of the running router |

The first run also generates a one-time dashboard setup code. Background runs
log it, so:

```bash
tail -f ~/.alnair-router/logs/router.log   # Get-Content -Wait on Windows
```

Open `http://127.0.0.1:7878/login`, paste the setup code and choose a password.
Then add a connection and an alias in the dashboard, or over the admin API.

## Use it

Point any OpenAI-compatible client at the router and use a model reference:

| Reference | Resolves to |
|---|---|
| `glm/glm-4.6` | the `glm` alias → its connection, with model `glm-4.6` |
| `free-forever` | a combo → each entry in order, as fallback tiers |
| `gpt-4o` | the configured `default_connection` |

```bash
curl http://127.0.0.1:7878/v1/chat/completions \
  -H 'content-type: application/json' \
  -d '{"model":"glm/glm-4.6","messages":[{"role":"user","content":"hi"}]}'
```

Responses report which tier answered through the `x-router-model`,
`x-router-provider`, `x-router-attempt` and `x-router-source` headers.

## Configuration

`~/.alnair-router/config.toml` holds the settings, and any value can be
overridden per run:

```bash
ALNAIR_ROUTER__SERVER__PORT=9000 alnair-router
```

`ALNAIR_ROUTER_HOME` moves the whole home directory. Every key, with comments,
is in
[`router.example.toml`](https://github.com/xFlawlessDev/alnair-router/blob/main/crates/alnair-router/router.example.toml).

## Documentation

- [Main README](https://github.com/xFlawlessDev/alnair-router#readme) — features, endpoints and configuration
- [Handover notes](https://github.com/xFlawlessDev/alnair-router/blob/main/docs/HANDOVER.md) — architecture and behaviours
- [Docker image](https://github.com/xFlawlessDev/alnair-router/pkgs/container/alnair-router) — `ghcr.io/xflawlessdev/alnair-router`
- [Releases](https://github.com/xFlawlessDev/alnair-router/releases) — binaries for every target

## License

MIT
