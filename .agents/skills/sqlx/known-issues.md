# Known Issues Found During sqlx Skill Testing (sqlx v0.8.6, Rust 1.95, April 2026)

This document records verified bugs, misleading examples, or common pitfalls encountered while running real-world test code against the sqlx skill's `md` files. Every issue was confirmed with actual code execution before being added.

---

## Issue 1: MySQL `LAST_INSERT_ID()` returns `BIGINT UNSIGNED`, not `i64`

### Location in Skill
- `references/database-specifics.md` — MySQL INSERT and Auto-Increment section

### Original Skill Code (Buggy)
```rust
let id: i64 = sqlx::query_scalar("SELECT LAST_INSERT_ID()")
    .fetch_one(&pool)
    .await?;
```

### Actual Error
```
Error: ColumnDecode { index: "0", source: "mismatched types; Rust type `i64` (as SQL type `BIGINT`) is not compatible with SQL type `BIGINT UNSIGNED`" }
```

### Root Cause
MySQL's `LAST_INSERT_ID()` returns a `BIGINT UNSIGNED`. sqlx maps unsigned MySQL integers to `u64`, not `i64`.

### Verified Fix
```rust
let id: u64 = sqlx::query_scalar("SELECT LAST_INSERT_ID()")
    .fetch_one(&pool)
    .await?;
```

Additionally, `LAST_INSERT_ID()` is **connection-specific**. If using a pool, you must execute the INSERT and the `LAST_INSERT_ID()` query on the **same acquired connection**:

```rust
let mut conn = pool.acquire().await?;
sqlx::query("INSERT INTO users (name) VALUES (?)")
    .bind("Alice")
    .execute(&mut *conn)
    .await?;
let id: u64 = sqlx::query_scalar("SELECT LAST_INSERT_ID()")
    .fetch_one(&mut *conn)
    .await?;
```

---

## Issue 2: `#[derive(sqlx::Type, sqlx::Encode, sqlx::Decode)]` causes conflicting trait implementations

### Location in Skill
- `references/type-mapping.md` — Transparent Wrapper and Enum Mapping sections

### Original Skill Code (Buggy)
```rust
#[derive(Debug, sqlx::Type, sqlx::Encode, sqlx::Decode)]
#[sqlx(type_name = "user_role", rename_all = "lowercase")]
enum UserRole {
    Admin,
    User,
    Guest,
}
```

### Actual Error
```
error[E0119]: conflicting implementations of trait `sqlx::Encode<'_, _>` for type `UserRole`
```

### Root Cause
The `sqlx::Type` derive macro **already generates** `Encode` and `Decode` implementations (as well as `sqlx::Encode` / `sqlx::Decode` trait derivations). Explicitly adding `sqlx::Encode` and `sqlx::Decode` as separate derive macro arguments creates duplicate/conflicting impls.

### Verified Fix
Use **only** `#[derive(sqlx::Type)]` for enums and structs with `#[sqlx(transparent)]`:

```rust
#[derive(Debug, sqlx::Type)]
#[sqlx(type_name = "user_role", rename_all = "lowercase")]
enum UserRole { Admin, User, Guest }

#[derive(Debug, sqlx::Type)]
#[sqlx(transparent)]
struct Email(String);
```

---

## Issue 3: `cargo sqlx prepare` — relative paths in `migrate!()` macro are NOT supported with `paths relative to the current file's directory`

### Location in Skill
- `references/transactions-migrations.md` — Embedding Migrations in Code
- `references/transactions-migrations.md` — `#[sqlx::test]` With Migrations

### Original Skill Code (Buggy)
```rust
let migrator = sqlx::migrate!("migrations/");
```

### Actual Error
```
error: paths relative to the current file's directory are not currently supported
```

### Root Cause
The `migrate!()` macro requires the migration path to be relative to the **crate root** (the directory containing `Cargo.toml`), not the current source file's directory.

### Verified Fix
Pass the path relative to the crate root:

```rust
let migrator = sqlx::migrate!("./migrations");
```

Note: The trailing slash is optional, but a leading `./` is recommended for crate-relative resolution.

---

## Issue 4: `PgCopyIn::send()` requires `impl Deref<Target = [u8]>`

### Location in Skill
- `references/database-specifics.md` — PostgreSQL COPY (Bulk Import)

### Original Skill Code (Buggy)
```rust
let mut writer = pool.copy_in_raw("...").await?;
writer.send(b"1,Alice,alice@example.com\n").await?;
```

### Actual Error
```
error[E0271]: type mismatch resolving `<&[u8; 26] as Deref>::Target == [u8]`
```

### Root Cause
Byte string literals (`b"..."`) have type `&[u8; N]`, which does **not** implement `Deref<Target = [u8]>` directly in the context `send()` expects. You must cast to a slice.

### Verified Fix
```rust
writer.send(b"1,Alice,alice@example.com\n" as &[u8]).await?;
```

Alternatively, use a `Vec<u8>` or string converted to bytes.

---

## Issue 5: `query_scalar!("SELECT COUNT(*) ...")` may return `Option<T>` depending on database inference

### Location in Skill
- `references/compile-time-checking.md` — `query_scalar!` section
- `references/testing-mocking.md` — With Fixtures section

