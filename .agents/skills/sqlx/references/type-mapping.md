# Type Mapping

sqlx maps between Rust types and SQL types. This reference covers all built-in mappings, feature-flagged types, and how to define custom type mappings.

## Built-in Type Mappings

### Numeric Types

| Rust Type | PostgreSQL       | MySQL             | SQLite  |
| --------- | ---------------- | ----------------- | ------- |
| `i8`      | —                | TINYINT           | INTEGER |
| `i16`     | SMALLINT         | SMALLINT          | INTEGER |
| `i32`     | INT, INTEGER     | INT               | INTEGER |
| `i64`     | BIGINT           | BIGINT            | INTEGER |
| `u8`      | —                | TINYINT UNSIGNED  | INTEGER |
| `u16`     | —                | SMALLINT UNSIGNED | INTEGER |
| `u32`     | —                | INT UNSIGNED      | INTEGER |
| `u64`     | —                | BIGINT UNSIGNED   | INTEGER |
| `f32`     | REAL             | FLOAT             | REAL    |
| `f64`     | DOUBLE PRECISION | DOUBLE            | REAL    |

### String and Binary Types

| Rust Type | PostgreSQL       | MySQL            | SQLite |
| --------- | ---------------- | ---------------- | ------ |
| `String`  | TEXT, VARCHAR(n) | TEXT, VARCHAR(n) | TEXT   |
| `&str`    | TEXT, VARCHAR(n) | TEXT, VARCHAR(n) | TEXT   |
| `Vec<u8>` | BYTEA            | BLOB             | BLOB   |

### Boolean

| Rust Type | PostgreSQL | MySQL   | SQLite        |
| --------- | ---------- | ------- | ------------- |
| `bool`    | BOOLEAN    | BOOLEAN | INTEGER (0/1) |

**SQLite pitfall**: SQLite stores booleans as 0/1 integers. In SQL queries, compare with `= 1` or `= 0`, not `= TRUE`.

### Date and Time Types

Requires the `chrono` or `time` feature flag.

| Rust Type                 | Feature  | PostgreSQL  | MySQL    | SQLite          |
| ------------------------- | -------- | ----------- | -------- | --------------- |
| `chrono::NaiveDateTime`   | `chrono` | TIMESTAMP   | DATETIME | TEXT (ISO 8601) |
| `chrono::DateTime<Utc>`   | `chrono` | TIMESTAMPTZ | DATETIME | TEXT            |
| `chrono::DateTime<Local>` | `chrono` | TIMESTAMPTZ | DATETIME | TEXT            |
| `chrono::NaiveDate`       | `chrono` | DATE        | DATE     | TEXT            |
| `chrono::NaiveTime`       | `chrono` | TIME        | TIME     | TEXT            |
| `time::PrimitiveDateTime` | `time`   | TIMESTAMP   | DATETIME | TEXT            |
| `time::OffsetDateTime`    | `time`   | TIMESTAMPTZ | DATETIME | TEXT            |
| `time::Date`              | `time`   | DATE        | DATE     | TEXT            |
| `time::Time`              | `time`   | TIME        | TIME     | TEXT            |

### UUID

Requires the `uuid` feature flag.

| Rust Type    | Feature | PostgreSQL | MySQL                | SQLite |
| ------------ | ------- | ---------- | -------------------- | ------ |
| `uuid::Uuid` | `uuid`  | UUID       | CHAR(36), BINARY(16) | TEXT   |

### JSON Types

The `json` feature is enabled by default.

| Rust Type              | PostgreSQL  | MySQL | SQLite |
| ---------------------- | ----------- | ----- | ------ |
| `serde_json::Value`    | JSON, JSONB | JSON  | TEXT   |
| `sqlx::types::Json<T>` | JSON, JSONB | JSON  | TEXT   |

### Decimal Types

| Rust Type                | Feature        | PostgreSQL       | MySQL   | SQLite |
| ------------------------ | -------------- | ---------------- | ------- | ------ |
| `rust_decimal::Decimal`  | `rust_decimal` | NUMERIC, DECIMAL | DECIMAL | TEXT   |
| `bigdecimal::BigDecimal` | `bigdecimal`   | NUMERIC, DECIMAL | DECIMAL | TEXT   |

