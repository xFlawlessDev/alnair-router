# Database-Specific Features

## PostgreSQL

PostgreSQL is the most feature-rich backend in sqlx.

### PgListener (LISTEN/NOTIFY)

`PgListener` provides async PostgreSQL LISTEN/NOTIFY functionality with automatic reconnection:

```rust
use sqlx::postgres::PgListener;
use futures::StreamExt;

// Connect and subscribe to a channel
let mut listener = PgListener::connect_with(&pool).await?;
listener.listen("user_events").await?;

// Blocking receive
let notification = listener.recv().await?;
println!("Channel: {}", notification.channel());
println!("Payload: {}", notification.payload());

// Stream-based (using futures::StreamExt)
while let Some(notification) = listener.next().await {
    let notification = notification?;
    println!("Received '{}' on '{}': {}",
        notification.payload(), notification.channel(), notification.payload());
}
```

**Sending notifications:**

```rust
sqlx::query("NOTIFY user_events, 'user_created:42'")
    .execute(&pool)
    .await?;
```

**Key behavior**: `PgListener` automatically reconnects if the connection dies. You do not need to handle reconnection manually.

### PostgreSQL Arrays

`Vec<T>` maps directly to PostgreSQL array types:

```rust
// Reading arrays
let tags: Vec<String> = sqlx::query_scalar!(
    "SELECT array_agg(tag) FROM posts"
).fetch_one(&pool).await?;

// Writing arrays
sqlx::query!("INSERT INTO posts (tags) VALUES ($1)", &vec!["rust", "sqlx"] as &[&str])
    .execute(&pool)
    .await?;

// Array of a custom type (requires #[sqlx(type_name = "...")])
let roles: Vec<UserRole> = sqlx::query_scalar!(
    "SELECT array_agg(role) FROM users"
).fetch_one(&pool).await?;
```

### PostgreSQL COPY (Bulk Import)

For high-performance bulk data loading:

```rust
use sqlx::postgres::PgPoolCopyExt;

// Binary COPY
let mut writer = pool.copy_in_raw("COPY users FROM STDIN WITH (FORMAT binary)").await?;
// Write binary-encoded data with .send()
writer.finish().await?;

// CSV COPY
let mut writer = pool.copy_in_raw("COPY users (id, name, email) FROM STDIN WITH (FORMAT csv)").await?;
writer.send(b"1,Alice,alice@example.com\n" as &[u8]).await?;
writer.send(b"2,Bob,bob@example.com\n" as &[u8]).await?;
let rows = writer.finish().await?;
println!("Imported {} rows", rows);
```

### PostgreSQL Ranges

Range types (int4range, int8range, numrange, tsrange, tstzrange, daterange) are supported via `sqlx::postgres::types::PgRange`:

```rust
use sqlx::postgres::types::PgRange;

// Reading a range
let range: PgRange<chrono::NaiveDateTime> = sqlx::query_scalar!(
    "SELECT valid_during FROM events WHERE id = $1",
    event_id
).fetch_one(&pool).await?;
```

### PostgreSQL Composite Types

Custom composite types are supported with `#[sqlx(type_name = "...")]`:

```rust
#[derive(Debug, sqlx::Type)]
#[sqlx(type_name = "address")]
struct Address {
    street: String,
    city: String,
    zip: String,
}

#[derive(Debug, FromRow)]
struct User {
    id: i64,
    name: String,
    address: Address,
}
```

### PostgreSQL SSL Modes

```rust
use sqlx::postgres::PgSslMode;

// Available modes:
PgSslMode::Disable      // No SSL
PgSslMode::Allow        // Try plaintext first; if that fails, try SSL
PgSslMode::Prefer       // Try SSL first; if that fails, try plaintext (default)
PgSslMode::Require     // Require SSL, but don't verify certificate
PgSslMode::VerifyCa    // Require SSL + verify certificate authority
PgSslMode::VerifyFull  // Require SSL + verify certificate + hostname match
```

## MySQL

### MySQL-Specific Connection

```rust
use sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions, MySqlSslMode};

let pool = MySqlPoolOptions::new()
    .max_connections(10)
    .connect_with(
        MySqlConnectOptions::new()
            .host("localhost")
            .port(3306)
            .username("root")
            .password("password")
            .database("mydb")
            .ssl_mode(MySqlSslMode::Preferred)
            .charset("utf8mb4")  // Set character set
    )
    .await?;
```

### MySQL Placeholder Syntax

MySQL uses `?` instead of `$1`:

