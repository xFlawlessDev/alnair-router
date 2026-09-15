//! Combos: named, ordered fallback chains over resolvable model references.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Combo {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub enabled: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Combo {
    pub fn is_enabled(&self) -> bool {
        self.enabled != 0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ComboEntry {
    pub id: String,
    pub combo_id: String,
    pub model_ref: String,
    pub position: i64,
    pub enabled: i64,
}

impl ComboEntry {
    pub fn is_enabled(&self) -> bool {
        self.enabled != 0
    }
}

/// A combo together with its ordered entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComboWithEntries {
    pub combo: Combo,
    pub entries: Vec<ComboEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateCombo {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Ordered model references. Order defines the fallback tier order.
    #[serde(default)]
    pub entries: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdateCombo {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "crate::db::repos::double_option")]
    pub description: Option<Option<String>>,
    #[serde(default)]
    pub enabled: Option<bool>,
    /// When present, replaces all entries wholesale in the given order.
    #[serde(default)]
    pub entries: Option<Vec<String>>,
}

fn default_true() -> bool {
    true
}

/// Normalizes a combo name for lookup.
pub fn normalize_combo_name(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

pub struct ComboRepository {
    pool: SqlitePool,
}

impl ComboRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn list(&self) -> Result<Vec<Combo>> {
        let rows = sqlx::query_as::<_, Combo>("SELECT * FROM combos ORDER BY name ASC")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows)
    }

    pub async fn get(&self, id: &str) -> Result<Option<Combo>> {
        let row = sqlx::query_as::<_, Combo>("SELECT * FROM combos WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row)
    }

    pub async fn get_by_name(&self, name: &str) -> Result<Option<Combo>> {
        let row = sqlx::query_as::<_, Combo>("SELECT * FROM combos WHERE name = ?")
            .bind(normalize_combo_name(name))
            .fetch_optional(&self.pool)
            .await?;
        Ok(row)
    }

    pub async fn entries(&self, combo_id: &str) -> Result<Vec<ComboEntry>> {
        let rows = sqlx::query_as::<_, ComboEntry>(
            "SELECT * FROM combo_entries WHERE combo_id = ? ORDER BY position ASC",
        )
        .bind(combo_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn get_with_entries(&self, id: &str) -> Result<Option<ComboWithEntries>> {
        let Some(combo) = self.get(id).await? else {
            return Ok(None);
        };
        let entries = self.entries(id).await?;
        Ok(Some(ComboWithEntries { combo, entries }))
    }

    pub async fn get_by_name_with_entries(&self, name: &str) -> Result<Option<ComboWithEntries>> {
        let Some(combo) = self.get_by_name(name).await? else {
            return Ok(None);
        };
        let entries = self.entries(&combo.id).await?;
        Ok(Some(ComboWithEntries { combo, entries }))
    }

    pub async fn create(&self, input: CreateCombo) -> Result<ComboWithEntries> {
        let name = normalize_combo_name(&input.name);
        if name.is_empty() {
            return Err(Error::BadRequest("name is required".to_string()));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();

        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO combos (id, name, description, enabled, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&name)
        .bind(&input.description)
        .bind(i64::from(input.enabled))
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await?;

        for (position, model_ref) in input.entries.iter().enumerate() {
            let trimmed = model_ref.trim();
            if trimmed.is_empty() {
                continue;
            }
            sqlx::query(
                "INSERT INTO combo_entries (id, combo_id, model_ref, position, enabled)
                 VALUES (?, ?, ?, ?, 1)",
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(&id)
            .bind(trimmed)
            .bind(position as i64)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;

        self.get_with_entries(&id)
            .await?
            .ok_or_else(|| Error::Internal("combo disappeared after insert".to_string()))
    }

    pub async fn update(&self, id: &str, input: UpdateCombo) -> Result<ComboWithEntries> {
        let existing = self
            .get(id)
            .await?
            .ok_or_else(|| Error::NotFound(format!("combo '{id}' not found")))?;

        let name = match &input.name {
            Some(value) => {
                let normalized = normalize_combo_name(value);
                if normalized.is_empty() {
                    return Err(Error::BadRequest("name is required".to_string()));
                }
                normalized
            }
            None => existing.name.clone(),
        };
        let description = match &input.description {
            Some(value) => value.clone(),
            None => existing.description.clone(),
        };
        let enabled = input.enabled.unwrap_or(existing.is_enabled());

        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "UPDATE combos SET name = ?, description = ?, enabled = ?, updated_at = ? WHERE id = ?",
        )
        .bind(name)
        .bind(&description)
        .bind(i64::from(enabled))
        .bind(Utc::now())
        .bind(id)
        .execute(&mut *tx)
        .await?;

        if let Some(entries) = &input.entries {
            sqlx::query("DELETE FROM combo_entries WHERE combo_id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            for (position, model_ref) in entries.iter().enumerate() {
                let trimmed = model_ref.trim();
                if trimmed.is_empty() {
                    continue;
                }
                sqlx::query(
                    "INSERT INTO combo_entries (id, combo_id, model_ref, position, enabled)
                     VALUES (?, ?, ?, ?, 1)",
                )
                .bind(uuid::Uuid::new_v4().to_string())
                .bind(id)
                .bind(trimmed)
                .bind(position as i64)
                .execute(&mut *tx)
                .await?;
            }
        }
        tx.commit().await?;

        self.get_with_entries(id)
            .await?
            .ok_or_else(|| Error::Internal("combo disappeared after update".to_string()))
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM combos WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}
