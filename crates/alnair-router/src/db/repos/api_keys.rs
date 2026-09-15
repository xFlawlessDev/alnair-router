//! Router-issued client API keys keyed.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, SqlitePool};

use crate::error::{Error, Result};

const KEY_PREFIX: &str = "sk-router-";

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ApiKey {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing)]
    pub key_hash: String,
    pub prefix: String,
    pub enabled: i64,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

impl ApiKey {
    pub fn is_enabled(&self) -> bool {
        self.enabled != 0
    }
}

/// A newly minted key plus the plaintext secret, returned exactly once.
#[derive(Debug, Clone, Serialize)]
pub struct CreatedApiKey {
    pub key: ApiKey,
    /// Plaintext key. Only available at creation time; never persisted.
    pub secret: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateApiKey {
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// SHA-256 hex digest of a plaintext key.
pub fn hash_key(secret: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub struct ApiKeyRepository {
    pool: SqlitePool,
}

impl ApiKeyRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn list(&self) -> Result<Vec<ApiKey>> {
        let rows = sqlx::query_as::<_, ApiKey>("SELECT * FROM api_keys ORDER BY created_at DESC")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows)
    }

    pub async fn create(&self, input: CreateApiKey) -> Result<CreatedApiKey> {
        let name = input.name.trim();
        if name.is_empty() {
            return Err(Error::BadRequest("name is required".to_string()));
        }

        let secret = format!("{KEY_PREFIX}{}", uuid::Uuid::new_v4().simple());
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();
        let prefix: String = secret.chars().take(KEY_PREFIX.len() + 8).collect();

        sqlx::query(
            "INSERT INTO api_keys (id, name, key_hash, prefix, enabled, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(name)
        .bind(hash_key(&secret))
        .bind(&prefix)
        .bind(i64::from(input.enabled))
        .bind(now)
        .execute(&self.pool)
        .await?;

        let key = sqlx::query_as::<_, ApiKey>("SELECT * FROM api_keys WHERE id = ?")
            .bind(&id)
            .fetch_one(&self.pool)
            .await?;

        Ok(CreatedApiKey { key, secret })
    }

    /// Looks up an enabled key by its plaintext secret.
    pub async fn find_by_secret(&self, secret: &str) -> Result<Option<ApiKey>> {
        let row = sqlx::query_as::<_, ApiKey>(
            "SELECT * FROM api_keys WHERE key_hash = ? AND enabled = 1",
        )
        .bind(hash_key(secret))
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn touch(&self, id: &str) -> Result<()> {
        sqlx::query("UPDATE api_keys SET last_used_at = ? WHERE id = ?")
            .bind(Utc::now())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM api_keys WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}
