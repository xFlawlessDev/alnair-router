---
name: sqlx
description: "Comprehensive guide for building solutions with the sqlx Rust library. Use this skill whenever the user wants to: write SQL queries in Rust with compile-time checking; connect to PostgreSQL, MySQL, or SQLite databases using async Rust; set up connection pools with sqlx; run database migrations; use query macros (query!, query_as!, query_scalar!); map SQL rows to Rust structs with FromRow; handle transactions in Rust; use #[sqlx::test] for database testing; work with JSON columns in SQL; use PgListener for PostgreSQL LISTEN/NOTIFY; handle sqlx errors properly; configure sqlx feature flags in Cargo.toml; set up offline mode with cargo sqlx prepare; use QueryBuilder for dynamic queries; or implement the repository pattern with sqlx. Make sure to use this skill when the user mentions sqlx, Rust database, Rust PostgreSQL, Rust MySQL, Rust SQLite, Rust SQL toolkit, compile-time query checking, async database Rust, database migrations Rust, sqlx pool, sqlx macros, or any SQL database interaction in Rust."
---

# sqlx — Async SQL Toolkit for Rust

sqlx is an async-first, pure-Rust SQL toolkit with compile-time query checking. It supports PostgreSQL, MySQL, and SQLite without a DSL — you write raw SQL, and sqlx validates it against a live database at compile time.

## Crate Architecture

| Module                | Purpose                                                         |
| --------------------- | --------------------------------------------------------------- |
| `sqlx::postgres`      | PostgreSQL driver (`PgPool`, `PgConnection`, `PgListener`)      |
| `sqlx::mysql`         | MySQL driver (`MySqlPool`, `MySqlConnection`)                   |
| `sqlx::sqlite`        | SQLite driver (`SqlitePool`, `SqliteConnection`)                |
| `sqlx::any`           | Database-agnostic driver (`AnyPool`) — no compile-time checking |
| `sqlx::query_builder` | Runtime dynamic query construction (`QueryBuilder`)             |
| `sqlx::migrate`       | Migration framework (`migrate!` macro, `Migrator`)              |
| `sqlx::types`         | Type wrappers (`Json<T>`, `Text<T>`)                            |

## Quick Start: Minimal Dependency Setup

Always include at least one database driver and a runtime feature. Without a runtime feature, the pool will panic at runtime.

```toml
# Cargo.toml
[dependencies]
sqlx = { version = "0.8", features = ["runtime-tokio", "tls-rustls", "postgres", "chrono", "uuid"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
dotenvy = "0.15"
```

```bash
# .env
DATABASE_URL=postgres://postgres:password@localhost:5432/mydb
```

```rust
use sqlx::PgPool;

#[tokio::main]
async fn main() -> Result<(), sqlx::Error> {
    dotenvy::dotenv().ok();
    let pool = PgPool::connect(&std::env::var("DATABASE_URL").unwrap()).await?;
    let users = sqlx::query_as!(User, "SELECT id, name FROM users")
        .fetch_all(&pool).await?;
    Ok(())
}
```

**Critical**: `DATABASE_URL` must be set at compile time for `query!()` macros to work. Set it in `.env` or use `SQLX_OFFLINE=true` with prepared metadata (see Offline Mode section).

## Quick Reference: Which Guide Do I Need?

- **Compile-time query macros, type overrides, offline mode** -> Read `references/compile-time-checking.md`
- **Connection pools, PgConnectOptions, pool tuning** -> Read `references/connection-pooling.md`
- **Rust-to-SQL type mapping, FromRow, custom types** -> Read `references/type-mapping.md`
- **Transactions, nested transactions, savepoints** -> Read `references/transactions-migrations.md`
- **#[sqlx::test], fixtures, test patterns** -> Read `references/testing-mocking.md`
- **PostgreSQL specifics (PgListener, COPY, arrays, ranges)** -> Read `references/database-specifics.md`
- **Error handling, sqlx::Error enum** -> Read `references/error-handling.md`
- **Feature flags, Cargo.toml patterns** -> Read `references/feature-flags.md`

## Core Patterns at a Glance

### 1. Query with Compile-Time Checking (Preferred)

The `query!()` macro connects to the database at compile time, validates SQL syntax, checks that parameter types match, and verifies return column types.

```rust
// Returns an anonymous struct with typed fields (no FromRow needed)
let record = sqlx::query!("SELECT id, name, email FROM users WHERE id = $1", user_id)
    .fetch_one(&pool)
    .await?;
// record.id: i64, record.name: String, record.email: String

// query_as! maps to a named struct (needs #[derive(FromRow)])
let user: User = sqlx::query_as!(User, "SELECT id, name FROM users WHERE id = $1", user_id)
    .fetch_one(&pool)
    .await?;

// query_scalar! returns a single column value
let count: i64 = sqlx::query_scalar!("SELECT COUNT(*)::BIGINT FROM users")
    .fetch_one(&pool)
    .await?;
```

### 2. Runtime Query Functions (No Compile-Time DB Needed)

Use these when you cannot set `DATABASE_URL` at compile time. They are not type-checked.

