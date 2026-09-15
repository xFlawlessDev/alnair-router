//! Reusable key plans: one rule set (model allowlist + limits) applied to many keys.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::types::Json;
use sqlx::{FromRow, SqlitePool};

use crate::db::repos::api_keys::{
    normalize_allowed_models, normalized_budget, normalized_budget_mode, normalized_rate_limit,
    validate_budget_pair,
};
use crate::error::{Error, Result};
use crate::limits::BudgetMode;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct KeyPlan {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Model patterns; an empty list allows any model.
    pub allowed_models: Json<Vec<String>>,
    pub rate_limit_per_minute: Option<i64>,
    pub monthly_budget_usd: Option<f64>,
    /// `off`, `warn`, or `block`.
    pub budget_mode: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl KeyPlan {
    /// Effective rate override, ignoring non-positive values.
    pub fn rate_limit(&self) -> Option<u32> {
        self.rate_limit_per_minute
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value > 0)
    }

    /// Effective budget, ignoring non-positive values.
    pub fn budget_usd(&self) -> Option<f64> {
        self.monthly_budget_usd.filter(|value| *value > 0.0)
    }

    /// Parsed budget mode; malformed stored values degrade to `off`.
    pub fn budget_mode(&self) -> BudgetMode {
        BudgetMode::parse(&self.budget_mode).unwrap_or_default()
    }

    /// Model patterns stored on the plan.
    pub fn allowed_models(&self) -> &[String] {
        &self.allowed_models.0
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateKeyPlan {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub allowed_models: Vec<String>,
    #[serde(default)]
    pub rate_limit_per_minute: Option<i64>,
    #[serde(default)]
    pub monthly_budget_usd: Option<f64>,
    #[serde(default)]
    pub budget_mode: Option<String>,
}

/// Partial update. `null` clears nullable fields; absence leaves them unchanged.
/// An empty `allowed_models` list clears the allowlist.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdateKeyPlan {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub allowed_models: Option<Vec<String>>,
    #[serde(default, deserialize_with = "crate::db::repos::double_option")]
    pub rate_limit_per_minute: Option<Option<i64>>,
    #[serde(default, deserialize_with = "crate::db::repos::double_option")]
    pub monthly_budget_usd: Option<Option<f64>>,
    #[serde(default)]
    pub budget_mode: Option<String>,
}

pub struct KeyPlanRepository {
    pool: SqlitePool,
}

impl KeyPlanRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn list(&self) -> Result<Vec<KeyPlan>> {
        let rows = sqlx::query_as::<_, KeyPlan>("SELECT * FROM key_plans ORDER BY name")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows)
    }

    pub async fn get(&self, id: &str) -> Result<Option<KeyPlan>> {
        let row = sqlx::query_as::<_, KeyPlan>("SELECT * FROM key_plans WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row)
    }

    pub async fn create(&self, input: CreateKeyPlan) -> Result<KeyPlan> {
        let name = input.name.trim();
        if name.is_empty() {
            return Err(Error::BadRequest("name is required".to_string()));
        }
        self.ensure_name_free(name, None).await?;

        let rate_limit = normalized_rate_limit(input.rate_limit_per_minute)?;
        let budget = normalized_budget(input.monthly_budget_usd)?;
        let budget_mode = normalized_budget_mode(input.budget_mode.as_deref())?;
        validate_budget_pair(budget, budget_mode)?;
        let allowed_models =
            normalize_allowed_models(Some(input.allowed_models))?.unwrap_or_default();

        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO key_plans
                (id, name, description, allowed_models, rate_limit_per_minute, monthly_budget_usd, budget_mode, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(name)
        .bind(input.description.trim())
        .bind(Json(&allowed_models))
        .bind(rate_limit)
        .bind(budget)
        .bind(budget_mode.as_str())
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        self.get(&id)
            .await?
            .ok_or_else(|| Error::Internal("plan disappeared after insert".to_string()))
    }

    /// Applies a partial update, leaving absent fields unchanged.
    pub async fn update(&self, id: &str, input: UpdateKeyPlan) -> Result<KeyPlan> {
        let existing = self
            .get(id)
            .await?
            .ok_or_else(|| Error::NotFound(format!("plan '{id}' not found")))?;

        let name = input
            .name
            .as_deref()
            .map(str::trim)
            .unwrap_or(&existing.name);
        if name.is_empty() {
            return Err(Error::BadRequest("name is required".to_string()));
        }
        self.ensure_name_free(name, Some(id)).await?;

        let description = match &input.description {
            Some(value) => value.trim().to_string(),
            None => existing.description.clone(),
        };
        let allowed_models = match &input.allowed_models {
            Some(value) => normalize_allowed_models(Some(value.clone()))?.unwrap_or_default(),
            None => existing.allowed_models.0.clone(),
        };
        let rate_limit = match &input.rate_limit_per_minute {
            Some(value) => normalized_rate_limit(*value)?,
            None => existing.rate_limit_per_minute,
        };
        let budget = match &input.monthly_budget_usd {
            Some(value) => normalized_budget(*value)?,
            None => existing.monthly_budget_usd,
        };
        let budget_mode = match &input.budget_mode {
            Some(value) => normalized_budget_mode(Some(value))?,
            None => existing.budget_mode(),
        };
        validate_budget_pair(budget, budget_mode)?;

        sqlx::query(
            "UPDATE key_plans
             SET name = ?, description = ?, allowed_models = ?, rate_limit_per_minute = ?, monthly_budget_usd = ?, budget_mode = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(name)
        .bind(description)
        .bind(Json(&allowed_models))
        .bind(rate_limit)
        .bind(budget)
        .bind(budget_mode.as_str())
        .bind(Utc::now())
        .bind(id)
        .execute(&self.pool)
        .await?;

        self.get(id)
            .await?
            .ok_or_else(|| Error::Internal("plan disappeared after update".to_string()))
    }

    /// Deletes a plan, detaching it from every key first.
    pub async fn delete(&self, id: &str) -> Result<bool> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query("UPDATE api_keys SET plan_id = NULL WHERE plan_id = ?")
            .bind(id)
            .execute(&mut *transaction)
            .await?;
        let result = sqlx::query("DELETE FROM key_plans WHERE id = ?")
            .bind(id)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;

        Ok(result.rows_affected() > 0)
    }

    /// Rejects duplicate plan names up front so the API returns a 400, not a
    /// database constraint error.
    async fn ensure_name_free(&self, name: &str, skip_id: Option<&str>) -> Result<()> {
        let existing: Option<(String,)> = sqlx::query_as("SELECT id FROM key_plans WHERE name = ?")
            .bind(name)
            .fetch_optional(&self.pool)
            .await?;

        match existing {
            Some((id,)) if skip_id != Some(id.as_str()) => Err(Error::BadRequest(format!(
                "a plan named '{name}' already exists"
            ))),
            _ => Ok(()),
        }
    }
}
