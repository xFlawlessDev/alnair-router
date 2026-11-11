# Testing and Mocking

## #[sqlx::test] Attribute Macro

The `#[sqlx::test]` macro is sqlx's primary testing tool. It automatically creates an isolated test database for each test, applies migrations and fixtures, and cleans up afterward.

### Basic Usage

```rust
use sqlx::PgPool;

#[sqlx::test]
async fn test_create_user(pool: PgPool) -> sqlx::Result<()> {
    // An isolated database is created automatically
    sqlx::query!("INSERT INTO users (name) VALUES ($1)", "Alice")
        .execute(&pool)
        .await?;

    let user = sqlx::query_as!(User, "SELECT * FROM users WHERE name = $1", "Alice")
        .fetch_one(&pool)
        .await?;

    assert_eq!(user.name, "Alice");
    Ok(())
}
```

### With Migrations

Automatically applies migrations before the test runs:

```rust
#[sqlx::test(migrations = "./migrations")]
async fn test_with_schema(pool: PgPool) -> sqlx::Result<()> {
    // All tables, indexes, and seed data from migrations/ are ready
    sqlx::query!("INSERT INTO users (name, email) VALUES ($1, $2)", "Bob", "bob@test.com")
        .execute(&pool)
        .await?;
    Ok(())
}
```

### With Fixtures

Apply additional SQL files (test data) after migrations:

```rust
// tests/fixtures/users.sql
// INSERT INTO users (name, email) VALUES ('Alice', 'alice@example.com');
// INSERT INTO users (name, email) VALUES ('Bob', 'bob@example.com');

#[sqlx::test(migrations = "./migrations", fixtures("tests/fixtures/users.sql"))]
async fn test_with_fixtures(pool: PgPool) -> sqlx::Result<()> {
    // users.sql has been executed — test data is ready
    let count: i64 = sqlx::query_scalar!("SELECT COUNT(*) FROM users")
        .fetch_one(&pool)
        .await?;
    assert_eq!(count, 2);
    Ok(())
}
```

### Multiple Fixtures

```rust
#[sqlx::test(
    migrations = "./migrations",
    fixtures("tests/fixtures/users.sql", "tests/fixtures/posts.sql")
)]
async fn test_with_multiple_fixtures(pool: PgPool) -> sqlx::Result<()> {
    Ok(())
}
```

### Custom Pool Options and Connect Options

For tests that need specific pool or connection configuration:

```rust
use sqlx::postgres::{PgPoolOptions, PgConnectOptions};

#[sqlx::test]
async fn test_with_custom_options(
    pool_options: PgPoolOptions,
    options: PgConnectOptions,
) -> sqlx::Result<()> {
    let pool = pool_options
        .max_connections(5)
        .connect_with(options)
        .await?;
    Ok(())
}
```

### Supported Function Signatures

The macro recognizes these parameter patterns:

| Parameters                                                 | Behavior                                  |
| ---------------------------------------------------------- | ----------------------------------------- |
| `(pool: PgPool)`                                           | Pool is created and managed automatically |
| `(pool: MySqlPool)`                                        | MySQL pool (requires MySQL DATABASE_URL)  |
| `(pool: SqlitePool)`                                       | SQLite pool (no DATABASE_URL needed)      |
| `(conn: PoolConnection<Postgres>)`                         | Single connection from a pool             |
| `(pool_options: PgPoolOptions, options: PgConnectOptions)` | Custom configuration                      |

### Database Requirements

| Database   | DATABASE_URL Required | Notes                                                   |
| ---------- | --------------------- | ------------------------------------------------------- |
| PostgreSQL | Yes (superuser)       | Creates/drops test databases                            |
| MySQL      | Yes (superuser)       | Creates/drops test databases                            |
| SQLite     | No                    | Uses in-memory or file-based at `target/sqlx/test-dbs/` |

### Test Database Lifecycle

- **Before test**: A new database is created, migrations are applied, fixtures are executed
- **On success**: The database is deleted
- **On failure**: The database is left intact for debugging
- **On next run**: Previous failed test databases are cleaned up

## Repository Pattern for Testability

For unit tests that don't need a real database, abstract SQLx behind a trait:

