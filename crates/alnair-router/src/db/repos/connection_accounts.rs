//! Extra API keys attached to one connection.
//!
//! The executor rotates across a connection's primary key and these accounts,
//! so several keys / quota buckets can back the same upstream endpoint.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::crypto::CredentialCipher;
use crate::error::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ConnectionAccount {
    pub id: String,
    pub connection_id: String,
    pub label: String,
    /// Encrypted at rest; decrypted on read and never serialized back out.
    #[serde(skip_serializing)]
    pub api_key: String,
    pub enabled: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ConnectionAccount {
    pub fn is_enabled(&self) -> bool {
        self.enabled != 0
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateConnectionAccount {
    pub label: String,
    pub api_key: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// Partial update. `null` clears the key (making the account keyless).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdateConnectionAccount {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default, deserialize_with = "crate::db::repos::double_option")]
    pub api_key: Option<Option<String>>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

fn default_true() -> bool {
    true
}

pub struct ConnectionAccountRepository {
    pool: SqlitePool,
    cipher: Arc<CredentialCipher>,
}

impl ConnectionAccountRepository {
    pub fn new(pool: SqlitePool, cipher: Arc<CredentialCipher>) -> Self {
        Self { pool, cipher }
    }

    pub async fn list(&self, connection_id: &str) -> Result<Vec<ConnectionAccount>> {
        let rows = sqlx::query_as::<_, ConnectionAccount>(
            "SELECT * FROM connection_accounts WHERE connection_id = ? ORDER BY created_at ASC",
        )
        .bind(connection_id)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(|row| self.decrypt(row)).collect()
    }

    pub async fn create(
        &self,
        connection_id: &str,
        input: CreateConnectionAccount,
    ) -> Result<ConnectionAccount> {
        self.ensure_connection(connection_id).await?;

        let label = normalized_label(&input.label)?;
        let api_key = normalized_secret(&input.api_key)?;
        let now = Utc::now();
        let id = uuid::Uuid::new_v4().to_string();

        let result = sqlx::query(
            "INSERT INTO connection_accounts
                (id, connection_id, label, api_key, enabled, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(connection_id)
        .bind(&label)
        .bind(self.cipher.encrypt(&api_key)?)
        .bind(i64::from(input.enabled))
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await;

        if let Err(error) = result {
            return Err(duplicate_label(error, &label));
        }

        self.get(connection_id, &id)
            .await?
            .ok_or_else(|| Error::Internal("account disappeared after insert".to_string()))
    }

    pub async fn update(
        &self,
        connection_id: &str,
        id: &str,
        input: UpdateConnectionAccount,
    ) -> Result<ConnectionAccount> {
        let existing = self
            .get(connection_id, id)
            .await?
            .ok_or_else(|| Error::NotFound(format!("account '{id}' not found")))?;

        let label = match input.label {
            Some(label) => normalized_label(&label)?,
            None => existing.label.clone(),
        };
        let api_key = match input.api_key {
            Some(Some(value)) => Some(self.cipher.encrypt(&normalized_secret(&value)?)?),
            Some(None) => None,
            None => Some(existing.api_key.clone()),
        };
        let enabled = input.enabled.unwrap_or(existing.is_enabled());

        let result = sqlx::query(
            "UPDATE connection_accounts
             SET label = ?, api_key = ?, enabled = ?, updated_at = ?
             WHERE id = ? AND connection_id = ?",
        )
        .bind(&label)
        .bind(&api_key)
        .bind(i64::from(enabled))
        .bind(Utc::now())
        .bind(id)
        .bind(connection_id)
        .execute(&self.pool)
        .await;

        if let Err(error) = result {
            return Err(duplicate_label(error, &label));
        }

        self.get(connection_id, id)
            .await?
            .ok_or_else(|| Error::Internal("account disappeared after update".to_string()))
    }

    pub async fn delete(&self, connection_id: &str, id: &str) -> Result<bool> {
        let result =
            sqlx::query("DELETE FROM connection_accounts WHERE id = ? AND connection_id = ?")
                .bind(id)
                .bind(connection_id)
                .execute(&self.pool)
                .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn get(&self, connection_id: &str, id: &str) -> Result<Option<ConnectionAccount>> {
        let row = sqlx::query_as::<_, ConnectionAccount>(
            "SELECT * FROM connection_accounts WHERE id = ? AND connection_id = ?",
        )
        .bind(id)
        .bind(connection_id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(|row| self.decrypt(row)).transpose()
    }

    async fn ensure_connection(&self, connection_id: &str) -> Result<()> {
        let exists: Option<(String,)> = sqlx::query_as("SELECT id FROM connections WHERE id = ?")
            .bind(connection_id)
            .fetch_optional(&self.pool)
            .await?;

        if exists.is_none() {
            return Err(Error::NotFound(format!(
                "connection '{connection_id}' not found"
            )));
        }
        Ok(())
    }

    fn decrypt(&self, mut account: ConnectionAccount) -> Result<ConnectionAccount> {
        account.api_key = self.cipher.decrypt(&account.api_key)?;
        Ok(account)
    }
}

fn normalized_label(value: &str) -> Result<String> {
    let label = value.trim();
    if label.is_empty() {
        return Err(Error::BadRequest("account label is required".to_string()));
    }
    Ok(label.to_string())
}

fn normalized_secret(value: &str) -> Result<String> {
    let secret = value.trim();
    if secret.is_empty() {
        return Err(Error::BadRequest("api_key is required".to_string()));
    }
    Ok(secret.to_string())
}

/// Turns a UNIQUE(label) violation into a friendly 400.
fn duplicate_label(error: sqlx::Error, label: &str) -> Error {
    match &error {
        sqlx::Error::Database(database) if database.is_unique_violation() => Error::BadRequest(
            format!("an account labelled '{label}' already exists on this connection"),
        ),
        _ => Error::Database(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::db::repos::connections::{ConnectionRepository, CreateConnection};

    const SECRET: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    async fn fixtures() -> (ConnectionAccountRepository, String) {
        let db = Db::connect_in_memory().await.expect("db");
        let cipher = Arc::new(
            CredentialCipher::from_config(&crate::config::SecretsConfig {
                key: Some(SECRET.to_string()),
            })
            .expect("cipher"),
        );
        let connection = ConnectionRepository::new(db.pool.clone(), cipher.clone())
            .create(CreateConnection {
                name: "openai-main".to_string(),
                provider_type: "openai-compatible".to_string(),
                base_url: "https://example.invalid/v1".to_string(),
                api_key: Some("sk-primary".to_string()),
                custom_headers: Default::default(),
                enabled: true,
                connect_timeout_ms: None,
                idle_timeout_ms: None,
                pricing_model: None,
                cache_retention: None,
                auth_style: None,
                provider_id: None,
            })
            .await
            .expect("connection");

        (
            ConnectionAccountRepository::new(db.pool.clone(), cipher),
            connection.id,
        )
    }

    #[tokio::test]
    async fn accounts_round_trip_and_are_encrypted() {
        let (repository, connection_id) = fixtures().await;

        let created = repository
            .create(
                &connection_id,
                CreateConnectionAccount {
                    label: "backup".to_string(),
                    api_key: "sk-backup".to_string(),
                    enabled: true,
                },
            )
            .await
            .expect("create");
        assert_eq!(created.api_key, "sk-backup");

        let list = repository.list(&connection_id).await.expect("list");
        assert_eq!(list.len(), 1);

        // The stored value is ciphertext, not the plaintext key.
        let stored: (String,) =
            sqlx::query_as("SELECT api_key FROM connection_accounts WHERE id = ?")
                .bind(&created.id)
                .fetch_one(&repository.pool)
                .await
                .expect("stored");
        assert!(stored.0.starts_with("enc:v1:"));

        assert!(
            repository
                .delete(&connection_id, &created.id)
                .await
                .expect("delete")
        );
        assert!(
            repository
                .list(&connection_id)
                .await
                .expect("list")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn duplicate_labels_are_rejected() {
        let (repository, connection_id) = fixtures().await;

        repository
            .create(
                &connection_id,
                CreateConnectionAccount {
                    label: "same".to_string(),
                    api_key: "sk-1".to_string(),
                    enabled: true,
                },
            )
            .await
            .expect("first");

        let error = repository
            .create(
                &connection_id,
                CreateConnectionAccount {
                    label: "same".to_string(),
                    api_key: "sk-2".to_string(),
                    enabled: true,
                },
            )
            .await
            .expect_err("duplicate");
        assert!(matches!(error, Error::BadRequest(_)), "{error:?}");
        assert_eq!(
            repository.list(&connection_id).await.expect("list").len(),
            1
        );
    }
}
