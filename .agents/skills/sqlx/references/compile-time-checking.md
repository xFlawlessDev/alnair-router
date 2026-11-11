# Compile-Time Query Checking

Compile-time query checking is sqlx's flagship feature. Proc macros connect to a live database during `cargo build`, parse the SQL, describe the query with `PREPARE`, and verify that parameter types and return column types match your Rust code.

## How It Works

When `cargo build` runs, each `query!()` macro:

1. Parses the SQL string for syntax validity
2. Reads `DATABASE_URL` from the environment
3. Connects to the database and issues a `PREPARE` statement
4. Retrieves parameter type descriptions and return column type descriptions
5. Checks that the Rust types you bind match the SQL parameter types
6. Generates an anonymous struct with properly typed fields for the return columns

If anything is wrong — syntax error, wrong parameter type, non-existent table — you get a compile error, not a runtime error.

## Available Macros

| Macro                                       | Returns                            | Compile-Time Checked | Notes                                    |
| ------------------------------------------- | ---------------------------------- | -------------------- | ---------------------------------------- |
| `query!(sql, params...)`                    | Anonymous struct with typed fields | Yes                  | Use when you want auto-typed row fields  |
| `query_as!(Type, sql, params...)`           | `Type` (must impl `FromRow`)       | Yes                  | Maps to a named struct                   |
| `query_scalar!(sql, params...)`             | Single column value                | Yes                  | Returns one value, not a row             |
| `query_unchecked!(sql, params...)`          | Anonymous struct                   | SQL only, no types   | Validates syntax but skips type checking |
| `query_as_unchecked!(Type, sql, params...)` | `Type`                             | SQL only             | Syntax checked, type mapping unchecked   |
| `query_scalar_unchecked!(sql, params...)`   | Single value                       | SQL only             | Syntax checked, type mapping unchecked   |
| `query_file!(path)`                         | Anonymous struct                   | Yes                  | Reads SQL from a `.sql` file             |
| `query_file_as!(Type, path)`                | `Type`                             | Yes                  | Named struct from `.sql` file            |
| `query_file_scalar!(path)`                  | Single value                       | Yes                  | Scalar from `.sql` file                  |

## query!() — Anonymous Record Type

The `query!()` macro returns an anonymous struct where each selected column becomes a typed field:

```rust
let record = sqlx::query!(
    "SELECT id, name, email, created_at FROM users WHERE id = $1",
    42i64
)
.fetch_one(&pool)
.await?;

// Fields are automatically typed based on the database schema:
// record.id: i64
// record.name: String
// record.email: String
// record.created_at: chrono::NaiveDateTime  (or DateTime<Utc> for TIMESTAMPTZ)
println!("User #{}: {} ({})", record.id, record.name, record.email);
```

The column names in the generated struct match the SQL column names exactly (lowercase).

## query_as!() — Named Struct

Use when you need a reusable type, or when working with existing structs:

```rust
#[derive(Debug, sqlx::FromRow)]
struct User {
    id: i64,
    name: String,
    email: String,
    created_at: chrono::DateTime<chrono::Utc>,
}

let users: Vec<User> = sqlx::query_as!(
    User,
    "SELECT id, name, email, created_at FROM users WHERE active = $1",
    true
)
.fetch_all(&pool)
.await?;
```

The struct fields must match the SQL column names. Use `#[sqlx(rename = "...")]` when they differ.

## query_scalar!() — Single Value

Returns just one column value from one row. Ideal for COUNT, EXISTS, or looking up a single field:

```rust
let count: i64 = sqlx::query_scalar!("SELECT COUNT(*)::BIGINT FROM users")
    .fetch_one(&pool)
    .await?;

let name: Option<String> = sqlx::query_scalar!(
    "SELECT name FROM users WHERE id = $1",
    user_id
)
.fetch_optional(&pool)
.await?; // Returns Option<String>, None if no user found

let exists: bool = sqlx::query_scalar!(
    "SELECT EXISTS(SELECT 1 FROM users WHERE email = $1)",
    "alice@example.com"
)
.fetch_one(&pool)
.await?;
```

## Type Overrides in Macros

Sometimes the inferred type doesn't match what you need. Use the `as` syntax to override.

**Important**: The override syntax is different for MySQL and PostgreSQL/SQLite because it is embedded in the SQL string itself:

| Database          | Override syntax                                   |
| ----------------- | ------------------------------------------------- |
| PostgreSQL/SQLite | `"column_name!"` or `"column_name: Type"`         |
| MySQL             | `` `column_name!` `` or `` `column_name: Type` `` |

