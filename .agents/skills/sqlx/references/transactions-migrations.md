# Transactions and Migrations

## Transactions

Transactions group multiple queries into an atomic unit. If any query fails, all changes are rolled back. If all succeed, changes are committed.

### Manual Transactions

```rust
let mut tx = pool.begin().await?;

sqlx::query!("INSERT INTO users (name) VALUES ($1)", "Alice")
    .execute(&mut *tx)  // Note: &mut *tx, not &tx
    .await?;

sqlx::query!("INSERT INTO audit_log (action) VALUES ($1)", "user created")
    .execute(&mut *tx)
    .await?;

// Explicitly commit
tx.commit().await?;

// If you don't call commit(), the Transaction drops and auto-rollbacks.
// This is by design: you must explicitly commit for changes to persist.
```

### Closure-Based Transactions

The closure pattern auto-commits on `Ok(())` and auto-rollbacks on `Err`. The method is on `Connection`, not `Pool`, so you must acquire a connection first:

```rust
use sqlx::{Connection, PgPool};

let mut conn = pool.acquire().await?;
let result = conn.transaction::<_, _, sqlx::Error>(|tx| {
    Box::pin(async move {
        sqlx::query!("INSERT INTO users (name) VALUES ($1)", "Bob")
            .execute(&mut **tx)
            .await?;

        let user_id = sqlx::query_scalar!("SELECT lastval()")
            .fetch_one(&mut **tx)
            .await?;

        sqlx::query!("INSERT INTO profiles (user_id, bio) VALUES ($1, $2)")
            .bind(user_id)
            .bind("Hello world")
            .execute(&mut **tx)
            .await?;

        Ok(user_id)
    })
}).await?;
// result is i64 — the value returned from inside the closure
```

Note the double dereference `**tx`: the closure receives `&mut Transaction`, and you need to pass `&mut *<deref>` to `execute()`.

### Nested Transactions (SAVEPOINTs)

Nested transactions use database SAVEPOINTs. Rolling back a nested transaction only rolls back to the savepoint, not the outer transaction.
**Requires `sqlx::Acquire` to bring `begin()` into scope on a `Transaction`.**

```rust
use sqlx::Acquire;

let mut tx = pool.begin().await?;

sqlx::query!("INSERT INTO users (name) VALUES ($1)", "Charlie")
    .execute(&mut *tx)
    .await?;

// Start a nested transaction
let mut nested = tx.begin().await?;

sqlx::query!("INSERT INTO orders (user_id, total) VALUES ($1, $2)")
    .bind(1i64)
    .bind(99.99f64)
    .execute(&mut *nested)
    .await?;

// Something went wrong — rollback only the nested transaction
// The user "Charlie" still exists
nested.rollback().await?;

// Outer transaction commits normally
tx.commit().await?;
```

### Transactions with Generic Executor

Functions that accept an executor can be used with pools, connections, or transactions:

```rust
use sqlx::PgExecutor;

async fn transfer_funds<'e, E>(executor: E, from: i64, to: i64, amount: f64) -> sqlx::Result<()>
where
    E: PgExecutor<'e>,
{
    sqlx::query!("UPDATE accounts SET balance = balance - $1 WHERE id = $2", amount, from)
        .execute(executor)
        .await?;

    sqlx::query!("UPDATE accounts SET balance = balance + $1 WHERE id = $2", amount, to)
        .execute(executor)
        .await?;

    Ok(())
}

// Use with a pool (no transaction)
transfer_funds(&pool, 1, 2, 100.0).await?;

// Use within a transaction (atomic)
let mut conn = pool.acquire().await?;
conn.transaction(|tx| {
    Box::pin(async move {
        transfer_funds(&mut **tx, 1, 2, 100.0).await?;
        Ok(())
    })
}).await?;
```

## Migrations

### Migration File Structure

Migrations live in a `migrations/` directory (by default). Each migration is a SQL file with a timestamp prefix:

```
migrations/
├── 20240101000000_create_users.sql
├── 20240102000000_create_posts.sql
├── 20240103000000_add_email_index.sql
└── 20240104000000_create_comments.sql
```

### Creating Migrations

```bash
# Create a new migration file (simple, up only)
sqlx migrate add create_users
# Creates: migrations/20240101000000_create_users.sql

# Create a reversible migration (with down.sql)
sqlx migrate add create_users -r
# Creates: migrations/20240101000000_create_users.up.sql
#          migrations/20240101000000_create_users.down.sql
```

### Writing Migrations

**Simple (up-only):**

```sql
-- migrations/20240101000000_create_users.sql
CREATE TABLE users (
    id SERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    email TEXT UNIQUE NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_users_email ON users(email);
```

**Reversible (separator-based):**

```sql
-- migrations/20240101000000_create_users.sql

-- This is the "up" migration (applied when running forward)
CREATE TABLE users (
    id SERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    email TEXT UNIQUE NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- This is the "down" migration (applied when reverting)
-- down
DROP TABLE users;
```

The `-- down` separator splits the file into up and down sections.

### Running Migrations

```bash
# Apply all pending migrations
sqlx migrate run

# Rollback the last applied migration
sqlx migrate revert

# Show migration status (applied vs pending)
sqlx migrate info

# Create the database first (if it doesn't exist)
sqlx database create

# Drop and recreate the database (dangerous!)
sqlx database reset
```

### Embedding Migrations in Code

Embed migrations into the binary using the `migrate!()` macro:

```rust
// Reads from ./migrations by default
let migrator = sqlx::migrate!();

// Custom path (relative to the crate root where Cargo.toml is located)
let migrator = sqlx::migrate!("./migrations");

// Run all pending migrations
migrator.run(&pool).await?;
```

This is useful for applications that self-migrate on startup (e.g., CLI tools, embedded databases).

### Migrations in Tests

The `#[sqlx::test]` macro automatically applies migrations before each test:

```rust
#[sqlx::test(migrations = "./migrations")]
async fn test_user_creation(pool: PgPool) -> sqlx::Result<()> {
    // The database is fully migrated at this point
    sqlx::query!("INSERT INTO users (name) VALUES ($1)", "Test User")
        .execute(&pool).await?;
    Ok(())
}
```

### Migration Configuration (sqlx.toml)

For more complex setups (e.g., multiple migration sources), create a `sqlx.toml`:

```toml
# sqlx.toml
[migrate]
# Directory containing migration files (relative to sqlx.toml location)
migrations_dir = "./migrations"
```

### Migration Best Practices

1. **Always make migrations reversible** — Use the `-- down` separator or separate `.up.sql`/`.down.sql` files
2. **Never modify existing migrations** — Create new ones instead. Modified migrations won't match what's already been applied.
3. **Test migrations against a clean database** — `sqlx database reset` drops and recreates, then runs all migrations
4. **Order matters** — Migrations run in timestamp order. Ensure dependencies are respected.
5. **Use transactions where possible** — Wrapping DDL in transactions (PostgreSQL supports this) ensures atomicity
