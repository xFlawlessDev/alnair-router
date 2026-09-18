# AGENTS.md

## Repo map

- Cargo workspace root owns `Cargo.lock` and profiles. Members: `crates/alnair-router` (router binary + lib) and `crates/alnair-llm` (provider stack, `publish = false`).
- `apps/web` is the Vue 3 dashboard with its own guide (`apps/web/AGENTS.md`); its built `dist/` is embedded into the router binary.
- `npm/` holds the published npm packages: the `@xflawlessdev/alnair-router` launcher plus one binary package per release target. Platform packages get their `bin/` injected by CI, never committed.
- `Dockerfile` (source build recipe, used by CI) and `docker-compose.yml` (pulls the GHCR image).
- Design docs: `docs/HANDOVER.md` (architecture, behaviours) and `docs/ROADMAP.md` (status).

## Commands

- `cargo fmt --all` then `cargo clippy --workspace --all-targets -- -D warnings` — CI fails on either; run both before finishing.
- `cargo test --workspace` (~203 tests, ~35s). `cargo test -p alnair-llm` is the slow half (~23s); for fast loops use `cargo test -p alnair-router --lib` or `--test routes`.
- `cargo run -p alnair-router` generates `$ALNAIR_ROUTER_HOME/secrets.key` on first run and prints a dashboard setup code; `ALNAIR_ROUTER__SECRETS__KEY` (64 hex or base64) overrides the generated key. Every config value overrides as `ALNAIR_ROUTER__SECTION__KEY`. From a terminal it detaches into the background and returns (`--foreground`, or `ALNAIR_ROUTER_FOREGROUND=1`, serves inline instead); a container running it as PID 1 never detaches.
- Real-provider e2e (opt-in, `#[ignore]`d): `ALNAIR_ROUTER_E2E_OPENAI_API_KEY=... cargo test -p alnair-router --test e2e_real -- --ignored`.
- Web: in `apps/web` run `pnpm test` and `pnpm run check` (vue-tsc + build). Run `pnpm run build` before a release build so the embedded dashboard is current. Dependency changes must update `pnpm-lock.yaml`. (pnpm settings live in `apps/web/pnpm-workspace.yaml`.)
- Release (root `npm install` once): `npm run release:dry` then `npm run release` — standard-version bumps, and the `postbump` hook syncs `crates/*/Cargo.toml`, `apps/web/package.json`, `npm/*/package.json`, and `Cargo.lock`. Pushing the `v*` tag triggers the release workflow (binaries, GHCR image, npm packages; the npm job needs the `NPM_TOKEN` secret); `npm run build:binary` builds a local release binary.
- Install/auto-start: `alnair-router install|uninstall|status` (auto-launch crate); `install.sh` (curl | sh) and `install.ps1` (irm | iex) fetch/unpack the release binary first. `install` never overwrites an existing `config.toml`. `start|stop|restart` drive the background process through `router.pid` (PID + address) + `control.token` in `$ALNAIR_ROUTER_HOME`; `serve|start|restart` accept `--port <n>` as a one-run override, and `tests/cli_lifecycle.rs` covers that contract against the real binary.

## Hard rules

- **Provider seam:** only `crates/alnair-router/src/upstream/chat_backend.rs` may reference `alnair_llm`; the `vendored_llm_layer_is_imported_from_exactly_one_file` test fails otherwise. Provider work belongs in `crates/alnair-llm`.
- **Catalog cache:** admin writes to connections/aliases/combos must call `state.invalidate_catalog()`; routing reads the cached snapshot (`model/cache.rs`, TTL `router.catalog_ttl_ms`).
- **Credentials:** never write `connections.api_key` outside `ConnectionRepository` (AES-256-GCM) — no raw SQL bypass.
- **Config keys:** add to `config.rs`, `router.example.toml`, and the README config list together.
- Migrations are embedded via `sqlx::migrate!`; a new file in `migrations/` needs a rebuild. Queries are runtime SQL — no `DATABASE_URL` or `cargo sqlx prepare` step.
- `provider_type` is CHECK-constrained to `openai-compatible` | `anthropic-native`.
- Wire JSON is snake_case end to end; mirror shape changes in `apps/web/src/types/api.ts`.
- Debug builds read dashboard assets from the `apps/web/dist` folder resolved at compile time; release builds embed them. `build.rs` writes a placeholder when `dist/` is missing; `server.serve_dashboard = false` disables serving.

## Rust conventions (review-enforced)

- No file over 800 LOC — split into `file/mod.rs` + submodules.
- `lib.rs`/`main.rs` hold module declarations, re-exports, docs, and entrypoints only.
- Visibility ladder: private → `pub(super)` → `pub(crate)` → bare `pub` only for real public API.
- Split large `impl` blocks across files; inline `#[cfg(test)]` modules under 150 lines, otherwise a sibling `tests.rs`.
- Prefer scoped `thiserror` enums over one crate-wide error.