### Nullable Types

| Rust Type   | SQL                                         |
| ----------- | ------------------------------------------- |
| `Option<T>` | Any nullable column. `None` maps to `NULL`. |

When reading, a `NULL` column always maps to `Option<T>::None`. When binding, `None` inserts `NULL`.

### Network Types (PostgreSQL only)

| Rust Type                 | Feature       | PostgreSQL  |
| ------------------------- | ------------- | ----------- |
| `ipnet::IpNet`            | `ipnet`       | INET, CIDR  |
| `ipnetwork::IpNetwork`    | `ipnetwork`   | INET, CIDR  |
| `mac_address::MacAddress` | `mac_address` | MACADDR     |
| `bit_vec::BitVec`         | `bit-vec`     | BIT, VARBIT |

### Array Types (PostgreSQL only)

| Rust Type        | PostgreSQL                          |
| ---------------- | ----------------------------------- |
| `Vec<T>`         | ARRAY (e.g., `TEXT[]`, `INTEGER[]`) |
| `Vec<Option<T>>` | ARRAY with NULL elements            |

Array support requires the element type `T` to implement `Encode`/`Decode`.

## FromRow Derive Attributes

```rust
use sqlx::FromRow;

#[derive(Debug, FromRow)]
struct User {
    id: i64,
    // Rename: map a SQL column to a differently-named Rust field
    #[sqlx(rename = "full_name")]
    name: String,

    // Default: provide a default value if the column is missing from the query
    #[sqlx(default)]
    nickname: String,

    // Skip: ignore this field entirely (field must have a Default impl)
    #[sqlx(skip)]
    computed_field: String,

    // Flatten: embed another FromRow struct (for JOINs)
    #[sqlx(flatten)]
    address: Address,

    // JSON: deserialize a JSON/JSONB column into a typed struct
    #[sqlx(json)]
    metadata: Json<Metadata>,

    // Try From: convert using TryFrom<&str> or similar
    #[sqlx(try_from = "String")]
    email: Email,
}
```

## Custom Type Implementations

### Transparent Wrapper (Newtype Pattern)

The simplest way to map a custom Rust type to a SQL type:

```rust
#[derive(Debug, sqlx::Type)]
#[sqlx(transparent)]
struct Email(String);
```

### Enum Mapping (PostgreSQL)

Map a Rust enum to a PostgreSQL enum type:

```rust
#[derive(Debug, sqlx::Type)]
#[sqlx(type_name = "user_role", rename_all = "lowercase")]
enum UserRole {
    Admin,
    User,
    Guest,
}
```

The corresponding PostgreSQL type must exist:

```sql
CREATE TYPE user_role AS ENUM ('admin', 'user', 'guest');
```

### Composite Type (PostgreSQL)

```rust
#[derive(Debug, sqlx::Type)]
#[sqlx(type_name = "address")]
struct Address {
    street: String,
    city: String,
    zip: String,
}
```

The corresponding PostgreSQL type:

```sql
CREATE TYPE address AS (
    street TEXT,
    city TEXT,
    zip TEXT
);
```

### Manual Encode/Decode Implementation

For types that need custom serialization logic:

```rust
impl<'q> sqlx::Encode<'q, sqlx::Postgres> for MyType {
    fn encode_by_ref(&self, buf: &mut sqlx::postgres::PgArgumentBuffer) -> sqlx::encode::IsNull {
        // Write to buffer
        sqlx::encode::IsNull::No
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for MyType {
    fn decode(value: sqlx::postgres::PgValueRef<'r>) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Read from value
        Ok(MyType { /* ... */ })
    }
}

impl sqlx::Type<sqlx::Postgres> for MyType {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        // Return the SQL type this maps to
        <String as sqlx::Type<sqlx::Postgres>>::type_info()
    }
}
```

## Text<T> Wrapper

`sqlx::types::Text<T>` maps any type that implements `Display` and `FromStr` to/from a SQL text column:

```rust
use sqlx::types::Text;

// If Email impl Display + FromStr
let email: Text<Email> = row.get("email");
// Read: Text(Email { ... })
// Write: binds as the Display string
```

This is useful for custom string types without writing full Encode/Decode implementations.