```rust
// Override parameter type
let record = sqlx::query!(
    "SELECT * FROM users WHERE id = $1",
    user_id as i64  // Override what type the macro expects for $1
)
.fetch_one(&pool)
.await?;
```

### PostgreSQL / SQLite — double-quoted identifiers

```rust
// Force NOT NULL (useful for expressions Postgres can't infer)
let record = sqlx::query!(r#"SELECT 1 as "id!""#)
    .fetch_one(&pool)
    .await?;

// Override return column type
let record = sqlx::query!(
    r#"SELECT id, created_at as "created_at: chrono::DateTime<chrono::Utc>" FROM users WHERE id = $1"#,
    1i64
)
.fetch_one(&pool)
.await?;
```

### MySQL — backtick-quoted identifiers

```rust
let record = sqlx::query!(
    "SELECT id, created_at as `created_at: chrono::DateTime<chrono::Utc>` FROM users WHERE id = ?",
    1i64
)
.fetch_one(&pool)
.await?;
```

Common cases where overrides are needed:

- `TIMESTAMP` columns can map to `NaiveDateTime` or `DateTime<Utc>` — override to pick
- `JSON`/`JSONB` columns can map to `serde_json::Value` or `Json<T>` — override for typed JSON
- `NUMERIC` columns can map to `f64`, `BigDecimal`, or `Decimal` — override based on precision needs
- Aggregate functions like `COUNT(*)` may be inferred as nullable depending on the database — use `!` to force non-null

## Unchecked Variants

Use `query_unchecked!()`, `query_as_unchecked!()`, or `query_scalar_unchecked!()` when you want SQL syntax validation but cannot connect to the database. These macros:

- Parse the SQL for syntax errors at compile time
- Do NOT connect to the database
- Do NOT check type compatibility
- Generate the same code but without type verification

```rust
// This will catch SQL syntax errors at compile time but won't check types
let users = sqlx::query_as_unchecked!(
    User,
    "SELECT id, name FROM users WHERE active = $1"
)
.bind(true)
.fetch_all(&pool)
.await?;
```

## Query Files

Store SQL in separate `.sql` files for better organization, especially for complex queries:

```rust
// queries/list_active_users.sql
// SELECT id, name, email FROM users WHERE active = true ORDER BY created_at DESC

// src/main.rs
let users = sqlx::query_file_as!(User, "queries/list_active_users.sql")
    .fetch_all(&pool)
    .await?;
```

The file path is relative to the crate root (where `Cargo.toml` is). For workspace members, it's relative to the member crate's root.

## Offline Mode

Offline mode lets you build without a live database. The workflow:

### Step 1: Prepare with a live database

```bash
# Ensure DATABASE_URL is set, then prepare
DATABASE_URL=postgres://user:pass@localhost/mydb cargo sqlx prepare

# For Cargo workspaces, use --workspace
DATABASE_URL=postgres://user:pass@localhost/mydb cargo sqlx prepare --workspace

# Verify the .sqlx/ directory is up-to-date (CI use)
cargo sqlx prepare --check
```

### Step 2: Commit .sqlx/ to version control

```
.sqlx/
├── query-a1b2c3d4.json
├── query-e5f6g7h8.json
└── ...
```

Each JSON file contains the query text, parameter type descriptions, and return column type descriptions. These are keyed by a hash of the query text so they survive code refactoring.

### Step 3: Build without a database

```bash
# Set SQLX_OFFLINE=true to use cached metadata
SQLX_OFFLINE=true cargo build
```

The macros will read from `.sqlx/` instead of connecting to the database. This is essential for CI/CD pipelines.

### Legacy: sqlx-data.json

Pre-0.7 used a single `sqlx-data.json` file. Sqlx 0.8+ uses the `.sqlx/` directory. Do not use the old format.

## DATABASE_URL Format

The `DATABASE_URL` environment variable must be a valid connection string for the target database:

```
# PostgreSQL
DATABASE_URL=postgres://user:pass@localhost:5432/mydb
DATABASE_URL=postgresql://user:pass@localhost:5432/mydb?sslmode=require

# MySQL
DATABASE_URL=mysql://user:pass@localhost:3306/mydb

# SQLite
DATABASE_URL=sqlite:./mydb.sqlite
DATABASE_URL=sqlite::memory:
```

The macros read this at compile time to connect and describe queries.
