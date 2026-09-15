//! Reusable key plans: one rule set (model allowlist + limits) applied to many keys.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::types::Json;
use sqlx::{FromRow, SqlitePool};

use crate::db::repos::api_keys::{
    normalize_allowed_models, normalized_budget, normalized_budget_mode, normalized_rate_limit,
    normalized_token_limit, validate_budget_pair,
};
use crate::error::{Error, Result};
use crate::limits::{BudgetMode, BudgetWindows, TokenWindows};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct KeyPlan {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Model patterns; an empty list allows any model.
    pub allowed_models: Json<Vec<String>>,
    pub rate_limit_per_minute: Option<i64>,
    /// Daily spend cap in USD; `None` is uncapped.
    pub daily_budget_usd: Option<f64>,
    /// Weekly spend cap in USD; `None` is uncapped.
    pub weekly_budget_usd: Option<f64>,
    /// Monthly spend cap in USD; `None` is uncapped.
    pub monthly_budget_usd: Option<f64>,
    /// Lifetime spend cap in USD with no reset; `None` is uncapped.
    pub lifetime_budget_usd: Option<f64>,
    /// Daily token cap (prompt + completion); `None` is uncapped.
    pub daily_token_limit: Option<i64>,
    /// Weekly token cap (prompt + completion); `None` is uncapped.
    pub weekly_token_limit: Option<i64>,
    /// Monthly token cap (prompt + completion); `None` is uncapped.
    pub monthly_token_limit: Option<i64>,
    /// Lifetime token cap (prompt + completion); `None` is uncapped.
    pub lifetime_token_limit: Option<i64>,
    /// `off`, `warn`, or `block`.
    pub budget_mode: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// When the plan stops applying; keys attached to it are rejected.
    pub expires_at: Option<DateTime<Utc>>,
}

