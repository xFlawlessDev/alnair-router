# sqlx-skills

A comprehensive AI skill for the [sqlx](https://github.com/launchbadge/sqlx) Rust library — the async-first, pure-Rust SQL toolkit with compile-time query checking. Supports PostgreSQL, MySQL, and SQLite.

## Overview

This skill enables an LLM to generate correct, production-ready Rust code using sqlx. It covers every major feature of the library with practical examples, type references, and common-pitfall documentation so that the LLM developer can build reliable database solutions without trial-and-error errors.

The skill uses a **progressive disclosure** architecture: the main `SKILL.md` file provides a concise quick-reference with 11 essential patterns, while 8 deep-dive reference files cover each topic in full detail (type tables, error code catalogs, Cargo.toml recipes, and more).

## Installation

```bash
npx skills add melonask/sqlx-skills
```

## What This Skill Covers

| Feature Area              | Highlights                                                                                                                                                                                                                     |
| ------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **Compile-Time Checking** | `query!`, `query_as!`, `query_scalar!`, type overrides, `query_unchecked!` variants, offline mode (`cargo sqlx prepare`), `.sqlx/` directory, query files                                                                      |
| **Connection Pooling**    | `PgPool` / `MySqlPool` / `SqlitePool`, pool options (max/min connections, timeouts, lifetimes), connect options, SSL modes, `after_connect` hooks, pool lifecycle                                                              |
| **Type Mapping**          | 40+ Rust-to-SQL type mappings across PostgreSQL, MySQL, and SQLite; `FromRow` derive attributes (`rename`, `flatten`, `skip`, `json`); custom `Encode`/`Decode`; transparent types; enum mapping; composite types              |
| **Transactions**          | Manual transactions, closure-based (auto-commit/rollback), nested transactions (SAVEPOINTs), generic functions over `Executor`                                                                                                 |
| **Migrations**            | CLI commands (`add`, `run`, `revert`, `info`), `migrate!()` macro, reversible migrations (`-- down` separator), `sqlx.toml` configuration                                                                                      |
| **Testing**               | `#[sqlx::test]` attribute macro, fixture files, test database lifecycle, repository pattern for mockability, in-memory SQLite testing                                                                                          |
| **Database-Specifics**    | PostgreSQL (PgListener / LISTEN/NOTIFY, COPY bulk import, arrays, ranges, composite types, SSL modes), MySQL (placeholder syntax, `LAST_INSERT_ID`), SQLite (in-memory, WAL journal mode, PRAGMA, boolean pitfall, extensions) |
| **Error Handling**        | Full `sqlx::Error` enum reference, PostgreSQL SQLSTATE error codes (23505 unique violation, 23503 FK violation, etc.), Axum integration patterns, custom error conversion                                                      |
| **Feature Flags**         | Complete 39-flag reference table, Cargo.toml patterns for every use case (PostgreSQL, MySQL, SQLite, minimal, all-databases), TLS options, runtime options                                                                     |

## File Structure

```
sqlx/
├── SKILL.md                              # Main guide (410 lines)
└── references/
    ├── compile-time-checking.md          # Query macros, type overrides, offline mode (225 lines)
    ├── connection-pooling.md             # Pool config, connect options, lifecycle (178 lines)
    ├── type-mapping.md                   # Full type table, FromRow, custom types (230 lines)
    ├── transactions-migrations.md        # Transactions, migrations, CLI commands (246 lines)
    ├── testing-mocking.md                # #[sqlx::test], fixtures, repository pattern (271 lines)
    ├── database-specifics.md             # PostgreSQL/MySQL/SQLite specifics (303 lines)
    ├── error-handling.md                 # sqlx::Error, error codes, patterns (228 lines)
    └── feature-flags.md                  # Complete flag reference, Cargo.toml recipes (180 lines)
```

**Total: 2,271 lines** of documentation.

## How It Works

1. **Triggering** — The skill description is designed to activate whenever the user mentions sqlx, Rust database operations, compile-time query checking, async database Rust, or any SQL database interaction in Rust.
2. **Core guide** — `SKILL.md` loads first with a quick-start setup, 11 essential patterns, placeholder syntax reference, key imports, and common pitfalls.
3. **Deep dives** — The LLM reads reference files on demand based on the specific task. For example, a question about error handling triggers loading `references/error-handling.md`.

## Trigger Phrases

This skill activates on mentions of: `sqlx`, `Rust database`, `Rust PostgreSQL`, `Rust MySQL`, `Rust SQLite`, `Rust SQL toolkit`, `compile-time query checking`, `async database Rust`, `database migrations Rust`, `sqlx pool`, `sqlx macros`, `query!`, `query_as!`, `FromRow`, `PgPool`, `#[sqlx::test]`, `PgListener`, `cargo sqlx prepare`, `SQLX_OFFLINE`, or any SQL database interaction in Rust.

## Requirements

- sqlx version 0.8.x (the skill references 0.8 APIs and feature flags)
- Rust edition 2021+
- An async runtime: Tokio or async-std
- For compile-time checking: a running database or prepared `.sqlx/` metadata

## License

This skill is provided as-is for use with LLM-powered development environments.
