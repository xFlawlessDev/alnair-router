//! Router-issued client API keys keyed.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::types::Json;
use sqlx::{FromRow, SqlitePool};

use crate::error::{Error, Result};
use crate::limits::BudgetMode;

const KEY_PREFIX: &str = "sk-router-";

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ApiKey {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing)]
    pub key_hash: String,
    pub prefix: String,
    pub enabled: i64,
    /// Per-key requests-per-minute override; `None` inherits the global default.
    pub rate_limit_per_minute: Option<i64>,
    /// Monthly spend cap in USD; `None` is uncapped.
    pub monthly_budget_usd: Option<f64>,
    /// `off`, `warn`, or `block`.
    pub budget_mode: String,
    /// Optional plan whose rules fill the fields this key leaves empty.
    pub plan_id: Option<String>,
    /// Model allowlist patterns; `None` inherits the plan (or allows any model).
    pub allowed_models: Option<Json<Vec<String>>>,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

impl ApiKey {
    pub fn is_enabled(&self) -> bool {
        self.enabled != 0
    }

    /// Model patterns stored on the key itself, if any.
    pub fn allowed_models(&self) -> Option<&[String]> {
        self.allowed_models
            .as_ref()
            .map(|models| models.0.as_slice())
    }

    /// Effective per-key rate override, ignoring non-positive values.
    pub fn rate_limit(&self) -> Option<u32> {
        self.rate_limit_per_minute
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value > 0)
    }

    /// Parsed budget mode; malformed stored values degrade to `off`.
    pub fn budget_mode(&self) -> BudgetMode {
        BudgetMode::parse(&self.budget_mode).unwrap_or_default()
    }

    /// Effective budget, ignoring non-positive values.
    pub fn budget_usd(&self) -> Option<f64> {
        self.monthly_budget_usd.filter(|value| *value > 0.0)
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
    #[serde(default)]
    pub rate_limit_per_minute: Option<i64>,
    #[serde(default)]
    pub monthly_budget_usd: Option<f64>,
    #[serde(default)]
    pub budget_mode: Option<String>,
    #[serde(default)]
    pub plan_id: Option<String>,
    #[serde(default)]
    pub allowed_models: Option<Vec<String>>,
}

/// Partial update. `null` clears nullable fields; absence leaves them unchanged.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdateApiKey {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default, deserialize_with = "crate::db::repos::double_option")]
    pub rate_limit_per_minute: Option<Option<i64>>,
    #[serde(default, deserialize_with = "crate::db::repos::double_option")]
    pub monthly_budget_usd: Option<Option<f64>>,
    #[serde(default)]
    pub budget_mode: Option<String>,
    #[serde(default, deserialize_with = "crate::db::repos::double_option")]
    pub plan_id: Option<Option<String>>,
    #[serde(default, deserialize_with = "crate::db::repos::double_option")]
    pub allowed_models: Option<Option<Vec<String>>>,
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

pub(crate) fn normalized_rate_limit(value: Option<i64>) -> Result<Option<i64>> {
    match value {
        None => Ok(None),
        Some(value) if value <= 0 => Err(Error::BadRequest(
            "rate_limit_per_minute must be positive".to_string(),
        )),
        Some(value) => Ok(Some(value)),
    }
}

pub(crate) fn normalized_budget(value: Option<f64>) -> Result<Option<f64>> {
    match value {
        None => Ok(None),
        Some(value) if !value.is_finite() || value <= 0.0 => Err(Error::BadRequest(
            "monthly_budget_usd must be a positive number".to_string(),
        )),
        Some(value) => Ok(Some(value)),
    }
}

pub(crate) fn normalized_budget_mode(value: Option<&str>) -> Result<BudgetMode> {
    match value {
        Some(value) => BudgetMode::parse(value),
        None => Ok(BudgetMode::Off),
    }
}

pub(crate) fn validate_budget_pair(budget: Option<f64>, mode: BudgetMode) -> Result<()> {
    if mode != BudgetMode::Off && budget.is_none() {
        return Err(Error::BadRequest(
            "monthly_budget_usd is required when budget_mode is warn or block".to_string(),
        ));
    }
    Ok(())
}

/// Trims, drops blanks and de-dupes model patterns; an empty result clears the
/// allowlist entirely (`None` means "any model").
pub(crate) fn normalize_allowed_models(value: Option<Vec<String>>) -> Result<Option<Vec<String>>> {
    let Some(models) = value else {
        return Ok(None);
    };

    let mut normalized: Vec<String> = Vec::new();
    for model in models {
        let model = model.trim();
        if model.is_empty() || normalized.iter().any(|existing| existing == model) {
            continue;
        }
        normalized.push(model.to_string());
    }

    Ok((!normalized.is_empty()).then_some(normalized))
}

