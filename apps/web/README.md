# alnair-router-web

Admin dashboard for [`alnair-router`](../../crates/alnair-router). Manage the
router's connections, aliases, combos, and client API keys, and inspect usage —
all against the router's admin JSON API.

Built with Vue 3, TypeScript, Vite, Tailwind CSS v4, and shadcn-vue primitives,
scaffolded from the [EvoFast `vue-tailwind-vite` template](https://github.com/evofast) (MIT).

## Quick start

Start the router first (from the repository root):

```bash
cargo run -p alnair-router
```

Then this app (from `apps/web`):

```bash
npm ci
npm run dev
```

Vite serves on `http://localhost:5173` and proxies `/api` and `/v1` to
`http://127.0.0.1:7878`. Point it elsewhere with `ALNAIR_ROUTER_URL`:

```bash
ALNAIR_ROUTER_URL=http://192.168.1.10:7878 npm run dev
```

## What it does

| Page | Purpose |
|---|---|
| **Overview** | Health, connection totals, client-auth posture, and usage rollup. |
| **Connections** | Upstream endpoints (`openai-compatible`, `anthropic-native`) with keys and custom headers. |
| **Aliases** | Prefix → connection mappings used by `prefix/model` references. |
| **Combos** | Ordered fallback chains, reorderable tier by tier. |
| **API Keys** | Mint router-issued client keys; the secret is shown once. |
| **Usage** | Per-attempt records with filters and pagination, plus summary aggregates. |

Admin routes are unauthenticated on loopback. If the router binds a
non-loopback host with `server.admin_token`, configure the same token via the
key icon in the header.

## Checks

```bash
npm run check   # vue-tsc --noEmit && vite build
npm test        # Vitest
npm run build   # production bundle in dist/
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