```rust
use async_trait::async_trait;

#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn find_by_id(&self, id: i64) -> Result<Option<User>>;
    async fn create(&self, name: &str, email: &str) -> Result<User>;
    async fn list_active(&self) -> Result<Vec<User>>;
}

// Production implementation
pub struct PgUserRepository {
    pool: PgPool,
}

#[async_trait]
impl UserRepository for PgUserRepository {
    async fn find_by_id(&self, id: i64) -> Result<Option<User>> {
        sqlx::query_as!(User, "SELECT * FROM users WHERE id = $1", id)
            .fetch_optional(&self.pool)
            .await
            .map_err(Into::into)
    }

    async fn create(&self, name: &str, email: &str) -> Result<User> {
        sqlx::query_as!(
            User,
            "INSERT INTO users (name, email) VALUES ($1, $2) RETURNING id, name, email, created_at",
            name, email
        )
        .fetch_one(&self.pool)
        .await
        .map_err(Into::into)
    }

    async fn list_active(&self) -> Result<Vec<User>> {
        sqlx::query_as!(User, "SELECT * FROM users WHERE active = true")
            .fetch_all(&self.pool)
            .await
            .map_err(Into::into)
    }
}

// Mock implementation for unit tests
pub struct MockUserRepository {
    users: std::sync::Mutex<Vec<User>>,
}

#[async_trait]
impl UserRepository for MockUserRepository {
    async fn find_by_id(&self, id: i64) -> Result<Option<User>> {
        let users = self.users.lock().unwrap();
        Ok(users.iter().find(|u| u.id == id).cloned())
    }

    async fn create(&self, name: &str, email: &str) -> Result<User> {
        let mut users = self.users.lock().unwrap();
        let user = User { id: users.len() as i64 + 1, name: name.to_string(), email: email.to_string(), active: true };
        users.push(user.clone());
        Ok(user)
    }

    async fn list_active(&self) -> Result<Vec<User>> {
        let users = self.users.lock().unwrap();
        Ok(users.iter().filter(|u| u.active).cloned().collect())
    }
}

// Usage in a service
pub struct UserService<U: UserRepository> {
    repo: U,
}

impl<U: UserRepository> UserService<U> {
    pub async fn get_user(&self, id: i64) -> Result<User> {
        self.repo.find_by_id(id).await?
            .ok_or_else(|| anyhow::anyhow!("User not found"))
    }
}
```

## In-Memory SQLite for Tests

SQLite in-memory databases are excellent for fast, isolated tests when your SQL is database-agnostic:

```rust
use sqlx::SqlitePool;

async fn setup_test_db() -> SqlitePool {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::query("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL)")
        .execute(&pool)
        .await
        .unwrap();
    pool
}

#[tokio::test]
async fn test_user_creation() {
    let pool = setup_test_db().await;
    sqlx::query("INSERT INTO users (name) VALUES ($1)")
        .bind("Alice")
        .execute(&pool)
        .await
        .unwrap();
    // ... assertions
}
```

**Caveat**: In-memory SQLite databases are connection-specific. If the pool creates multiple connections, they won't share the same in-memory database. Use `max_connections(1)` or a file-based database.

## Common Testing Patterns

### Setup/Teardown with Fixtures

```rust
async fn seed_users(pool: &PgPool) -> sqlx::Result<()> {
    sqlx::query!("INSERT INTO users (name, email) VALUES ($1, $2)", "Alice", "alice@test.com")
        .execute(pool).await?;
    sqlx::query!("INSERT INTO users (name, email) VALUES ($1, $2)", "Bob", "bob@test.com")
        .execute(pool).await?;
    Ok(())
}

#[sqlx::test(migrations = "./migrations")]
async fn test_list_users(pool: PgPool) -> sqlx::Result<()> {
    seed_users(&pool).await?;
    let users = sqlx::query_as!(User, "SELECT * FROM users")
        .fetch_all(&pool).await?;
    assert_eq!(users.len(), 2);
    Ok(())
}
```

### Integration Test File Structure

```
tests/
├── common/
│   └── mod.rs          # Shared helpers (setup, seed functions)
├── user_tests.rs        # #[sqlx::test] tests for user operations
├── post_tests.rs        # #[sqlx::test] tests for post operations
└── fixtures/
    ├── users.sql        # Test data for users
    └── posts.sql        # Test data for posts
```