```rust
// query() returns anonymous PgRow — access columns by name or index
// Note: `sqlx::Row` trait must be in scope for .get()
use sqlx::Row;

let row = sqlx::query("SELECT id, name FROM users WHERE id = $1")
    .bind(1)
    .fetch_one(&pool)
    .await?;
let id: i64 = row.get("id");
let name: String = row.get("name");
```

### 3. Fetch Strategies

Every query supports multiple fetch methods. Choose based on expected result count:

```rust
// .fetch_one()       — Exactly one row. Error if 0 rows (RowNotFound) or >1 rows
// .fetch_optional()  — 0 or 1 row. Returns Option<T>. None if no rows.
// .fetch_all()       — All rows as Vec<T>. Empty vec if no rows.
// .fetch(_)          — Returns a Stream (row by row, lazy). Use with futures::StreamExt.
```

### 4. Connection Pool Configuration

```rust
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;

let pool = PgPoolOptions::new()
    .max_connections(20)
    .min_connections(5)
    .acquire_timeout(Duration::from_secs(3))
    .idle_timeout(Duration::from_secs(600))
    .max_lifetime(Duration::from_secs(1800))
    .connect("postgres://user:pass@localhost/db")
    .await?;
```

### 5. Transactions

Transactions implement `Executor`, so you can pass them to any function that accepts a pool or connection.

```rust
use sqlx::Acquire;

// Manual transaction
let mut tx = pool.begin().await?;
sqlx::query!("INSERT INTO users (name) VALUES ($1)", "Alice")
    .execute(&mut *tx)
    .await?;
sqlx::query!("INSERT INTO audit_log (action) VALUES ($1)", "user created")
    .execute(&mut *tx)
    .await?;
tx.commit().await?;
// Dropping tx without commit() triggers automatic rollback.

// Closure-based (auto-commit on Ok, auto-rollback on Err)
// Requires acquiring a connection first — .transaction() is on Connection, not Pool.
let mut conn = pool.acquire().await?;
let result = conn.transaction::<_, _, sqlx::Error>(|tx| {
    Box::pin(async move {
        sqlx::query!("INSERT INTO users (name) VALUES ($1)", "Bob")
            .execute(&mut **tx).await?;
        Ok(())
    })
}).await?;

// Nested transactions (SAVEPOINTs)
let mut tx = pool.begin().await?;
let mut nested = tx.begin().await?; // Creates a SAVEPOINT — requires `Acquire`
nested.rollback().await?;           // Only rolls back to savepoint
tx.commit().await?;                 // Outer transaction unaffected
```

### 6. Generic Functions over Executor

Write functions that work with pools, connections, and transactions alike using the `Executor` trait:

```rust
use sqlx::{Executor, PgExecutor};

async fn find_user_by_id<'e, E>(executor: E, id: i64) -> sqlx::Result<User>
where
    E: PgExecutor<'e>,
{
    sqlx::query_as!(User, "SELECT * FROM users WHERE id = $1", id)
        .fetch_one(executor)
        .await
}

// Works with pool, connection, or transaction:
let user = find_user_by_id(&pool, 1).await?;
let user = find_user_by_id(&mut tx, 1).await?;
```

### 7. FromRow Derive

Map SQL rows to Rust structs with `#[derive(FromRow)]`. The struct field names must match SQL column names.

```rust
use sqlx::FromRow;

#[derive(Debug, FromRow)]
struct User {
    id: i64,
    name: String,
    email: String,
    created_at: chrono::DateTime<chrono::Utc>,
}

// Column renaming when names don't match
#[derive(Debug, FromRow)]
struct User {
    id: i64,
    #[sqlx(rename = "full_name")]
    name: String,
}

// Flatten for JOINs (combines fields from multiple tables)
#[derive(Debug, FromRow)]
struct UserWithPost {
    #[sqlx(flatten)]
    user: User,
    #[sqlx(flatten)]
    post: Post,
}

// Skip columns from the query that you don't need
#[derive(Debug, FromRow)]
struct UserSummary {
    id: i64,
    name: String,
    #[sqlx(skip)]
    _ignored_field: String,
}
```

### 8. JSON Columns

```rust
use sqlx::types::Json;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
struct User {
    id: i64,
    #[sqlx(json)]
    metadata: Json<Metadata>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Metadata {
    role: String,
    preferences: std::collections::HashMap<String, String>,
}

// Insert with JSON value
sqlx::query!(
    "INSERT INTO users (name, metadata) VALUES ($1, $2)",
    "Alice",
    Json(Metadata { role: "admin".into(), preferences: Default::default() })
).execute(&pool).await?;

// Read — Json<T> automatically deserializes
let user: User = sqlx::query_as(
    "SELECT id, name, metadata FROM users WHERE id = $1"
).bind(1).fetch_one(&pool).await?;
println!("Role: {}", user.metadata.0.role);
```

### 9. QueryBuilder for Dynamic Queries

When SQL structure depends on runtime conditions, use `QueryBuilder` instead of string concatenation.