impl KeyPlan {
    /// Effective rate override, ignoring non-positive values.
    pub fn rate_limit(&self) -> Option<u32> {
        self.rate_limit_per_minute
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value > 0)
    }

    /// Effective budget caps, ignoring non-positive values.
    pub fn budget_windows(&self) -> BudgetWindows {
        BudgetWindows::from_parts(
            self.daily_budget_usd,
            self.weekly_budget_usd,
            self.monthly_budget_usd,
            self.lifetime_budget_usd,
        )
    }

    /// Effective token caps, ignoring non-positive values.
    pub fn token_windows(&self) -> TokenWindows {
        TokenWindows::from_parts(
            self.daily_token_limit,
            self.weekly_token_limit,
            self.monthly_token_limit,
            self.lifetime_token_limit,
        )
    }

    /// Parsed budget mode; malformed stored values degrade to `off`.
    pub fn budget_mode(&self) -> BudgetMode {
        BudgetMode::parse(&self.budget_mode).unwrap_or_default()
    }

    /// True when the plan is past its expiry; a plan without one never expires.
    pub fn is_expired(&self) -> bool {
        self.expires_at.is_some_and(|at| at <= Utc::now())
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
    pub daily_budget_usd: Option<f64>,
    #[serde(default)]
    pub weekly_budget_usd: Option<f64>,
    #[serde(default)]
    pub monthly_budget_usd: Option<f64>,
    #[serde(default)]
    pub lifetime_budget_usd: Option<f64>,
    #[serde(default)]
    pub daily_token_limit: Option<i64>,
    #[serde(default)]
    pub weekly_token_limit: Option<i64>,
    #[serde(default)]
    pub monthly_token_limit: Option<i64>,
    #[serde(default)]
    pub lifetime_token_limit: Option<i64>,
    #[serde(default)]
    pub budget_mode: Option<String>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
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
    pub daily_budget_usd: Option<Option<f64>>,
    #[serde(default, deserialize_with = "crate::db::repos::double_option")]
    pub weekly_budget_usd: Option<Option<f64>>,
    #[serde(default, deserialize_with = "crate::db::repos::double_option")]
    pub monthly_budget_usd: Option<Option<f64>>,
    #[serde(default, deserialize_with = "crate::db::repos::double_option")]
    pub lifetime_budget_usd: Option<Option<f64>>,
    #[serde(default, deserialize_with = "crate::db::repos::double_option")]
    pub daily_token_limit: Option<Option<i64>>,
    #[serde(default, deserialize_with = "crate::db::repos::double_option")]
    pub weekly_token_limit: Option<Option<i64>>,
    #[serde(default, deserialize_with = "crate::db::repos::double_option")]
    pub monthly_token_limit: Option<Option<i64>>,
    #[serde(default, deserialize_with = "crate::db::repos::double_option")]
    pub lifetime_token_limit: Option<Option<i64>>,
    #[serde(default)]
    pub budget_mode: Option<String>,
    #[serde(default, deserialize_with = "crate::db::repos::double_option")]
    pub expires_at: Option<Option<DateTime<Utc>>>,
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
        let budgets = BudgetWindows::from_parts(
            normalized_budget(input.daily_budget_usd, "daily_budget_usd")?,
            normalized_budget(input.weekly_budget_usd, "weekly_budget_usd")?,
            normalized_budget(input.monthly_budget_usd, "monthly_budget_usd")?,
            normalized_budget(input.lifetime_budget_usd, "lifetime_budget_usd")?,
        );
        let tokens = TokenWindows::from_parts(
            normalized_token_limit(input.daily_token_limit, "daily_token_limit")?,
            normalized_token_limit(input.weekly_token_limit, "weekly_token_limit")?,
            normalized_token_limit(input.monthly_token_limit, "monthly_token_limit")?,
            normalized_token_limit(input.lifetime_token_limit, "lifetime_token_limit")?,
        );
        let budget_mode = normalized_budget_mode(input.budget_mode.as_deref())?;
        validate_budget_pair(&budgets, &tokens, budget_mode)?;
        let allowed_models =
            normalize_allowed_models(Some(input.allowed_models))?.unwrap_or_default();

        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO key_plans
                (id, name, description, allowed_models, rate_limit_per_minute, daily_budget_usd,
                 weekly_budget_usd, monthly_budget_usd, lifetime_budget_usd, daily_token_limit,
                 weekly_token_limit, monthly_token_limit, lifetime_token_limit, budget_mode,
                 created_at, updated_at, expires_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(name)
        .bind(input.description.trim())
        .bind(Json(&allowed_models))
        .bind(rate_limit)
        .bind(budgets.daily)
        .bind(budgets.weekly)
        .bind(budgets.monthly)
        .bind(budgets.lifetime)
        .bind(tokens.daily)
        .bind(tokens.weekly)
        .bind(tokens.monthly)
        .bind(tokens.lifetime)
        .bind(budget_mode.as_str())
        .bind(now)
        .bind(now)
        .bind(input.expires_at)
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
        let budgets = BudgetWindows::from_parts(
            match &input.daily_budget_usd {
                Some(value) => normalized_budget(*value, "daily_budget_usd")?,
                None => existing.daily_budget_usd,
            },
            match &input.weekly_budget_usd {
                Some(value) => normalized_budget(*value, "weekly_budget_usd")?,
                None => existing.weekly_budget_usd,
            },
            match &input.monthly_budget_usd {
                Some(value) => normalized_budget(*value, "monthly_budget_usd")?,
                None => existing.monthly_budget_usd,
            },
            match &input.lifetime_budget_usd {
                Some(value) => normalized_budget(*value, "lifetime_budget_usd")?,
                None => existing.lifetime_budget_usd,
            },
        );
        let tokens = TokenWindows::from_parts(
            match &input.daily_token_limit {
                Some(value) => normalized_token_limit(*value, "daily_token_limit")?,
                None => existing.daily_token_limit,
            },
            match &input.weekly_token_limit {
                Some(value) => normalized_token_limit(*value, "weekly_token_limit")?,
                None => existing.weekly_token_limit,
            },
            match &input.monthly_token_limit {
                Some(value) => normalized_token_limit(*value, "monthly_token_limit")?,
                None => existing.monthly_token_limit,
            },
            match &input.lifetime_token_limit {
                Some(value) => normalized_token_limit(*value, "lifetime_token_limit")?,
                None => existing.lifetime_token_limit,
            },
        );
        let budget_mode = match &input.budget_mode {
            Some(value) => normalized_budget_mode(Some(value))?,
            None => existing.budget_mode(),
        };
        validate_budget_pair(&budgets, &tokens, budget_mode)?;
        let expires_at = match &input.expires_at {
            Some(value) => *value,
            None => existing.expires_at,
        };

        sqlx::query(
            "UPDATE key_plans
             SET name = ?, description = ?, allowed_models = ?, rate_limit_per_minute = ?,
                 daily_budget_usd = ?, weekly_budget_usd = ?, monthly_budget_usd = ?,
                 lifetime_budget_usd = ?, daily_token_limit = ?, weekly_token_limit = ?,
                 monthly_token_limit = ?, lifetime_token_limit = ?, budget_mode = ?,
                 expires_at = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(name)
        .bind(description)
        .bind(Json(&allowed_models))
        .bind(rate_limit)
        .bind(budgets.daily)
        .bind(budgets.weekly)
        .bind(budgets.monthly)
        .bind(budgets.lifetime)
        .bind(tokens.daily)
        .bind(tokens.weekly)
        .bind(tokens.monthly)
        .bind(tokens.lifetime)
        .bind(budget_mode.as_str())
        .bind(expires_at)
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
