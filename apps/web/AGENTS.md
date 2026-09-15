# alnair-router-web — Agent Guide

Admin dashboard for the `alnair-router` crate. Vue 3 + TypeScript + Vite + Tailwind CSS v4 + shadcn-vue primitives, scaffolded from the EvoFast `vue-tailwind-vite` template (MIT). It is a static, client-side app: no backend code, no secrets, no committed `.env` files.

## Commands

Run from `apps/web` (pnpm only; the lockfile is `pnpm-lock.yaml`):

- `pnpm install --frozen-lockfile` installs the locked dependencies.
- `pnpm dev` starts Vite on `:5173`, proxying `/api` and `/v1` to `ALNAIR_ROUTER_URL` (default `http://127.0.0.1:7878`).
- `pnpm run check` runs `vue-tsc --noEmit` and a production build.
- `pnpm test` runs the Vitest suite.
- `pnpm run format` formats with Prettier.

Note: `pnpm-workspace.yaml` here is pnpm's settings file (single package, no `packages:` list); its `allowBuilds` entries approve the two dependencies with install scripts (`vue-demi`, `maplibre-gl`). Without them `pnpm install` exits non-zero.

## Structure

- `src/lib/api.ts` — typed client for the router's admin API; `ApiError` surfaces the server's error message.
- `src/lib/adminToken.ts` — optional bearer token for `/api/*`, persisted in localStorage.
- `src/types/api.ts` — wire types mirroring the Rust repositories (snake_case fields).
- `src/pages/` — route-level views (Overview, Connections, Aliases, Combos, Keys, Usage, About).
- `src/components/` — app components; `src/components/ui/` is the untouched shadcn-vue primitive set — preserve its API.
- `src/router/` — route table; page titles live in route meta.

## Rules

- Composition API with typed `<script setup lang="ts">` only.
- Mutations go through `src/lib/api.ts`; do not call `fetch` from components.
- Use existing UI primitives and Tailwind tokens before adding components or dependencies; any new dependency must update `pnpm-lock.yaml` (`pnpm install` after editing `package.json`).
- Use `vue-sonner` toasts for mutation feedback, `ConfirmDialog` for destructive actions.
- Keep accessibility: labels, aria-labels on icon buttons, focus states.
- Tests live next to the module they cover (`*.test.ts`); run `pnpm test` before finishing.