```rust
use sqlx::QueryBuilder;

let mut qb = QueryBuilder::new("SELECT * FROM users WHERE ");

// Conditionally add clauses
if let Some(name) = &filter.name {
    qb.push("name LIKE ").push_bind(format!("%{}%", name));
}

if filter.active_only {
    if filter.name.is_some() {
        qb.push(" AND ");
    }
    qb.push("active = ").push_bind(true);
}

let users: Vec<User> = qb.build_query_as::<User>()
    .fetch_all(&pool)
    .await?;
```

### 10. Migrations

```bash
# Create a new migration
sqlx migrate add create_users

# Run all pending migrations
sqlx migrate run

# Rollback the last migration
sqlx migrate revert

# Show migration status
sqlx migrate info
```

Embed and run migrations in code:

```rust
let migrator = sqlx::migrate!(); // Reads from ./migrations by default
migrator.run(&pool).await?;
```

### 11. #[sqlx::test] Attribute

Automatically creates an isolated test database, runs migrations, and cleans up afterward.

```rust
#[sqlx::test(migrations = "./migrations")]
async fn test_create_user(pool: PgPool) -> sqlx::Result<()> {
    sqlx::query!("INSERT INTO users (name) VALUES ($1)", "Alice")
        .execute(&pool).await?;

    let user = sqlx::query_as!(User, "SELECT * FROM users WHERE name = $1", "Alice")
        .fetch_one(&pool).await?;

    assert_eq!(user.name, "Alice");
    Ok(())
}
```

## Placeholder Syntax by Database

This is a frequent source of errors. Different databases use different placeholders:

| Database   | Placeholder         | Example                       |
| ---------- | ------------------- | ----------------------------- |
| PostgreSQL | `$1`, `$2`, `$3`... | `WHERE id = $1 AND name = $2` |
| MySQL      | `?`                 | `WHERE id = ? AND name = ?`   |
| SQLite     | `?`                 | `WHERE id = ? AND name = ?`   |

Compile-time macros handle this automatically based on `DATABASE_URL`. Runtime queries must use the correct syntax for the target database.

## Key Imports Reference

```rust
use sqlx::{
    // Query functions (runtime, no compile-time checking)
    query, query_as, query_scalar,
    // Traits
    Executor, FromRow, Decode, Encode, Type,
    // Pool types
    PgPool, MySqlPool, SqlitePool, AnyPool, Pool,
    // Connection types
    PgConnectOptions, Transaction,
    // Error types
    Error, Result,
    // JSON support
    types::Json,
    // PostgreSQL extras
    postgres::{PgConnectOptions, PgListener, PgPoolOptions, PgSslMode},
    // MySQL extras
    mysql::{MySqlConnectOptions, MySqlPoolOptions},
    // SQLite extras
    sqlite::{SqliteConnectOptions, SqlitePoolOptions, SqliteJournalMode},
};
```

## Common Pitfalls

1. **Missing DATABASE_URL at compile time** — `query!()` macros connect to the DB during `cargo build`. Set `DATABASE_URL` in `.env` or use offline mode with `cargo sqlx prepare`.

2. **Missing runtime feature flag** — Always include `runtime-tokio` or `runtime-async-std` in features. Without it, pool creation will panic.

3. **Missing database feature flag** — Just adding `sqlx = "0.8"` without a DB feature only enables `any`, `json`, `macros`, `migrate`, and `derive`. Always add e.g. `features = ["postgres"]`.

4. **Wrong placeholder syntax** — PostgreSQL uses `$1`, MySQL/SQLite use `?`. Compile-time macros enforce this; runtime queries do not.

5. **Transaction dereference** — Execute queries on a transaction with `&mut *tx` (not `&tx`).

6. **SQLite boolean mapping** — Booleans are stored as INTEGER (0/1). Use integers in SQL comparisons.

7. **Slow compile times with many queries** — Use offline mode, or consider `query_unchecked!()` for non-critical queries.

8. **RETURNING clause** — PostgreSQL supports `RETURNING *` in INSERT/UPDATE. MySQL does not (use `LAST_INSERT_ID()` instead).

## Reference Files

For detailed information on any topic, read the appropriate reference file:

- `references/compile-time-checking.md` — query!, query_as!, query_scalar!, type overrides, unchecked variants, offline mode, .sqlx/ directory, query files
- `references/connection-pooling.md` — Pool options, connect options, PgConnectOptions, SSL modes, after_connect hooks, pool lifecycle
- `references/type-mapping.md` — Full Rust-to-SQL type table, feature-flag types, FromRow attributes, custom Encode/Decode, transparent types, enum mapping
- `references/transactions-migrations.md` — Transactions (manual, closure, nested), reversible migrations, migrate! macro, migration CLI commands
- `references/testing-mocking.md` — #[sqlx::test] macro, fixtures, repository pattern for mockability, in-memory SQLite for tests
- `references/database-specifics.md` — PostgreSQL (PgListener, COPY, arrays, ranges, composite types), MySQL specifics, SQLite (in-memory, journal mode, PRAGMA, extensions)
- `references/error-handling.md` — sqlx::Error enum, database error codes, RowNotFound, decode errors, pool errors
- `references/feature-flags.md` — Complete feature flag table, Cargo.toml patterns per use case, TLS options, runtime options
