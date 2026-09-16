# alnair-router-web

Admin dashboard for [`alnair-router`](../../crates/alnair-router). Manage the
router's connections, aliases, combos, and client API keys, and inspect usage —
all against the router's admin JSON API.

In production the built assets are embedded into the router binary
(`rust-embed`), so the dashboard is served from the same origin at `/` —
`pnpm run build` and the next `cargo build` pick it up. `server.serve_dashboard
= false` disables that when a reverse proxy owns the root path.

Built with Vue 3, TypeScript, Vite, Tailwind CSS v4, and shadcn-vue primitives,
scaffolded from the [EvoFast `vue-tailwind-vite` template](https://github.com/evofast) (MIT).

## Quick start

Start the router first (from the repository root):

```bash
cargo run -p alnair-router
```

Then this app (from `apps/web`):

```bash
pnpm install
pnpm dev
```

Vite serves on `http://localhost:5173` and proxies `/api` and `/v1` to
`http://127.0.0.1:7878`. Point it elsewhere with `ALNAIR_ROUTER_URL`:

```bash
ALNAIR_ROUTER_URL=http://192.168.1.10:7878 pnpm dev
```

## What it does

| Page            | Purpose                                                                                                                                                                                                                                                                                                  |
| --------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Overview**    | Health, connection totals, client-auth posture, and usage rollup.                                                                                                                                                                                                                                        |
| **Connections** | Upstream endpoints (`openai-compatible`, `anthropic-native`, `command-code`) with keys and custom headers; search, provider-type and status filters, list or card-grid views, flat or grouped by provider/type, provider-brand glyphs, and a per-row connectivity test against the upstream's `/models`. |
| **Aliases**     | Prefix → connection mappings; bulk-import selected upstream models as aliases, run a real chat completion from a row, and test resolution + model override.                                                                                                                                              |
| **Combos**      | Ordered fallback chains, reorderable tier by tier.                                                                                                                                                                                                                                                       |
| **API Keys**    | Mint router-issued client keys; reveal a stored key or rotate it for a new secret.                                                                                                                                                                                                                       |
| **Usage**       | Per-attempt records that auto-refresh (5s) alongside the summary aggregates, plus a live provider topology (Vue Flow, with zoom/fit controls): connection nodes around the router hub, animated edge on each route currently handling a request.                                                         |
| **Console**     | Live API transfer log (requests, attempts, auth/limit rejections) with level filters, follow, and pause.                                                                                                                                                                                                 |

Admin routes are unauthenticated on loopback. If the router binds a
non-loopback host with `server.admin_token`, configure the same token via the
key icon in the header.

## Checks

```bash
pnpm run check   # vue-tsc --noEmit && vite build
pnpm test        # Vitest
pnpm run build   # production bundle in dist/
```

## Structure

```
src/
├── components/
│   ├── layout/          # app shell, header, theme toggle
│   ├── ui/              # shadcn-vue primitives
│   └── ...              # PageHeader, StatusBadge, ConfirmDialog, feature dialogs
├── lib/                 # api client, admin token, formatters
├── pages/               # route-level views
├── router/              # Vue Router configuration
├── stores/              # Pinia (theme)
└── types/               # API wire types
```
