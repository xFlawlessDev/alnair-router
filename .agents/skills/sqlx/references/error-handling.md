# Error Handling

## sqlx::Error Enum

sqlx provides a comprehensive error type through `sqlx::Error`. Understanding its variants is essential for writing robust database code.

```rust
use sqlx::Error;

match result {
    Ok(user) => println!("Found user: {}", user.name),
    Err(Error::RowNotFound) => println!("No user found"),
    Err(Error::Database(db_err)) => {
        println!("DB error: {}", db_err.message());
        if let Some(code) = db_err.code() {
            println!("Error code: {}", code);
        }
    }
    Err(Error::PoolTimedOut) => println!("Timed out waiting for a connection"),
    Err(Error::PoolClosed) => println!("Connection pool was closed"),
    Err(Error::Io(e)) => println!("IO error: {}", e),
    Err(Error::Tls(e)) => println!("TLS error: {}", e),
    Err(Error::Configuration(e)) => println!("Config error: {}", e),
    Err(Error::Protocol(e)) => println!("Protocol error: {}", e),
    Err(Error::ColumnNotFound(name)) => println!("Column '{}' not found in row", name),
    Err(Error::Decode(e)) => println!("Failed to decode value: {}", e),
    Err(Error::Encode(e)) => println!("Failed to encode parameter: {}", e),
    Err(Error::TypeNotFound { type_name }) => println!("Unknown SQL type: {}", type_name),
    Err(Error::Migration(e)) => println!("Migration error: {}", e),
    Err(Error::WorkerCrashed) => println!("Background pool worker panicked"),
    Err(e) => println!("Other error: {}", e),
}
```

## Common Error Variants in Detail

### RowNotFound

Returned by `fetch_one()` and `fetch_optional()` (as `None`) when no rows match:

```rust
// fetch_one returns Error::RowNotFound if 0 rows
let user = sqlx::query_as!(User, "SELECT * FROM users WHERE id = $1", 999)
    .fetch_one(&pool)
    .await
    .unwrap_err(); // Error::RowNotFound

// fetch_optional returns Option<T>
let user: Option<User> = sqlx::query_as!(User, "SELECT * FROM users WHERE id = $1", 999)
    .fetch_optional(&pool)
    .await?;
// user is None — no error
```

### Database Error

Wraps database-specific errors with rich context:

```rust
if let Error::Database(db_err) = result.err() {
    // The human-readable error message
    println!("Message: {}", db_err.message());

    // The SQLSTATE error code (PostgreSQL/MySQL)
    if let Some(code) = db_err.code() {
        println!("Code: {}", code);
    }

    // The constraint that was violated (if applicable)
    if let Some(constraint) = db_err.constraint() {
        println!("Constraint: {}", constraint);
    }

    // The table involved (if applicable)
    if let Some(table) = db_err.table() {
        println!("Table: {}", table);
    }

    // The column involved (if applicable)
    if let Some(column) = db_err.column() {
        println!("Column: {}", column);
    }

    // The original error from the database driver
    println!("Original: {:?}", db_err.into_source());
}
```

### PostgreSQL Error Codes

Common SQLSTATE codes to handle:

| Code    | Name                                        | Common Cause                       |
| ------- | ------------------------------------------- | ---------------------------------- |
| `23505` | unique_violation                            | Duplicate key on UNIQUE constraint |
| `23503` | foreign_key_violation                       | Referencing non-existent row       |
| `23502` | not_null_violation                          | NULL in NOT NULL column            |
| `42P01` | undefined_table                             | Table doesn't exist                |
| `42703` | undefined_column                            | Column doesn't exist               |
| `42601` | syntax_error                                | SQL syntax error                   |
| `08001` | sqlclient_unable_to_establish_sqlconnection | Connection failure                 |
| `08006` | connection_failure                          | Connection dropped                 |
| `57014` | query_canceled                              | Statement timeout                  |
| `54001` | statement_too_complex                       | Query too complex                  |
| `55P03` | lock_not_available                          | Can't acquire lock                 |

### Practical Error Handling Patterns

#### Unique Constraint (Duplicate Entry)

```rust
async fn create_user(pool: &PgPool, name: &str, email: &str) -> Result<User, CreateUserError> {
    sqlx::query_as!(
        User,
        "INSERT INTO users (name, email) VALUES ($1, $2) RETURNING *",
        name, email
    )
    .fetch_one(pool)
    .await
    .map_err(|e| match e {
        Error::Database(db_err) if db_err.code().as_deref() == Some("23505") => {
            CreateUserError::AlreadyExists(db_err.constraint().unwrap_or("unknown").to_string())
        }
        e => CreateUserError::Database(e),
    })
}

#[derive(Debug)]
enum CreateUserError {
    AlreadyExists(String),
    Database(sqlx::Error),
}
```

#### Not Found vs Other Errors

```rust
async fn get_user(pool: &PgPool, id: i64) -> Result<User, AppError> {
    sqlx::query_as!(User, "SELECT * FROM users WHERE id = $1", id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound(format!("User {} not found", id)))
}

#[derive(Debug)]
enum AppError {
    NotFound(String),
    Database(sqlx::Error),
}
```

#### Pool Timeout

```rust
use std::time::Duration;

// Increase acquire timeout if timeouts are frequent
let pool = PgPoolOptions::new()
    .acquire_timeout(Duration::from_secs(10))
    .max_connections(20)
    .connect(&database_url)
    .await?;

// Handle timeout gracefully
match pool.acquire().await {
    Ok(conn) => { /* use connection */ }
    Err(Error::PoolTimedOut) => {
        // Retry or return a service-unavailable response
        eprintln!("Connection pool exhausted");
    }
    Err(e) => return Err(e),
}
```

## Error Conversion

### IntoResponse for Axum

```rust
use axum::response::{IntoResponse, Response};
use axum::http::StatusCode;

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized".into()),
            AppError::Validation(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            AppError::Database(Error::RowNotFound) => (StatusCode::NOT_FOUND, "Not found".into()),
            AppError::Database(Error::Database(db_err))
                if db_err.code().as_deref() == Some("23505") =>
                    (StatusCode::CONFLICT, "Resource already exists".into()),
            AppError::Database(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Database error".into()),
        };
        (status, message).into_response()
    }
}
```

### From<sqlx::Error> for Custom Errors

```rust
#[derive(Debug)]
enum MyError {
    Database(sqlx::Error),
    NotFound(String),
}

impl From<sqlx::Error> for MyError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::RowNotFound => MyError::NotFound("Resource not found".into()),
            other => MyError::Database(other),
        }
    }
}
```

This lets you use `?` directly:

```rust
async fn get_user(pool: &PgPool, id: i64) -> Result<User, MyError> {
    let user = sqlx::query_as!(User, "SELECT * FROM users WHERE id = $1", id)
        .fetch_one(pool)
        .await?;  // RowNotFound automatically converts to MyError::NotFound
    Ok(user)
}
```