/// Rejects a blank plan id and verifies the plan exists.
pub(crate) async fn validate_plan(
    pool: &SqlitePool,
    plan_id: Option<&str>,
) -> Result<Option<String>> {
    let Some(plan_id) = plan_id else {
        return Ok(None);
    };

    let plan_id = plan_id.trim();
    if plan_id.is_empty() {
        return Err(Error::BadRequest("plan_id must not be blank".to_string()));
    }

    let exists: Option<(String,)> = sqlx::query_as("SELECT id FROM key_plans WHERE id = ?")
        .bind(plan_id)
        .fetch_optional(pool)
        .await?;
    if exists.is_none() {
        return Err(Error::NotFound(format!("plan '{plan_id}' not found")));
    }

    Ok(Some(plan_id.to_string()))
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

    pub async fn get(&self, id: &str) -> Result<Option<ApiKey>> {
        let row = sqlx::query_as::<_, ApiKey>("SELECT * FROM api_keys WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row)
    }

    pub async fn create(&self, input: CreateApiKey) -> Result<CreatedApiKey> {
        let name = input.name.trim();
        if name.is_empty() {
            return Err(Error::BadRequest("name is required".to_string()));
        }

        let rate_limit = normalized_rate_limit(input.rate_limit_per_minute)?;
        let budget = normalized_budget(input.monthly_budget_usd)?;
        let budget_mode = normalized_budget_mode(input.budget_mode.as_deref())?;
        validate_budget_pair(budget, budget_mode)?;
        let plan_id = validate_plan(&self.pool, input.plan_id.as_deref()).await?;
        let allowed_models = normalize_allowed_models(input.allowed_models)?;

        let secret = format!("{KEY_PREFIX}{}", uuid::Uuid::new_v4().simple());
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();
        let prefix: String = secret.chars().take(KEY_PREFIX.len() + 8).collect();

        sqlx::query(
            "INSERT INTO api_keys
                (id, name, key_hash, prefix, enabled, rate_limit_per_minute, monthly_budget_usd, budget_mode, plan_id, allowed_models, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(name)
        .bind(hash_key(&secret))
        .bind(&prefix)
        .bind(i64::from(input.enabled))
        .bind(rate_limit)
        .bind(budget)
        .bind(budget_mode.as_str())
        .bind(plan_id)
        .bind(allowed_models.map(Json))
        .bind(now)
        .execute(&self.pool)
        .await?;

        let key = sqlx::query_as::<_, ApiKey>("SELECT * FROM api_keys WHERE id = ?")
            .bind(&id)
            .fetch_one(&self.pool)
            .await?;

        Ok(CreatedApiKey { key, secret })
    }

    /// Applies a partial update, leaving absent fields unchanged.
    pub async fn update(&self, id: &str, input: UpdateApiKey) -> Result<ApiKey> {
        let existing = self
            .get(id)
            .await?
            .ok_or_else(|| Error::NotFound(format!("api key '{id}' not found")))?;

        let name = input
            .name
            .as_deref()
            .map(str::trim)
            .unwrap_or(&existing.name);
        if name.is_empty() {
            return Err(Error::BadRequest("name is required".to_string()));
        }
        let enabled = input.enabled.unwrap_or(existing.is_enabled());

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

        let plan_id = match &input.plan_id {
            Some(value) => validate_plan(&self.pool, value.as_deref()).await?,
            None => existing.plan_id.clone(),
        };
        let allowed_models = match &input.allowed_models {
            Some(value) => normalize_allowed_models(value.clone())?,
            None => existing
                .allowed_models
                .as_ref()
                .map(|models| models.0.clone()),
        };

        sqlx::query(
            "UPDATE api_keys
             SET name = ?, enabled = ?, rate_limit_per_minute = ?, monthly_budget_usd = ?, budget_mode = ?, plan_id = ?, allowed_models = ?
             WHERE id = ?",
        )
        .bind(name)
        .bind(i64::from(enabled))
        .bind(rate_limit)
        .bind(budget)
        .bind(budget_mode.as_str())
        .bind(plan_id)
        .bind(allowed_models.map(Json))
        .bind(id)
        .execute(&self.pool)
        .await?;

        self.get(id)
            .await?
            .ok_or_else(|| Error::Internal("api key disappeared after update".to_string()))
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
