//! SQLite persistence: connection pool, migrations, and repositories.

pub mod repos;

use std::str::FromStr;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

use crate::config::{RouterConfig, router_home};
use crate::error::{Error, Result};

/// Shared SQLite handle.
#[derive(Clone)]
pub struct Db {
    pub pool: SqlitePool,
}

impl Db {
    /// Opens the pool described by `config`, creating the default database directory if needed.
    pub async fn connect(config: &RouterConfig) -> Result<Self> {
        let options = match config.storage.url.trim() {
            "" => {
                let path = default_database_path();
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(|error| {
                        Error::Config(format!("failed to create database directory: {error}"))
                    })?;
                }
                SqliteConnectOptions::new()
                    .filename(&path)
                    .create_if_missing(true)
                    .foreign_keys(true)
            }
            url => SqliteConnectOptions::from_str(url)
                .map_err(|error| Error::Config(format!("invalid storage url '{url}': {error}")))?
                .create_if_missing(true)
                .foreign_keys(true),
        };

        let pool = SqlitePoolOptions::new()
            .max_connections(8)
            .connect_with(options)
            .await?;

        Ok(Self { pool })
    }

    /// Opens an in-memory database with migrations applied. Used by tests.
    pub async fn connect_in_memory() -> Result<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::from_str("sqlite::memory:")
                    .map_err(|error| Error::Config(error.to_string()))?
                    .foreign_keys(true),
            )
            .await?;
        let db = Self { pool };
        db.migrate().await?;
        Ok(db)
    }

    /// Applies the embedded migrations.
    pub async fn migrate(&self) -> Result<()> {
        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await
            .map_err(|error| Error::Database(sqlx::Error::Migrate(Box::new(error))))?;
        Ok(())
    }

    /// Encrypts legacy plaintext connection credentials and verifies that every
    /// stored value decrypts with the configured key.
    ///
    /// Refuses to proceed when a cipher is required but unavailable, or when an
    /// encrypted row cannot be decrypted (wrong `secrets.key`). Returns the
    /// number of rows that were rewritten.
    pub async fn migrate_credentials(
        &self,
        cipher: &crate::crypto::CredentialCipher,
    ) -> Result<usize> {
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT id, api_key FROM connections WHERE api_key IS NOT NULL")
                .fetch_all(&self.pool)
                .await?;

        let mut re_encrypted = 0;
        for (id, stored) in rows {
            if crate::crypto::CredentialCipher::is_encrypted(&stored) {
                cipher.decrypt(&stored)?;
                continue;
            }

            let encrypted = cipher.encrypt(&stored)?;
            sqlx::query("UPDATE connections SET api_key = ? WHERE id = ?")
                .bind(&encrypted)
                .bind(&id)
                .execute(&self.pool)
                .await?;
            re_encrypted += 1;
        }

        Ok(re_encrypted)
    }
}

/// `$ALNAIR_ROUTER_HOME/db/router.sqlite`.
pub fn default_database_path() -> std::path::PathBuf {
    router_home().join("db").join("router.sqlite")
}
