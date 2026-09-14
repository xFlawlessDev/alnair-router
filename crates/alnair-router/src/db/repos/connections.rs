//! Upstream provider endpoint records.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::error::{Error, Result};

/// The two upstream families supported by the router.
pub const SUPPORTED_PROVIDER_TYPES: [&str; 2] = ["openai-compatible", "anthropic-native"];

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Connection {
    pub id: String,
    pub name: String,
    pub provider_type: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub custom_headers: String,
    pub enabled: i64,
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
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateConnection {
    pub name: String,
    pub provider_type: String,
    pub base_url: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub custom_headers: BTreeMap<String, String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdateConnection {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub provider_type: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<Option<String>>,
    #[serde(default)]
    pub custom_headers: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub enabled: Option<bool>,
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
}

impl ConnectionRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn list(&self) -> Result<Vec<Connection>> {
        let rows = sqlx::query_as::<_, Connection>(
            "SELECT * FROM connections ORDER BY name ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn get(&self, id: &str) -> Result<Option<Connection>> {
        let row = sqlx::query_as::<_, Connection>("SELECT * FROM connections WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row)
    }

    pub async fn get_by_name(&self, name: &str) -> Result<Option<Connection>> {
        let row = sqlx::query_as::<_, Connection>("SELECT * FROM connections WHERE name = ?")
            .bind(name)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row)
    }

    pub async fn create(&self, input: CreateConnection) -> Result<Connection> {
        validate_provider_type(&input.provider_type)?;
        let name = input.name.trim();
        if name.is_empty() {
            return Err(Error::BadRequest("name is required".to_string()));
        }
        if input.base_url.trim().is_empty() {
            return Err(Error::BadRequest("base_url is required".to_string()));
        }

        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();
        let headers = serde_json::to_string(&input.custom_headers)
            .map_err(|error| Error::BadRequest(format!("invalid custom_headers: {error}")))?;

        sqlx::query(
            "INSERT INTO connections (id, name, provider_type, base_url, api_key, custom_headers, enabled, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(name)
        .bind(&input.provider_type)
        .bind(input.base_url.trim())
        .bind(&input.api_key)
        .bind(headers)
        .bind(i64::from(input.enabled))
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

        let name = input.name.as_deref().map(str::trim).unwrap_or(&existing.name);
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
        let api_key = match &input.api_key {
            Some(value) => value.clone(),
            None => existing.api_key.clone(),
        };
        let headers = match &input.custom_headers {
            Some(map) => serde_json::to_string(map)
                .map_err(|error| Error::BadRequest(format!("invalid custom_headers: {error}")))?,
            None => existing.custom_headers.clone(),
        };
        let enabled = input.enabled.unwrap_or(existing.is_enabled());

        sqlx::query(
            "UPDATE connections
             SET name = ?, provider_type = ?, base_url = ?, api_key = ?, custom_headers = ?, enabled = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(name)
        .bind(provider_type)
        .bind(base_url)
        .bind(&api_key)
        .bind(headers)
        .bind(i64::from(enabled))
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