```rust
// Correct for MySQL
sqlx::query("SELECT * FROM users WHERE id = ? AND active = ?")
    .bind(1)
    .bind(true)
    .fetch_one(&pool)
    .await?;

// WRONG — $1 is PostgreSQL syntax
// sqlx::query("SELECT * FROM users WHERE id = $1")  // Will fail on MySQL
```

### MySQL INSERT and Auto-Increment

MySQL does not support `RETURNING *`. Use `LAST_INSERT_ID()` instead:

```rust
sqlx::query("INSERT INTO users (name, email) VALUES (?, ?)")
    .bind("Alice")
    .bind("alice@example.com")
    .execute(&pool)
    .await?;

let id: u64 = sqlx::query_scalar("SELECT LAST_INSERT_ID()")
    .fetch_one(&pool)
    .await?;
```

### MySQL Stored Procedures

```rust
let result = sqlx::query("CALL my_procedure(?, ?)")
    .bind(param1)
    .bind(param2)
    .execute(&pool)
    .await?;
```

## SQLite

### In-Memory Database

Ideal for testing and temporary data:

```rust
// In-memory (fastest, data lost when connection closes)
let pool = SqlitePool::connect("sqlite::memory:").await?;

// File-based
let pool = SqlitePool::connect("sqlite:./mydb.sqlite").await?;
```

### SQLite Connect Options

```rust
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use std::time::Duration;

let pool = SqlitePoolOptions::new()
    .max_connections(5)  // Keep low for SQLite
    .connect_with(
        SqliteConnectOptions::new()
            .filename("./mydb.sqlite")
            .journal_mode(SqliteJournalMode::Wal)    // WAL for better concurrency
            .busy_timeout(Duration::from_secs(5))     // Wait if DB is locked
            .create_if_missing(true)                   // Create file if missing
            .foreign_keys(true)                       // Enforce FK constraints
    )
    .await?;
```

### SQLite PRAGMA Settings

Set PRAGMA values through connect options or raw queries:

```rust
// Via connect options
let options = SqliteConnectOptions::new()
    .filename("./mydb.sqlite")
    .foreign_keys(true)
    .journal_mode(SqliteJournalMode::Wal)
    .synchronous(SqliteSynchronous::Normal)
    .pragma("cache_size", "-8000");  // 8MB cache

// Via raw query
sqlx::query("PRAGMA journal_mode = WAL")
    .execute(&pool)
    .await?;
```

### SQLite Journal Modes

| Mode       | Description                                                   |
| ---------- | ------------------------------------------------------------- |
| `Delete`   | Default. Deletes journal file on commit                       |
| `Truncate` | Truncates journal file to zero length                         |
| `Persist`  | Journal file is not deleted (prevents file deletion overhead) |
| `Memory`   | Journal kept in memory (fast but less safe)                   |
| `Wal`      | Write-Ahead Logging (best concurrency for readers/writers)    |

### SQLite Boolean Pitfall

SQLite has no native boolean type. Booleans are stored as 0 (false) and 1 (true):

```rust
// Inserting boolean (sqlx handles the conversion)
sqlx::query("INSERT INTO users (name, active) VALUES ($1, $2)")
    .bind("Alice")
    .bind(true)  // Stored as 1
    .execute(&pool)
    .await?;

// Querying boolean (sqlx handles the conversion back)
let active: bool = sqlx::query_scalar("SELECT active FROM users WHERE id = $1")
    .bind(1)
    .fetch_one(&pool)
    .await?;  // Reads 1 as true

// But in raw SQL, compare with integers:
sqlx::query("SELECT * FROM users WHERE active = 1")  // Not WHERE active = TRUE
```

### SQLite Feature Flags

| Flag               | Description                                     |
| ------------------ | ----------------------------------------------- |
| `sqlite`           | Bundled SQLite (default when sqlite is enabled) |
| `sqlite-unbundled` | Use system libsqlite3 instead of bundled        |
| `regexp`           | Enable REGEXP extension function                |

### SQLite No Native Array Type

SQLite has no native array type. Use JSON serialization as a workaround:

```rust
// Store array as JSON text
let tags = vec!["rust", "sqlx"];
sqlx::query("INSERT INTO posts (tags) VALUES ($1)")
    .bind(serde_json::to_string(&tags).unwrap())
    .execute(&pool)
    .await?;

// Read back
let tags_str: String = sqlx::query_scalar("SELECT tags FROM posts WHERE id = $1")
    .bind(1)
    .fetch_one(&pool)
    .await?;
let tags: Vec<String> = serde_json::from_str(&tags_str).unwrap();
```
