//! Usage tracking: one row per upstream attempt, plus rollup aggregates.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct UsageRecord {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub api_key_id: Option<String>,
    pub requested_model: String,
    pub resolved_provider: Option<String>,
    pub resolved_model: Option<String>,
    pub attempt: i64,
    pub status: String,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub cached_tokens: i64,
    pub cost_usd: f64,
    pub latency_ms: i64,
}

/// A single attempt to record. Status is `ok` or `error`.
#[derive(Debug, Clone, Deserialize)]
pub struct NewUsageRecord {
    pub api_key_id: Option<String>,
    pub requested_model: String,
    pub resolved_provider: Option<String>,
    pub resolved_model: Option<String>,
    pub attempt: usize,
    pub status: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_tokens: u64,
    pub cost_usd: f64,
    pub latency_ms: u64,
}

/// Aggregate rollup over a time window.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct UsageSummary {
    pub requests: i64,
    pub ok_requests: i64,
    pub error_requests: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub cached_tokens: i64,
    pub cost_usd: f64,
    pub avg_latency_ms: f64,
}

pub struct UsageRepository {
    pool: SqlitePool,
}

impl UsageRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn record(&self, entry: NewUsageRecord) -> Result<()> {
        sqlx::query(
            "INSERT INTO usage_records
                (id, created_at, api_key_id, requested_model, resolved_provider, resolved_model,
                 attempt, status, prompt_tokens, completion_tokens, cached_tokens, cost_usd, latency_ms)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(Utc::now())
        .bind(&entry.api_key_id)
        .bind(&entry.requested_model)
        .bind(&entry.resolved_provider)
        .bind(&entry.resolved_model)
        .bind(entry.attempt as i64)
        .bind(&entry.status)
        .bind(entry.prompt_tokens as i64)
        .bind(entry.completion_tokens as i64)
        .bind(entry.cached_tokens as i64)
        .bind(entry.cost_usd)
        .bind(entry.latency_ms as i64)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list(&self, limit: i64, offset: i64) -> Result<Vec<UsageRecord>> {
        let rows = sqlx::query_as::<_, UsageRecord>(
            "SELECT * FROM usage_records ORDER BY created_at DESC LIMIT ? OFFSET ?",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Rollup since `since`. Pass `None` for all time.
    pub async fn summary(&self, since: Option<DateTime<Utc>>) -> Result<UsageSummary> {
        let row = sqlx::query_as::<_, UsageSummary>(
            "SELECT
                COUNT(*) AS requests,
                COALESCE(SUM(CASE WHEN status = 'ok' THEN 1 ELSE 0 END), 0) AS ok_requests,
                COALESCE(SUM(CASE WHEN status = 'ok' THEN 0 ELSE 1 END), 0) AS error_requests,
                COALESCE(SUM(prompt_tokens), 0) AS prompt_tokens,
                COALESCE(SUM(completion_tokens), 0) AS completion_tokens,
                COALESCE(SUM(cached_tokens), 0) AS cached_tokens,
                COALESCE(SUM(cost_usd), 0.0) AS cost_usd,
                COALESCE(AVG(latency_ms), 0.0) AS avg_latency_ms
             FROM usage_records
             WHERE (?1 IS NULL OR created_at >= ?1)",
        )
        .bind(since)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    /// Total recorded spend for one key since `since`, used for budget checks.
    pub async fn spend_since(&self, api_key_id: &str, since: DateTime<Utc>) -> Result<f64> {
        let total: f64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(cost_usd), 0.0)
             FROM usage_records
             WHERE api_key_id = ? AND created_at >= ?",
        )
        .bind(api_key_id)
        .bind(since)
        .fetch_one(&self.pool)
        .await?;
        Ok(total)
    }
}
