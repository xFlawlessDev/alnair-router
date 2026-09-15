//! Aliases: model prefix → connection mappings.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Alias {
    pub id: String,
    pub prefix: String,
    pub connection_id: String,
    pub model_override: Option<String>,
    pub enabled: i64,
    pub sort_order: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Alias {
    pub fn is_enabled(&self) -> bool {
        self.enabled != 0
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateAlias {
    pub prefix: String,
    pub connection_id: String,
    #[serde(default)]
    pub model_override: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub sort_order: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdateAlias {
    #[serde(default)]
    pub prefix: Option<String>,
    #[serde(default)]
    pub connection_id: Option<String>,
    #[serde(default)]
    pub model_override: Option<Option<String>>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub sort_order: Option<i64>,
}

fn default_true() -> bool {
    true
}

/// Normalizes a prefix: trimmed, lowercase, no trailing slash.
pub fn normalize_prefix(prefix: &str) -> String {
    prefix.trim().trim_end_matches('/').to_ascii_lowercase()
}

/// Validates a prefix shape (no whitespace, no slashes, non-empty).
pub fn validate_prefix(prefix: &str) -> Result<String> {
    let normalized = normalize_prefix(prefix);
    if normalized.is_empty() {
        return Err(Error::BadRequest("prefix is required".to_string()));
    }
    if normalized.contains('/') || normalized.contains(char::is_whitespace) {
        return Err(Error::BadRequest(format!(
            "prefix must not contain '/' or whitespace: {normalized}"
        )));
    }
    Ok(normalized)
}

pub struct AliasRepository {
    pool: SqlitePool,
}

impl AliasRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn list(&self) -> Result<Vec<Alias>> {
        let rows =
            sqlx::query_as::<_, Alias>("SELECT * FROM aliases ORDER BY sort_order ASC, prefix ASC")
                .fetch_all(&self.pool)
                .await?;
        Ok(rows)
    }

    pub async fn get(&self, id: &str) -> Result<Option<Alias>> {
        let row = sqlx::query_as::<_, Alias>("SELECT * FROM aliases WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row)
    }

    pub async fn get_by_prefix(&self, prefix: &str) -> Result<Option<Alias>> {
        let row = sqlx::query_as::<_, Alias>("SELECT * FROM aliases WHERE prefix = ?")
            .bind(normalize_prefix(prefix))
            .fetch_optional(&self.pool)
            .await?;
        Ok(row)
    }

    pub async fn create(&self, input: CreateAlias) -> Result<Alias> {
        let prefix = validate_prefix(&input.prefix)?;
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();
        let model_override = input
            .model_override
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string);

        sqlx::query(
            "INSERT INTO aliases (id, prefix, connection_id, model_override, enabled, sort_order, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&prefix)
        .bind(&input.connection_id)
        .bind(&model_override)
        .bind(i64::from(input.enabled))
        .bind(input.sort_order)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        self.get(&id)
            .await?
            .ok_or_else(|| Error::Internal("alias disappeared after insert".to_string()))
    }

    pub async fn update(&self, id: &str, input: UpdateAlias) -> Result<Alias> {
        let existing = self
            .get(id)
            .await?
            .ok_or_else(|| Error::NotFound(format!("alias '{id}' not found")))?;

        let prefix = match &input.prefix {
            Some(value) => validate_prefix(value)?,
            None => existing.prefix.clone(),
        };
        let connection_id = input
            .connection_id
            .as_deref()
            .unwrap_or(&existing.connection_id);
        let model_override = match &input.model_override {
            Some(value) => value
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_string),
            None => existing.model_override.clone(),
        };
        let enabled = input.enabled.unwrap_or(existing.is_enabled());
        let sort_order = input.sort_order.unwrap_or(existing.sort_order);

        sqlx::query(
            "UPDATE aliases
             SET prefix = ?, connection_id = ?, model_override = ?, enabled = ?, sort_order = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(prefix)
        .bind(connection_id)
        .bind(&model_override)
        .bind(i64::from(enabled))
        .bind(sort_order)
        .bind(Utc::now())
        .bind(id)
        .execute(&self.pool)
        .await?;

        self.get(id)
            .await?
            .ok_or_else(|| Error::Internal("alias disappeared after update".to_string()))
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM aliases WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}
