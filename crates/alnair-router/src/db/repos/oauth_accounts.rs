//! OAuth accounts attached to one connection.
//!
//! The whole credential document is encrypted as a single JSON blob through
//! `CredentialCipher`, so access token, refresh token and the endpoints needed
//! to renew them are rotated as a unit. Only this module may touch the
//! `credential` column — the same rule that protects `connections.api_key`.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::crypto::CredentialCipher;
use crate::error::{Error, Result};
use crate::oauth::credential::OAuthCredential;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct OAuthAccount {
    pub id: String,
    pub connection_id: String,
    pub label: String,
    /// Endpoint preset the login used: `gitlab-duo`, `google` or `generic`.
    pub provider_key: String,
    /// Encrypted at rest; decrypted on read and never serialized back out.
    #[serde(skip_serializing)]
    pub credential: OAuthCredential,
    pub enabled: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl OAuthAccount {
    pub fn is_enabled(&self) -> bool {
        self.enabled != 0
    }

    /// The credential document, decrypted.
    pub fn credential(&self) -> &OAuthCredential {
        &self.credential
    }
}

/// A row shaped for `sqlx`, where `credential` is the raw stored string.
#[derive(Debug, FromRow)]
struct StoredOAuthAccount {
    id: String,
    connection_id: String,
    label: String,
    provider_key: String,
    credential: String,
    enabled: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateOAuthAccount {
    pub label: String,
    #[serde(default = "default_provider_key")]
    pub provider_key: String,
    pub credential: OAuthCredential,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// Partial update. The credential is never replaced by a PATCH: a renewal goes
/// through [`OAuthAccountRepository::store_credential`].
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdateOAuthAccount {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

fn default_true() -> bool {
    true
}

fn default_provider_key() -> String {
    "generic".to_string()
}

pub struct OAuthAccountRepository {
    pool: SqlitePool,
    cipher: Arc<CredentialCipher>,
}

impl OAuthAccountRepository {
    pub fn new(pool: SqlitePool, cipher: Arc<CredentialCipher>) -> Self {
        Self { pool, cipher }
    }

    pub async fn list(&self, connection_id: &str) -> Result<Vec<OAuthAccount>> {
        let rows = sqlx::query_as::<_, StoredOAuthAccount>(
            "SELECT * FROM oauth_accounts WHERE connection_id = ? ORDER BY created_at ASC",
        )
        .bind(connection_id)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(|row| self.decrypt(row)).collect()
    }

    pub async fn get(&self, connection_id: &str, id: &str) -> Result<Option<OAuthAccount>> {
        let row = sqlx::query_as::<_, StoredOAuthAccount>(
            "SELECT * FROM oauth_accounts WHERE id = ? AND connection_id = ?",
        )
        .bind(id)
        .bind(connection_id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(|row| self.decrypt(row)).transpose()
    }

    /// Reads one account by id alone, for the executor's refresh path, which
    /// knows the account but not the connection it hangs off.
    pub async fn find(&self, id: &str) -> Result<Option<OAuthAccount>> {
        let row =
            sqlx::query_as::<_, StoredOAuthAccount>("SELECT * FROM oauth_accounts WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;

        row.map(|row| self.decrypt(row)).transpose()
    }

    pub async fn create(
        &self,
        connection_id: &str,
        input: CreateOAuthAccount,
    ) -> Result<OAuthAccount> {
        self.ensure_connection(connection_id).await?;

        let label = normalized_label(&input.label)?;
        let provider_key = normalized_provider_key(&input.provider_key)?;
        let now = Utc::now();
        let id = uuid::Uuid::new_v4().to_string();

        let result = sqlx::query(
            "INSERT INTO oauth_accounts
                (id, connection_id, label, provider_key, credential, enabled, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(connection_id)
        .bind(&label)
        .bind(&provider_key)
        .bind(self.encrypt(&input.credential)?)
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
            .ok_or_else(|| Error::Internal("oauth account disappeared after insert".to_string()))
    }

    /// Rewrites the credential blob after a refresh.
    pub async fn store_credential(&self, id: &str, credential: &OAuthCredential) -> Result<()> {
        sqlx::query("UPDATE oauth_accounts SET credential = ?, updated_at = ? WHERE id = ?")
            .bind(self.encrypt(credential)?)
            .bind(Utc::now())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update(
        &self,
        connection_id: &str,
        id: &str,
        input: UpdateOAuthAccount,
    ) -> Result<OAuthAccount> {
        let existing = self
            .get(connection_id, id)
            .await?
            .ok_or_else(|| Error::NotFound(format!("account '{id}' not found")))?;

        let label = match input.label {
            Some(label) => normalized_label(&label)?,
            None => existing.label.clone(),
        };
        let enabled = input.enabled.unwrap_or(existing.is_enabled());

        let result = sqlx::query(
            "UPDATE oauth_accounts
             SET label = ?, enabled = ?, updated_at = ?
             WHERE id = ? AND connection_id = ?",
        )
        .bind(&label)
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
            .ok_or_else(|| Error::Internal("oauth account disappeared after update".to_string()))
    }

    pub async fn delete(&self, connection_id: &str, id: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM oauth_accounts WHERE id = ? AND connection_id = ?")
            .bind(id)
            .bind(connection_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
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

    fn encrypt(&self, credential: &OAuthCredential) -> Result<String> {
        let json = serde_json::to_string(credential)
            .map_err(|error| Error::Internal(format!("cannot serialize credential: {error}")))?;
        self.cipher.encrypt(&json)
    }

    fn decrypt(&self, row: StoredOAuthAccount) -> Result<OAuthAccount> {
        let json = self.cipher.decrypt(&row.credential)?;
        let credential: OAuthCredential = serde_json::from_str(&json).map_err(|error| {
            Error::Internal(format!("stored OAuth credential is malformed: {error}"))
        })?;

        Ok(OAuthAccount {
            id: row.id,
            connection_id: row.connection_id,
            label: row.label,
            provider_key: row.provider_key,
            credential,
            enabled: row.enabled,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

fn normalized_label(value: &str) -> Result<String> {
    let label = value.trim();
    if label.is_empty() {
        return Err(Error::BadRequest("account label is required".to_string()));
    }
    Ok(label.to_string())
}

fn normalized_provider_key(value: &str) -> Result<String> {
    let key = value.trim();
    if key.is_empty() {
        return Ok(default_provider_key());
    }
    Ok(key.to_string())
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
    use crate::config::SecretsConfig;
    use crate::db::Db;
    use crate::db::repos::connections::{ConnectionRepository, CreateConnection};
    use crate::oauth::credential::{AccountInfo, EndpointConfig};

    const SECRET: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    fn credential(access: &str) -> OAuthCredential {
        OAuthCredential {
            access_token: access.to_string(),
            refresh_token: Some("refresh-1".to_string()),
            expires_at: None,
            token_type: Some("Bearer".to_string()),
            scope: None,
            endpoints: EndpointConfig {
                client_id: "client-1".to_string(),
                client_secret: Some("secret-1".to_string()),
                authorize_url: "https://example.invalid/authorize".to_string(),
                token_url: "https://example.invalid/token".to_string(),
                device_code_url: None,
                user_info_url: None,
                scopes: "read".to_string(),
            },
            account: Some(AccountInfo {
                username: Some("octocat".to_string()),
                ..AccountInfo::default()
            }),
        }
    }

    async fn fixtures() -> (OAuthAccountRepository, Db, String) {
        let db = Db::connect_in_memory().await.expect("db");
        let cipher = Arc::new(
            CredentialCipher::from_config(&SecretsConfig {
                key: Some(SECRET.to_string()),
            })
            .expect("cipher"),
        );
        let connection = ConnectionRepository::new(db.pool.clone(), cipher.clone())
            .create(CreateConnection {
                name: "gitlab-duo".to_string(),
                provider_type: "anthropic-native".to_string(),
                base_url: "https://gitlab.com/api/v4".to_string(),
                api_key: None,
                custom_headers: Default::default(),
                enabled: true,
                connect_timeout_ms: None,
                idle_timeout_ms: None,
                pricing_model: None,
                cache_retention: None,
                auth_style: Some("bearer".to_string()),
                provider_id: None,
            })
            .await
            .expect("connection");

        (
            OAuthAccountRepository::new(db.pool.clone(), cipher),
            db,
            connection.id,
        )
    }

    #[tokio::test]
    async fn accounts_round_trip_and_are_encrypted() {
        let (repository, db, connection_id) = fixtures().await;

        let created = repository
            .create(
                &connection_id,
                CreateOAuthAccount {
                    label: "work".to_string(),
                    provider_key: "gitlab-duo".to_string(),
                    credential: credential("access-1"),
                    enabled: true,
                },
            )
            .await
            .expect("create");

        assert_eq!(created.credential().access_token, "access-1");
        assert_eq!(created.provider_key, "gitlab-duo");
        assert!(created.is_enabled());

        // The stored column is ciphertext, and the plaintext token is in it nowhere.
        let stored: (String,) =
            sqlx::query_as("SELECT credential FROM oauth_accounts WHERE id = ?")
                .bind(&created.id)
                .fetch_one(&db.pool)
                .await
                .expect("stored");
        assert!(stored.0.starts_with("enc:v1:"));
        assert!(!stored.0.contains("access-1"));
        assert!(!stored.0.contains("secret-1"));

        // Read-back restores the whole document.
        let read = repository
            .get(&connection_id, &created.id)
            .await
            .expect("get")
            .expect("some");
        assert_eq!(read.credential(), &credential("access-1"));

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
    async fn a_refresh_rewrites_only_the_credential() {
        let (repository, _db, connection_id) = fixtures().await;
        let created = repository
            .create(
                &connection_id,
                CreateOAuthAccount {
                    label: "work".to_string(),
                    provider_key: "gitlab-duo".to_string(),
                    credential: credential("access-1"),
                    enabled: true,
                },
            )
            .await
            .expect("create");

        let mut renewed = created.credential.clone();
        renewed.access_token = "access-2".to_string();
        repository
            .store_credential(&created.id, &renewed)
            .await
            .expect("store");

        let read = repository
            .find(&created.id)
            .await
            .expect("find")
            .expect("some");
        assert_eq!(read.credential().access_token, "access-2");
        // The endpoints survive the rotation, so the next refresh still works.
        assert_eq!(
            read.credential().endpoints.client_id,
            created.credential.endpoints.client_id
        );
        assert_eq!(read.label, "work");
    }

    #[tokio::test]
    async fn duplicate_labels_are_rejected() {
        let (repository, _db, connection_id) = fixtures().await;

        let body = || CreateOAuthAccount {
            label: "same".to_string(),
            provider_key: "generic".to_string(),
            credential: credential("access-1"),
            enabled: true,
        };

        repository
            .create(&connection_id, body())
            .await
            .expect("first");

        let error = repository
            .create(&connection_id, body())
            .await
            .expect_err("duplicate");
        assert!(matches!(error, Error::BadRequest(_)), "{error:?}");
        assert_eq!(
            repository.list(&connection_id).await.expect("list").len(),
            1
        );
    }

    #[tokio::test]
    async fn deleting_the_connection_cascades_to_its_accounts() {
        let (repository, db, connection_id) = fixtures().await;
        repository
            .create(
                &connection_id,
                CreateOAuthAccount {
                    label: "work".to_string(),
                    provider_key: "generic".to_string(),
                    credential: credential("access-1"),
                    enabled: true,
                },
            )
            .await
            .expect("create");

        sqlx::query("DELETE FROM connections WHERE id = ?")
            .bind(&connection_id)
            .execute(&db.pool)
            .await
            .expect("delete connection");

        let remaining: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM oauth_accounts")
            .fetch_one(&db.pool)
            .await
            .expect("count");
        assert_eq!(remaining.0, 0);
    }
}
