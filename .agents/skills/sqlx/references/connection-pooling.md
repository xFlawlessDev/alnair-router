# Connection Pooling

sqlx provides built-in connection pooling powered by `deadpool`. Pools manage a set of reusable connections, handle reconnection, and allow tuning for performance.

## Pool Types

| Type         | Database          | Import               |
| ------------ | ----------------- | -------------------- |
| `PgPool`     | PostgreSQL        | `sqlx::PgPool`       |
| `MySqlPool`  | MySQL             | `sqlx::MySqlPool`    |
| `SqlitePool` | SQLite            | `sqlx::SqlitePool`   |
| `AnyPool`    | Database-agnostic | `sqlx::any::AnyPool` |

## Simple Connection

```rust
// Simple — uses default pool options
let pool = PgPool::connect("postgres://user:pass@localhost/db").await?;

// From environment variable
let pool = PgPool::connect(&std::env::var("DATABASE_URL")?).await?;
```

## Pool Configuration

```rust
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;

let pool = PgPoolOptions::new()
    .max_connections(20)           // Maximum concurrent connections (default: 10)
    .min_connections(5)            // Minimum idle connections to maintain (default: 0)
    .acquire_timeout(Duration::from_secs(3))  // Max wait to get a connection (default: 30s)
    .idle_timeout(Duration::from_secs(600))   // Close idle connections after this (default: 10min)
    .max_lifetime(Duration::from_secs(1800))  // Max total lifetime per connection (default: 30min)
    .connect("postgres://user:pass@localhost/db")
    .await?;
```

### Pool Options Reference

| Option                | Type                                   | Default | Description                                                   |
| --------------------- | -------------------------------------- | ------- | ------------------------------------------------------------- |
| `max_connections`     | `u32`                                  | 10      | Maximum connections in the pool                               |
| `min_connections`     | `u32`                                  | 0       | Minimum idle connections maintained                           |
| `acquire_timeout`     | `Duration`                             | 30s     | How long to wait for an available connection                  |
| `idle_timeout`        | `Duration`                             | 10min   | Close connections idle longer than this                       |
| `max_lifetime`        | `Duration`                             | 30min   | Recycle connections after this lifetime                       |
| `after_connect`       | `Fn(&mut Conn)`                        | None    | Callback after creating each new connection                   |
| `before_acquire`      | `Fn(&mut Conn) -> bool`                | None    | Callback before reusing a connection (return false to reject) |
| `after_release`       | `Fn(&mut Conn, &mut Option<Duration>)` | None    | Callback after releasing a connection back                    |
| `test_before_acquire` | `bool`                                 | true    | Ping connection before reusing (detects stale connections)    |
| `connect`             | `&str`                                 | —       | Connection string (calls `connect_with` internally)           |
| `connect_with`        | `ConnectOpts`                          | —       | Connect with custom options                                   |

### After Connect Hook

Use `after_connect` to run setup commands on every new connection (e.g., setting search_path, timezone, or session variables):

```rust
let pool = PgPoolOptions::new()
    .after_connect(|conn, _meta| Box::pin(async move {
        // Run SET commands on each new connection
        sqlx::query("SET search_path TO 'my_schema'")
            .execute(&mut *conn)
            .await?;
        sqlx::query("SET timezone = 'UTC'")
            .execute(&mut *conn)
            .await?;
        Ok(())
    }))
    .connect("postgres://user:pass@localhost/db")
    .await?;
```

## Connect Options (Per-Connection)

For fine-grained connection configuration, use the database-specific `ConnectOptions`:

### PostgreSQL Connect Options

```rust
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use sqlx::ConnectOptions;

let options = PgConnectOptions::new()
    .host("localhost")
    .port(5432)
    .username("user")
    .password("password")
    .database("mydb")
    .ssl_mode(PgSslMode::Prefer)          // Prefer, Require, Disable, NoTls
    .statement_cache_capacity(100)        // Prepared statement cache size
    .application_name("my-app")           // Shows in pg_stat_activity
    .options([("search_path", "my_schema,public")]);

let pool = PgPoolOptions::new()
    .connect_with(options)
    .await?;
```

### SQLite Connect Options

```rust
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use std::time::Duration;

let options = SqliteConnectOptions::new()
    .filename("./mydb.sqlite")
    .journal_mode(SqliteJournalMode::Wal)   // WAL mode for better concurrency
    .busy_timeout(Duration::from_secs(5))   // Wait up to 5s if DB is locked
    .create_if_missing(true)                 // Create the file if it doesn't exist
    .foreign_keys(true);                     // Enable foreign key enforcement
    // .pragma("synchronous", "normal")     // Set any PRAGMA value

let pool = SqlitePoolOptions::new()
    .connect_with(options)
    .await?;
```

### MySQL Connect Options

```rust
use sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions, MySqlSslMode};

let options = MySqlConnectOptions::new()
    .host("localhost")
    .port(3306)
    .username("user")
    .password("password")
    .database("mydb")
    .ssl_mode(MySqlSslMode::Preferred);

let pool = MySqlPoolOptions::new()
    .connect_with(options)
    .await?;
```

## Acquiring Connections

```rust
// Get a connection from the pool (returns PoolConnection)
let mut conn = pool.acquire().await?;

// Begin a transaction (returns Transaction, which impls Executor)
let mut tx = pool.begin().await?;

// The pool itself implements Executor (most common usage)
sqlx::query("SELECT 1").fetch_one(&pool).await?;
```

## Pool Lifecycle

- **Startup**: `min_connections` connections are created immediately
- **On demand**: New connections are created up to `max_connections` when needed
- **Idle cleanup**: Connections idle longer than `idle_timeout` are closed (but pool retains at least `min_connections`)
- **Lifetime recycling**: Connections older than `max_lifetime` are closed and replaced
- **Health check**: If `test_before_acquire` is true (default), connections are pinged before reuse

## Pool Exhaustion

If all `max_connections` are in use and a new acquire request comes in, it waits up to `acquire_timeout`. If the timeout elapses, it returns `Error::PoolTimedOut`.

Common solutions:

- Increase `max_connections`
- Decrease query execution time
- Use transactions to batch multiple operations
- Use streaming (`fetch()`) instead of `fetch_all()` to avoid holding connections while processing large result sets

## Graceful Shutdown

```rust
// Close the pool gracefully (waits for all connections to be returned)
pool.close().await;
```

This is important in application shutdown to avoid leaving transactions in progress.
