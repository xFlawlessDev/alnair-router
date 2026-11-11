# Feature Flags

sqlx uses Cargo feature flags to control which database backends, runtimes, TLS implementations, and extra type integrations are compiled.

## Default Features

```toml
sqlx = "0.8"
# Equivalent to:
# features = ["any", "json", "macros", "migrate", "derive"]
```

These defaults are always included unless you use `default-features = false`.

## Common Configurations

### PostgreSQL + Tokio + Rustls (Most Common)

```toml
sqlx = { version = "0.8", features = [
    "runtime-tokio",
    "tls-rustls",
    "postgres",
    "chrono",
    "uuid",
] }
```

### PostgreSQL + Tokio + All Extras

```toml
sqlx = { version = "0.8", features = [
    "runtime-tokio",
    "tls-rustls",
    "postgres",
    "chrono",
    "uuid",
    "bigdecimal",
    "json",
    "ipnet",
    "mac_address",
    "bit-vec",
] }
```

### MySQL + Tokio

```toml
sqlx = { version = "0.8", features = [
    "runtime-tokio",
    "tls-rustls",
    "mysql",
    "chrono",
] }
```

### SQLite + Tokio

```toml
sqlx = { version = "0.8", features = [
    "runtime-tokio",
    "sqlite",
] }
```

### Minimal (Smallest Binary)

```toml
sqlx = { version = "0.8", default-features = false, features = [
    "runtime-tokio",
    "tls-rustls",
    "postgres",
] }
```

### All Databases

```toml
sqlx = { version = "0.8", features = [
    "all-databases",
    "runtime-tokio",
    "tls-rustls",
] }
```

## Complete Feature Flag Reference

### Database Backends

| Feature            | Description                                      |
| ------------------ | ------------------------------------------------ |
| `postgres`         | PostgreSQL driver (pure Rust, no C dependencies) |
| `mysql`            | MySQL/MariaDB driver (pure Rust)                 |
| `sqlite`           | SQLite driver with bundled libsqlite3            |
| `sqlite-unbundled` | SQLite driver using system libsqlite3            |
| `all-databases`    | Enables postgres + mysql + sqlite                |

### Runtime

Exactly one runtime feature is required. Without a runtime, pool operations will panic.

| Feature             | Description                                  |
| ------------------- | -------------------------------------------- |
| `runtime-tokio`     | Use Tokio async runtime (most common choice) |
| `runtime-async-std` | Use async-std runtime                        |

**Legacy combo features** (included runtime + TLS, still work but split is preferred):

- `runtime-tokio-native-tls`
- `runtime-tokio-rustls`
- `runtime-async-std-native-tls`
- `runtime-async-std-rustls`

### TLS (SSL/TLS)

At most one TLS implementation should be enabled.

| Feature                        | Description                                                                  |
| ------------------------------ | ---------------------------------------------------------------------------- |
| `tls-native-tls`               | Native TLS (OpenSSL on Linux, SecureTransport on macOS, SChannel on Windows) |
| `tls-rustls`                   | Rustls (default crypto provider: ring)                                       |
| `tls-rustls-ring`              | Rustls with ring crypto provider (same as `tls-rustls`)                      |
| `tls-rustls-aws-lc-rs`         | Rustls with AWS LC RS crypto provider (FIPS-compatible)                      |
| `tls-rustls-ring-webpki`       | Rustls ring + WebPKI root certificates (from webpki-roots crate)             |
| `tls-rustls-ring-native-roots` | Rustls ring + native root certificates (from OS cert store)                  |
| `tls-none`                     | Explicitly disable all TLS support                                           |

### Extra Type Integrations

These features enable Encode/Decode support for additional Rust types:

| Feature        | Type                                                    | Database Support        |
| -------------- | ------------------------------------------------------- | ----------------------- |
| `chrono`       | `chrono::NaiveDateTime`, `chrono::DateTime<Utc>`, etc.  | All databases           |
| `time`         | `time::PrimitiveDateTime`, `time::OffsetDateTime`, etc. | All databases           |
| `uuid`         | `uuid::Uuid`                                            | All databases           |
| `bigdecimal`   | `bigdecimal::BigDecimal`                                | PostgreSQL, MySQL       |
| `rust_decimal` | `rust_decimal::Decimal`                                 | PostgreSQL, MySQL       |
| `json`         | `serde_json::Value`, `sqlx::types::Json<T>`             | All databases (default) |
| `ipnet`        | `ipnet::IpNet`                                          | PostgreSQL only         |
| `ipnetwork`    | `ipnetwork::IpNetwork`                                  | PostgreSQL only         |
| `mac_address`  | `mac_address::MacAddress`                               | PostgreSQL only         |
| `bit-vec`      | `bit_vec::BitVec`                                       | PostgreSQL only         |
| `bstr`         | `bstr::BString`                                         | PostgreSQL (BYTEA)      |

### Core Features (Defaults)

| Feature   | Description                                                     |
| --------- | --------------------------------------------------------------- |
| `any`     | Database-agnostic driver (`AnyPool`, `AnyConnection`)           |
| `json`    | `serde_json::Value` and `Json<T>` support                       |
| `macros`  | Compile-time checked query macros (`query!`, `query_as!`, etc.) |
| `migrate` | Migration framework (`migrate!` macro, `Migrator`)              |
| `derive`  | Derive macros (`FromRow`, `Type`, `Encode`, `Decode`)           |

### SQLite-Specific

| Feature                 | Description                                 |
| ----------------------- | ------------------------------------------- |
| `regexp`                | Enable REGEXP extension function for SQLite |
| `sqlite-preupdate-hook` | Pre-update hook for auditing (advanced)     |

## Choosing TLS

- **`tls-rustls`** — Recommended. Pure Rust, fast, no C dependencies. Use this unless you have a specific reason not to.
- **`tls-native-tls`** — Use if you need system certificate stores, or if rustls has compatibility issues with your database server.
- **`tls-none`** — Use only for local development or trusted internal networks.

## Choosing Runtime

- **`runtime-tokio`** — Most popular choice. If you're building a web server with Axum, Actix-web, or similar, use Tokio.
- **`runtime-async-std`** — Use if your project already uses async-std, or if you prefer its API.

## Build Time Optimization

Compile-time query checking (`query!()` macros) connects to the database during build, which can slow down compilation. Mitigations:

1. **Offline mode**: Use `cargo sqlx prepare` + `SQLX_OFFLINE=true`
2. **Unchecked macros**: Use `query_unchecked!()` for non-critical queries
3. **Fewer unique queries**: Use parameterized queries instead of building different query strings
4. **Query files**: Use `query_file!()` to keep SQL out of Rust code and reduce recompilation
