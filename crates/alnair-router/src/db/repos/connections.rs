//! Upstream provider endpoint records.

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::crypto::CredentialCipher;
use crate::error::{Error, Result};

/// The upstream families supported by the router.
pub const SUPPORTED_PROVIDER_TYPES: [&str; 3] =
    ["openai-compatible", "anthropic-native", "command-code"];

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Connection {
    pub id: String,
    pub name: String,
    pub provider_type: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub custom_headers: String,
    pub enabled: i64,
    /// Connect/first-byte timeout override in milliseconds; NULL inherits.
    pub connect_timeout_ms: Option<i64>,
    /// Stream idle timeout override in milliseconds; NULL inherits.
    pub idle_timeout_ms: Option<i64>,
    /// Model id used for price lookups when the upstream id differs from the
    /// catalog (e.g. relay paths). NULL falls back to the upstream model id.
    pub pricing_model: Option<String>,
    /// Built-in preset this connection was created from, if any.
    pub provider_id: Option<String>,
    /// Enabled extra keys from `connection_accounts`; loaded by the catalog and
    /// never serialized into API responses.
    #[sqlx(skip)]
    #[serde(skip_serializing, default)]
    pub extra_keys: Vec<String>,
    /// Enabled extra-key count for the dashboard; `0` when none.
    #[sqlx(default)]
    pub account_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Connection {
    pub fn is_enabled(&self) -> bool {
        self.enabled != 0
    }

    /// Parses `custom_headers` JSON into a header map. Malformed values yield an empty map.
    pub fn headers(&self) -> BTreeMap<String, String> {
        serde_json::from_str(&self.custom_headers).unwrap_or_default()
    }

    /// Decrypts the stored `api_key` in place.
    pub fn decrypt_api_key(&mut self, cipher: &CredentialCipher) -> Result<()> {
        if let Some(stored) = self.api_key.take() {
            self.api_key = Some(cipher.decrypt(&stored)?);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateConnection {
    pub name: String,
    /// Wire family; filled from `provider_id` when omitted.
    #[serde(default)]
    pub provider_type: String,
    /// Upstream root URL; filled from `provider_id` when omitted.
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub custom_headers: BTreeMap<String, String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub connect_timeout_ms: Option<i64>,
    #[serde(default)]
    pub idle_timeout_ms: Option<i64>,
    #[serde(default)]
    pub pricing_model: Option<String>,
    /// Preset to derive `provider_type`, `base_url` and default headers from.
    /// Filled fields may still be overridden.
    #[serde(default)]
    pub provider_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdateConnection {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub provider_type: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default, deserialize_with = "crate::db::repos::double_option")]
    pub api_key: Option<Option<String>>,
    #[serde(default)]
    pub custom_headers: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default, deserialize_with = "crate::db::repos::double_option")]
    pub connect_timeout_ms: Option<Option<i64>>,
    #[serde(default, deserialize_with = "crate::db::repos::double_option")]
    pub idle_timeout_ms: Option<Option<i64>>,
    #[serde(default, deserialize_with = "crate::db::repos::double_option")]
    pub pricing_model: Option<Option<String>>,
    /// Set or clear the preset this connection is labelled with.
    #[serde(default)]
    pub provider_id: Option<String>,
}

fn default_true() -> bool {
    true
}

/// Rejects provider types the router cannot dispatch to.
pub fn validate_provider_type(provider_type: &str) -> Result<()> {
    if SUPPORTED_PROVIDER_TYPES.contains(&provider_type) {
        Ok(())
    } else {
        Err(Error::UnsupportedProviderType(provider_type.to_string()))
    }
}

pub struct ConnectionRepository {
    pool: SqlitePool,
    cipher: Arc<CredentialCipher>,
}

impl ConnectionRepository {
    pub fn new(pool: SqlitePool, cipher: Arc<CredentialCipher>) -> Self {
        Self { pool, cipher }
    }

    /// Decrypts the `api_key` of a row read from the database.
    fn decrypt(&self, mut connection: Connection) -> Result<Connection> {
        connection.decrypt_api_key(&self.cipher)?;
        Ok(connection)
    }

    pub async fn list(&self) -> Result<Vec<Connection>> {
        let rows = sqlx::query_as::<_, Connection>(
            "SELECT c.*,
                    (SELECT COUNT(*) FROM connection_accounts a
                     WHERE a.connection_id = c.id AND a.enabled = 1) AS account_count
             FROM connections c ORDER BY c.name ASC",
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(|row| self.decrypt(row)).collect()
    }

    pub async fn get(&self, id: &str) -> Result<Option<Connection>> {
        let row = sqlx::query_as::<_, Connection>(
            "SELECT c.*,
                    (SELECT COUNT(*) FROM connection_accounts a
                     WHERE a.connection_id = c.id AND a.enabled = 1) AS account_count
             FROM connections c WHERE c.id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(|row| self.decrypt(row)).transpose()
    }

    pub async fn get_by_name(&self, name: &str) -> Result<Option<Connection>> {
        let row = sqlx::query_as::<_, Connection>(
            "SELECT c.*,
                    (SELECT COUNT(*) FROM connection_accounts a
                     WHERE a.connection_id = c.id AND a.enabled = 1) AS account_count
             FROM connections c WHERE c.name = ?",
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;

        row.map(|row| self.decrypt(row)).transpose()
    }

    pub async fn create(&self, input: CreateConnection) -> Result<Connection> {
        let preset = match input
            .provider_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(id) => Some(
                crate::providers::find(id)
                    .ok_or_else(|| Error::BadRequest(format!("unknown provider preset '{id}'")))?,
            ),
            None => None,
        };

        let provider_type = match input.provider_type.trim() {
            "" => preset
                .as_ref()
                .map(|preset| preset.provider_type.to_string())
                .unwrap_or_default(),
            value => value.to_string(),
        };
        validate_provider_type(&provider_type)?;

        let name = input.name.trim();
        if name.is_empty() {
            return Err(Error::BadRequest("name is required".to_string()));
        }

        let base_url = match input.base_url.trim() {
            "" => preset
                .as_ref()
                .map(|preset| preset.base_url)
                .unwrap_or_default(),
            value => value,
        };
        if base_url.is_empty() {
            return Err(Error::BadRequest("base_url is required".to_string()));
        }
        validate_base_url(base_url)?;

        // Preset headers are defaults: whatever the user sent wins.
        let mut headers = input.custom_headers.clone();
        if let Some(preset) = &preset {
            for (key, value) in &preset.default_headers {
                headers
                    .entry((*key).to_string())
                    .or_insert_with(|| (*value).to_string());
            }
        }

        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();
        let headers = serde_json::to_string(&headers)
            .map_err(|error| Error::BadRequest(format!("invalid custom_headers: {error}")))?;
        let api_key = match normalized_secret(input.api_key.as_deref()) {
            Some(value) => Some(self.cipher.encrypt(&value)?),
            None => None,
        };
        let provider_id = preset.as_ref().map(|preset| preset.id.to_string());

        sqlx::query(
            "INSERT INTO connections
                (id, name, provider_type, base_url, api_key, custom_headers, enabled,
                 connect_timeout_ms, idle_timeout_ms, pricing_model, provider_id,
                 created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(name)
        .bind(&provider_type)
        .bind(base_url)
        .bind(&api_key)
        .bind(headers)
        .bind(i64::from(input.enabled))
        .bind(normalized_timeout(input.connect_timeout_ms)?)
        .bind(normalized_timeout(input.idle_timeout_ms)?)
        .bind(normalized_model(input.pricing_model.as_deref()))
        .bind(&provider_id)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        self.get(&id)
            .await?
            .ok_or_else(|| Error::Internal("connection disappeared after insert".to_string()))
    }

    pub async fn update(&self, id: &str, input: UpdateConnection) -> Result<Connection> {
        let existing = self
            .get(id)
            .await?
            .ok_or_else(|| Error::NotFound(format!("connection '{id}' not found")))?;

        if let Some(provider_type) = &input.provider_type {
            validate_provider_type(provider_type)?;
        }

        let name = input
            .name
            .as_deref()
            .map(str::trim)
            .unwrap_or(&existing.name);
        if name.is_empty() {
            return Err(Error::BadRequest("name is required".to_string()));
        }
        let provider_type = input
            .provider_type
            .as_deref()
            .unwrap_or(&existing.provider_type);
        let base_url = input
            .base_url
            .as_deref()
            .map(str::trim)
            .unwrap_or(&existing.base_url);
        if base_url.is_empty() {
            return Err(Error::BadRequest("base_url is required".to_string()));
        }
        validate_base_url(base_url)?;
        let api_key = match &input.api_key {
            Some(Some(value)) => match normalized_secret(Some(value)) {
                Some(value) => Some(self.cipher.encrypt(&value)?),
                None => None,
            },
            Some(None) => None,
            None => match &existing.api_key {
                // `existing` was decrypted on read; re-encrypt before write.
                Some(value) => Some(self.cipher.encrypt(value)?),
                None => None,
            },
        };
        let headers = match &input.custom_headers {
            Some(map) => serde_json::to_string(map)
                .map_err(|error| Error::BadRequest(format!("invalid custom_headers: {error}")))?,
            None => existing.custom_headers.clone(),
        };
        let enabled = input.enabled.unwrap_or(existing.is_enabled());
        let connect_timeout = match &input.connect_timeout_ms {
            Some(value) => normalized_timeout(*value)?,
            None => existing.connect_timeout_ms,
        };
        let idle_timeout = match &input.idle_timeout_ms {
            Some(value) => normalized_timeout(*value)?,
            None => existing.idle_timeout_ms,
        };
        let pricing_model = match &input.pricing_model {
            Some(value) => normalized_model(value.as_deref()),
            None => existing.pricing_model.clone(),
        };
        let provider_id = match input.provider_id.as_deref().map(str::trim) {
            Some("") => None,
            Some(id) => {
                crate::providers::find(id)
                    .ok_or_else(|| Error::BadRequest(format!("unknown provider preset '{id}'")))?;
                Some(id.to_string())
            }
            None => existing.provider_id.clone(),
        };

        sqlx::query(
            "UPDATE connections
             SET name = ?, provider_type = ?, base_url = ?, api_key = ?, custom_headers = ?, enabled = ?,
                 connect_timeout_ms = ?, idle_timeout_ms = ?, pricing_model = ?, provider_id = ?,
                 updated_at = ?
             WHERE id = ?",
        )
        .bind(name)
        .bind(provider_type)
        .bind(base_url)
        .bind(&api_key)
        .bind(headers)
        .bind(i64::from(enabled))
        .bind(connect_timeout)
        .bind(idle_timeout)
        .bind(pricing_model)
        .bind(&provider_id)
        .bind(Utc::now())
        .bind(id)
        .execute(&self.pool)
        .await?;

        self.get(id)
            .await?
            .ok_or_else(|| Error::Internal("connection disappeared after update".to_string()))
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM connections WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}

/// Rejects a base URL that still carries a preset placeholder (`<account-id>`,
/// `<your-resource>`), which would fail at request time instead of at save time.
fn validate_base_url(base_url: &str) -> Result<()> {
    if base_url.contains('<') || base_url.contains('>') {
        return Err(Error::BadRequest(
            "base_url still contains a placeholder; replace it with your own value".to_string(),
        ));
    }
    Ok(())
}

/// Trims a secret and treats blanks as absent.
fn normalized_secret(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// Trims a model id and treats blanks as absent.
fn normalized_model(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// Rejects negative timeouts; zero disables the timeout explicitly.
fn normalized_timeout(value: Option<i64>) -> Result<Option<i64>> {
    match value {
        None => Ok(None),
        Some(value) if value < 0 => Err(Error::BadRequest(
            "timeouts must be zero or positive milliseconds".to_string(),
        )),
        Some(value) => Ok(Some(value)),
    }
}