### Original Skill Code (Buggy)
```rust
let count: i64 = sqlx::query_scalar!("SELECT COUNT(*) FROM users")
    .fetch_one(&pool)
    .await?;
```

### Actual Error
```
error[E0308]: `?` operator has incompatible types
expected `i64`, found `Option<i64>`
```

### Root Cause
The compile-time macros infer the return type based on the database metadata. For aggregate functions like `COUNT(*)`, some PostgreSQL versions may infer the result as nullable (`Option<i64>`). This is especially common with PostgreSQL.

### Verified Fix
Either use `Option<i64>` as the type, or use a type override with `COUNT(*)::BIGINT`:

```rust
let count: i64 = sqlx::query_scalar!("SELECT COUNT(*)::BIGINT FROM users")
    .fetch_one(&pool)
    .await?;
```

Or accept `Option`:

```rust
let count: Option<i64> = sqlx::query_scalar!("SELECT COUNT(*) FROM users")
    .fetch_one(&pool)
    .await?;
```

---

## Issue 6: `#[sqlx::test(migrations = "...")]` — relative path from test file directory NOT supported

### Location in Skill
- `references/testing-mocking.md` — With Migrations section

### Original Skill Code (Buggy)
```rust
#[sqlx::test(migrations = "migrations/")]
async fn test_with_schema(pool: PgPool) -> sqlx::Result<()> { ... }
```

### Actual Error
```
error: paths relative to the current file's directory are not currently supported
```

### Root Cause
Same as Issue 3 — the `migrate!()` macro (internally used by `#[sqlx::test]`) requires paths relative to the crate root, not the test file's directory.

### Verified Fix
Use a crate-root-relative path, or (safer) don't specify `migrations` if your schema is already present in the test DB. If migrations are needed, place them at the crate root and use:

```rust
#[sqlx::test(migrations = "./migrations")]
async fn test_with_schema(pool: PgPool) -> sqlx::Result<()> { ... }
```

Alternatively, create tables inline in the test without relying on migrations for compile-time checked macros.

---

## Issue 7: `chrono` needs `serde` feature for `#[derive(Serialize, Deserialize)]` on structs containing `DateTime<Utc>`

### Location in Skill
- `references/compile-time-checking.md` — `query!()` examples with `chrono::DateTime<chrono::Utc>`

### Problem
The skill frequently shows structs with `chrono::DateTime<chrono::Utc>` and `#[derive(Serialize, Deserialize)]`, but does not mention that `chrono` must be configured with the `serde` feature:

```toml
chrono = { version = "0.4", features = ["serde"] }
```

### Actual Error (without `serde` feature)
```
error[E0277]: the trait bound `DateTime<Utc>: serde::Serialize` is not satisfied
error[E0277]: the trait bound `DateTime<Utc>: serde::Deserialize<'_>` is not satisfied
```

### Verified Fix
Add the `serde` feature to `chrono` in `Cargo.toml`:

```toml
chrono = { version = "0.4", features = ["serde"] }
```

---

## Issue 8: `uuid` requires `v4` feature for `Uuid::new_v4()`

### Location in Skill
- `references/database-specifics.md` — UUID Type section

### Problem
The skill shows `Uuid::new_v4()` but does not mention that the `uuid` crate must have the `v4` feature enabled.

### Actual Error (without `v4` feature)
```
error[E0599]: no function or associated item named `new_v4` found for struct `Uuid`
```

### Verified Fix
```toml
uuid = { version = "1", features = ["v4"] }
```

---

## Issue 9: `ipnet` type needs `ipnet` dependency in Cargo.toml, not just sqlx feature

### Location in Skill
- `references/database-specifics.md` — Network Types section
- `references/type-mapping.md` — Network Types

### Problem
The skill says `ipnet::IpNet` requires the `ipnet` feature on sqlx, but in practice the `ipnet` crate must also be added as an explicit dependency because the Rust code directly uses `ipnet::IpNet`.

### Actual Error
```
error[E0433]: cannot find module or crate `ipnet` in this scope
```

### Verified Fix
Add `ipnet` as a direct dependency **in addition to** enabling the sqlx feature:

```toml
[dependencies]
sqlx = { version = "0.8", features = ["ipnet", "postgres", ...] }
ipnet = "2"
```

---

## Summary Table

| # | Issue | Skill File | Severity |
|---|-------|------------|----------|
| 1 | MySQL `LAST_INSERT_ID()` type is `u64`, not `i64` | `database-specifics.md` | **High** |
| 2 | `sqlx::Type` already includes `Encode`/`Decode` | `type-mapping.md` | **High** |
| 3 | `migrate!("...")` paths must be crate-root-relative | `transactions-migrations.md` | **Medium** |
| 4 | `PgCopyIn::send()` needs `as &[u8]` cast | `database-specifics.md` | **Medium** |
| 5 | `COUNT(*)` may infer to `Option<i64>` in `query_scalar!` | `compile-time-checking.md` | **Medium** |
| 6 | `#[sqlx::test(migrations = ...)]` needs crate-relative path | `testing-mocking.md` | **Medium** |
| 7 | `chrono` needs `serde` feature for struct derives | `compile-time-checking.md` | **Low** |
| 8 | `uuid` needs `v4` feature for `new_v4()` | `database-specifics.md` | **Low** |
| 9 | `ipnet` needs explicit dependency in Cargo.toml | `type-mapping.md` | **Low** |
