//! Router-issued client API keys keyed.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::types::Json;
use sqlx::{FromRow, SqlitePool};

use crate::error::{Error, Result};
use crate::limits::{BudgetMode, BudgetWindows, TokenWindows};

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
    /// Optional plan whose rules fill the fields this key leaves empty.
    pub plan_id: Option<String>,
    /// Model allowlist patterns; `None` inherits the plan (or allows any model).
    pub allowed_models: Option<Json<Vec<String>>>,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    /// When the key stops authenticating; `None` never expires.
    pub expires_at: Option<DateTime<Utc>>,
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

    /// True when the key is past its expiry; a key without one never expires.
    pub fn is_expired(&self) -> bool {
        self.expires_at.is_some_and(|at| at <= Utc::now())
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
    pub plan_id: Option<String>,
    #[serde(default)]
    pub allowed_models: Option<Vec<String>>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
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
    pub plan_id: Option<Option<String>>,
    #[serde(default, deserialize_with = "crate::db::repos::double_option")]
    pub allowed_models: Option<Option<Vec<String>>>,
    #[serde(default, deserialize_with = "crate::db::repos::double_option")]
    pub expires_at: Option<Option<DateTime<Utc>>>,
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

pub(crate) fn normalized_budget(value: Option<f64>, field: &str) -> Result<Option<f64>> {
    match value {
        None => Ok(None),
        Some(value) if !value.is_finite() || value <= 0.0 => Err(Error::BadRequest(format!(
            "{field} must be a positive number"
        ))),
        Some(value) => Ok(Some(value)),
    }
}

pub(crate) fn normalized_token_limit(value: Option<i64>, field: &str) -> Result<Option<i64>> {
    match value {
        None => Ok(None),
        Some(value) if value <= 0 => Err(Error::BadRequest(format!(
            "{field} must be a positive whole number of tokens"
        ))),
        Some(value) => Ok(Some(value)),
    }
}

pub(crate) fn normalized_budget_mode(value: Option<&str>) -> Result<BudgetMode> {
    match value {
        Some(value) => BudgetMode::parse(value),
        None => Ok(BudgetMode::Off),
    }
}

pub(crate) fn validate_budget_pair(
    budgets: &BudgetWindows,
    tokens: &TokenWindows,
    mode: BudgetMode,
) -> Result<()> {
    if mode != BudgetMode::Off && budgets.is_empty() && tokens.is_empty() {
        return Err(Error::BadRequest(
            "at least one budget or token limit is required when budget_mode is warn or block"
                .to_string(),
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
        let plan_id = validate_plan(&self.pool, input.plan_id.as_deref()).await?;
        let allowed_models = normalize_allowed_models(input.allowed_models)?;

        let secret = format!("{KEY_PREFIX}{}", uuid::Uuid::new_v4().simple());
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();
        let prefix: String = secret.chars().take(KEY_PREFIX.len() + 8).collect();

        sqlx::query(
            "INSERT INTO api_keys
                (id, name, key_hash, prefix, enabled, rate_limit_per_minute, daily_budget_usd,
                 weekly_budget_usd, monthly_budget_usd, lifetime_budget_usd, daily_token_limit,
                 weekly_token_limit, monthly_token_limit, lifetime_token_limit, budget_mode,
                 plan_id, allowed_models, created_at, expires_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(name)
        .bind(hash_key(&secret))
        .bind(&prefix)
        .bind(i64::from(input.enabled))
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
        .bind(plan_id)
        .bind(allowed_models.map(Json))
        .bind(now)
        .bind(input.expires_at)
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
        let expires_at = match &input.expires_at {
            Some(value) => *value,
            None => existing.expires_at,
        };

        sqlx::query(
            "UPDATE api_keys
             SET name = ?, enabled = ?, rate_limit_per_minute = ?, daily_budget_usd = ?,
                 weekly_budget_usd = ?, monthly_budget_usd = ?, lifetime_budget_usd = ?,
                 daily_token_limit = ?, weekly_token_limit = ?, monthly_token_limit = ?,
                 lifetime_token_limit = ?, budget_mode = ?, plan_id = ?, allowed_models = ?,
                 expires_at = ?
             WHERE id = ?",
        )
        .bind(name)
        .bind(i64::from(enabled))
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
        .bind(plan_id)
        .bind(allowed_models.map(Json))
        .bind(expires_at)
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
