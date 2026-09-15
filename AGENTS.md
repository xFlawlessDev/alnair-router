# AGENTS.md

## Repo map

- Cargo workspace root owns `Cargo.lock` and profiles. Members: `crates/alnair-router` (router binary + lib) and `crates/alnair-llm` (provider stack, `publish = false`).
- `apps/web` is the Vue 3 dashboard with its own guide (`apps/web/AGENTS.md`); its built `dist/` is embedded into the router binary.
- Design docs: `docs/HANDOVER.md` (architecture, behaviours) and `docs/ROADMAP.md` (status).

## Commands

- `cargo fmt --all` then `cargo clippy --workspace --all-targets -- -D warnings` — CI fails on either; run both before finishing.
- `cargo test --workspace` (~203 tests, ~35s). `cargo test -p alnair-llm` is the slow half (~23s); for fast loops use `cargo test -p alnair-router --lib` or `--test routes`.
- `cargo run -p alnair-router` **refuses to start** without `ALNAIR_ROUTER__SECRETS__KEY` (64 hex or base64). Every config value overrides as `ALNAIR_ROUTER__SECTION__KEY`.
- Real-provider e2e (opt-in, `#[ignore]`d): `ALNAIR_ROUTER_E2E_OPENAI_API_KEY=... cargo test -p alnair-router --test e2e_real -- --ignored`.
- Web: in `apps/web` run `pnpm test` and `pnpm run check` (vue-tsc + build). Run `pnpm run build` before a release build so the embedded dashboard is current. Dependency changes must update `pnpm-lock.yaml`. (pnpm settings live in `apps/web/pnpm-workspace.yaml`.)

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
