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
    /// Connection that served the attempt, snapshotted at write time.
    pub connection_name: Option<String>,
    pub attempt: i64,
    pub status: String,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub cached_tokens: i64,
    /// Reasoning tokens, included in `completion_tokens`.
    pub reasoning_tokens: i64,
    pub cost_usd: f64,
    /// Input share of `cost_usd`.
    pub cost_input_usd: f64,
    /// Output share, excluding the reasoning premium.
    pub cost_output_usd: f64,
    /// Reasoning premium over the output rate.
    pub cost_reasoning_usd: f64,
    pub latency_ms: i64,
}

/// A single attempt to record. Status is `ok` or `error`.
#[derive(Debug, Clone, Deserialize)]
pub struct NewUsageRecord {
    pub api_key_id: Option<String>,
    pub requested_model: String,
    pub resolved_provider: Option<String>,
    pub resolved_model: Option<String>,
    pub connection_name: Option<String>,
    pub attempt: usize,
    pub status: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_tokens: u64,
    pub reasoning_tokens: u64,
    pub cost_usd: f64,
    pub cost_input_usd: f64,
    pub cost_output_usd: f64,
    pub cost_reasoning_usd: f64,
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
    pub reasoning_tokens: i64,
    pub cost_usd: f64,
    /// Input share of the cost.
    pub cost_input_usd: f64,
    /// Output share, excluding the reasoning premium.
    pub cost_output_usd: f64,
    /// Reasoning premium over the output rate.
    pub cost_reasoning_usd: f64,
    pub avg_latency_ms: f64,
}

/// Spend for one API key, split by budget window.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct KeySpend {
    pub api_key_id: String,
    /// Spend since the start of the current UTC day.
    pub daily_usd: f64,
    /// Spend since the start of the current UTC week.
    pub weekly_usd: f64,
    /// Spend since the start of the current UTC month.
    pub monthly_usd: f64,
    /// All recorded spend; the lifetime window.
    pub lifetime_usd: f64,
    /// Prompt + completion tokens since the start of the current UTC day.
    pub daily_tokens: i64,
    /// Prompt + completion tokens since the start of the current UTC week.
    pub weekly_tokens: i64,
    /// Prompt + completion tokens since the start of the current UTC month.
    pub monthly_tokens: i64,
    /// All recorded prompt + completion tokens; the lifetime window.
    pub lifetime_tokens: i64,
}

/// Distinct values seen in usage rows, for filter pickers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageFacets {
    /// Requested model references, most used first.
    pub models: Vec<String>,
    pub providers: Vec<String>,
    /// Connection names that served requests, most used first.
    pub connections: Vec<String>,
}

/// Per-model rollup over a filtered set, used by the self-service page.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ModelUsage {
    pub model: String,
    pub requests: i64,
    pub error_requests: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub cost_usd: f64,
}

/// Bucket width for the usage trend rollup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bucket {
    Hour,
    Day,
}

impl Bucket {
    /// Parses a `bucket` query value; absent or blank means `day`.
    pub fn parse(value: Option<&str>) -> Result<Self> {
        match value.map(str::trim) {
            None | Some("") | Some("day") => Ok(Bucket::Day),
            Some("hour") => Ok(Bucket::Hour),
            Some(other) => Err(crate::error::Error::BadRequest(format!(
                "bucket must be 'hour' or 'day' (got '{other}')"
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Bucket::Hour => "hour",
            Bucket::Day => "day",
        }
    }

    /// `created_at` is stored as RFC 3339, so the bucket start is a fixed-width
    /// prefix. Substrings avoid SQLite date parsing of fractional seconds.
    fn expression(self) -> &'static str {
        match self {
            Bucket::Hour => "substr(created_at, 1, 13) || ':00:00Z'",
            Bucket::Day => "substr(created_at, 1, 10)",
        }
    }
}

/// One (bucket, model) cell of the usage trend, oldest bucket first.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct UsageBucket {
    /// RFC 3339 bucket start for `hour`, `YYYY-MM-DD` for `day`.
    pub bucket: String,
    /// Requested model reference, so the dashboard can stack by model.
    pub model: String,
    pub requests: i64,
    pub error_requests: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub cost_usd: f64,
}

/// Optional usage filters. Blank strings count as "no filter"; the model is a
/// case-insensitive substring match, the rest are exact.
#[derive(Debug, Clone, Default)]
pub struct UsageFilter {
    pub api_key_id: Option<String>,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub connection: Option<String>,
    pub since: Option<DateTime<Utc>>,
    /// Inclusive upper bound, used by the month selector.
    pub until: Option<DateTime<Utc>>,
}

impl UsageFilter {
    pub fn new(
        api_key_id: Option<String>,
        model: Option<String>,
        provider: Option<String>,
        connection: Option<String>,
        since: Option<DateTime<Utc>>,
    ) -> Self {
        Self {
            api_key_id: normalized(api_key_id),
            model: normalized(model),
            provider: normalized(provider),
            connection: normalized(connection),
            since,
            until: None,
        }
    }

    /// `LIKE` pattern for the model filter.
    fn model_pattern(&self) -> Option<String> {
        self.model
            .as_deref()
            .map(|model| format!("%{}%", model.to_lowercase()))
    }
}

/// Trims a filter value, treating blanks as unset.
fn normalized(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
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
                 connection_name, attempt, status, prompt_tokens, completion_tokens, cached_tokens,
                 reasoning_tokens, cost_usd, cost_input_usd, cost_output_usd, cost_reasoning_usd,
                 latency_ms)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(Utc::now())
        .bind(&entry.api_key_id)
        .bind(&entry.requested_model)
        .bind(&entry.resolved_provider)
        .bind(&entry.resolved_model)
        .bind(&entry.connection_name)
        .bind(entry.attempt as i64)
        .bind(&entry.status)
        .bind(entry.prompt_tokens as i64)
        .bind(entry.completion_tokens as i64)
        .bind(entry.cached_tokens as i64)
        .bind(entry.reasoning_tokens as i64)
        .bind(entry.cost_usd)
        .bind(entry.cost_input_usd)
        .bind(entry.cost_output_usd)
        .bind(entry.cost_reasoning_usd)
        .bind(entry.latency_ms as i64)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list(
        &self,
        limit: i64,
        offset: i64,
        filter: &UsageFilter,
    ) -> Result<Vec<UsageRecord>> {
        let rows = sqlx::query_as::<_, UsageRecord>(
            "SELECT * FROM usage_records
             WHERE (?1 IS NULL OR api_key_id = ?1)
               AND (?2 IS NULL OR lower(requested_model) LIKE ?2)
               AND (?3 IS NULL OR resolved_provider = ?3)
               AND (?4 IS NULL OR connection_name = ?4)
               AND (?5 IS NULL OR created_at >= ?5)
               AND (?6 IS NULL OR created_at <= ?6)
             ORDER BY created_at DESC
             LIMIT ?7 OFFSET ?8",
        )
        .bind(&filter.api_key_id)
        .bind(filter.model_pattern())
        .bind(&filter.provider)
        .bind(&filter.connection)
        .bind(filter.since)
        .bind(filter.until)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Rollup over the filtered set.
    pub async fn summary(&self, filter: &UsageFilter) -> Result<UsageSummary> {
        let row = sqlx::query_as::<_, UsageSummary>(
            "SELECT
                COUNT(*) AS requests,
                COALESCE(SUM(CASE WHEN status = 'ok' THEN 1 ELSE 0 END), 0) AS ok_requests,
                COALESCE(SUM(CASE WHEN status = 'ok' THEN 0 ELSE 1 END), 0) AS error_requests,
                COALESCE(SUM(prompt_tokens), 0) AS prompt_tokens,
                COALESCE(SUM(completion_tokens), 0) AS completion_tokens,
                COALESCE(SUM(cached_tokens), 0) AS cached_tokens,
                COALESCE(SUM(reasoning_tokens), 0) AS reasoning_tokens,
                COALESCE(SUM(cost_usd), 0.0) AS cost_usd,
                COALESCE(SUM(cost_input_usd), 0.0) AS cost_input_usd,
                COALESCE(SUM(cost_output_usd), 0.0) AS cost_output_usd,
                COALESCE(SUM(cost_reasoning_usd), 0.0) AS cost_reasoning_usd,
                COALESCE(AVG(latency_ms), 0.0) AS avg_latency_ms
             FROM usage_records
             WHERE (?1 IS NULL OR api_key_id = ?1)
               AND (?2 IS NULL OR lower(requested_model) LIKE ?2)
               AND (?3 IS NULL OR resolved_provider = ?3)
               AND (?4 IS NULL OR connection_name = ?4)
               AND (?5 IS NULL OR created_at >= ?5)
               AND (?6 IS NULL OR created_at <= ?6)",
        )
        .bind(&filter.api_key_id)
        .bind(filter.model_pattern())
        .bind(&filter.provider)
        .bind(&filter.connection)
        .bind(filter.since)
        .bind(filter.until)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    /// Per-model rollup over the filtered set, priciest model first.
    pub async fn models(&self, filter: &UsageFilter) -> Result<Vec<ModelUsage>> {
        let rows = sqlx::query_as::<_, ModelUsage>(
            "SELECT
                requested_model AS model,
                COUNT(*) AS requests,
                COALESCE(SUM(CASE WHEN status = 'ok' THEN 0 ELSE 1 END), 0) AS error_requests,
                COALESCE(SUM(prompt_tokens), 0) AS prompt_tokens,
                COALESCE(SUM(completion_tokens), 0) AS completion_tokens,
                COALESCE(SUM(cost_usd), 0.0) AS cost_usd
             FROM usage_records
             WHERE (?1 IS NULL OR api_key_id = ?1)
               AND (?2 IS NULL OR lower(requested_model) LIKE ?2)
               AND (?3 IS NULL OR resolved_provider = ?3)
               AND (?4 IS NULL OR connection_name = ?4)
               AND (?5 IS NULL OR created_at >= ?5)
               AND (?6 IS NULL OR created_at <= ?6)
             GROUP BY requested_model
             ORDER BY cost_usd DESC, requests DESC, requested_model
             LIMIT 200",
        )
        .bind(&filter.api_key_id)
        .bind(filter.model_pattern())
        .bind(&filter.provider)
        .bind(&filter.connection)
        .bind(filter.since)
        .bind(filter.until)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Time-bucketed rollup per model over the filtered set, oldest first.
    pub async fn timeseries(
        &self,
        filter: &UsageFilter,
        bucket: Bucket,
    ) -> Result<Vec<UsageBucket>> {
        let sql = format!(
            "SELECT
                {} AS bucket,
                requested_model AS model,
                COUNT(*) AS requests,
                COALESCE(SUM(CASE WHEN status = 'ok' THEN 0 ELSE 1 END), 0) AS error_requests,
                COALESCE(SUM(prompt_tokens), 0) AS prompt_tokens,
                COALESCE(SUM(completion_tokens), 0) AS completion_tokens,
                COALESCE(SUM(cost_usd), 0.0) AS cost_usd
             FROM usage_records
             WHERE (?1 IS NULL OR api_key_id = ?1)
               AND (?2 IS NULL OR lower(requested_model) LIKE ?2)
               AND (?3 IS NULL OR resolved_provider = ?3)
               AND (?4 IS NULL OR connection_name = ?4)
               AND (?5 IS NULL OR created_at >= ?5)
               AND (?6 IS NULL OR created_at <= ?6)
             GROUP BY bucket, model
             ORDER BY bucket, model
             LIMIT 5000",
            bucket.expression()
        );

        let rows = sqlx::query_as::<_, UsageBucket>(&sql)
            .bind(&filter.api_key_id)
            .bind(filter.model_pattern())
            .bind(&filter.provider)
            .bind(&filter.connection)
            .bind(filter.since)
            .bind(filter.until)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows)
    }

    /// Distinct models, providers and connections seen so far, for filter
    /// pickers. Models and connections are ordered most used first.
    pub async fn facets(&self) -> Result<UsageFacets> {
        let models: Vec<(String,)> = sqlx::query_as(
            "SELECT requested_model
             FROM usage_records
             GROUP BY requested_model
             ORDER BY COUNT(*) DESC, requested_model
             LIMIT 200",
        )
        .fetch_all(&self.pool)
        .await?;

        let providers: Vec<(String,)> = sqlx::query_as(
            "SELECT resolved_provider
             FROM usage_records
             WHERE resolved_provider IS NOT NULL
             GROUP BY resolved_provider
             ORDER BY resolved_provider",
        )
        .fetch_all(&self.pool)
        .await?;

        let connections: Vec<(String,)> = sqlx::query_as(
            "SELECT connection_name
             FROM usage_records
             WHERE connection_name IS NOT NULL
             GROUP BY connection_name
             ORDER BY COUNT(*) DESC, connection_name
             LIMIT 200",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(UsageFacets {
            models: models.into_iter().map(|(model,)| model).collect(),
            providers: providers.into_iter().map(|(provider,)| provider).collect(),
            connections: connections
                .into_iter()
                .map(|(connection,)| connection)
                .collect(),
        })
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

    /// Prompt + completion tokens for one key since `since`, used for token
    /// limit checks.
    pub async fn tokens_since(&self, api_key_id: &str, since: DateTime<Utc>) -> Result<i64> {
        let total: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(prompt_tokens + completion_tokens), 0)
             FROM usage_records
             WHERE api_key_id = ? AND created_at >= ?",
        )
        .bind(api_key_id)
        .bind(since)
        .fetch_one(&self.pool)
        .await?;
        Ok(total)
    }

    /// Spend and tokens for every key that has usage rows, split by the budget
    /// windows. Rows without a key (unauthenticated `/v1` calls) are skipped.
    pub async fn spend_by_key(
        &self,
        daily_since: DateTime<Utc>,
        weekly_since: DateTime<Utc>,
        monthly_since: DateTime<Utc>,
    ) -> Result<Vec<KeySpend>> {
        let rows = sqlx::query_as::<_, KeySpend>(
            "SELECT api_key_id,
                    COALESCE(SUM(CASE WHEN created_at >= ?1 THEN cost_usd ELSE 0.0 END), 0.0) AS daily_usd,
                    COALESCE(SUM(CASE WHEN created_at >= ?2 THEN cost_usd ELSE 0.0 END), 0.0) AS weekly_usd,
                    COALESCE(SUM(CASE WHEN created_at >= ?3 THEN cost_usd ELSE 0.0 END), 0.0) AS monthly_usd,
                    COALESCE(SUM(cost_usd), 0.0) AS lifetime_usd,
                    COALESCE(SUM(CASE WHEN created_at >= ?1 THEN prompt_tokens + completion_tokens ELSE 0 END), 0) AS daily_tokens,
                    COALESCE(SUM(CASE WHEN created_at >= ?2 THEN prompt_tokens + completion_tokens ELSE 0 END), 0) AS weekly_tokens,
                    COALESCE(SUM(CASE WHEN created_at >= ?3 THEN prompt_tokens + completion_tokens ELSE 0 END), 0) AS monthly_tokens,
                    COALESCE(SUM(prompt_tokens + completion_tokens), 0) AS lifetime_tokens
             FROM usage_records
             WHERE api_key_id IS NOT NULL
             GROUP BY api_key_id",
        )
        .bind(daily_since)
        .bind(weekly_since)
        .bind(monthly_since)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Spend and tokens for one key, split by the budget windows. Returns a
    /// zeroed `KeySpend` when the key has no usage rows.
    pub async fn spend_for_key(
        &self,
        api_key_id: &str,
        daily_since: DateTime<Utc>,
        weekly_since: DateTime<Utc>,
        monthly_since: DateTime<Utc>,
    ) -> Result<KeySpend> {
        let row = sqlx::query_as::<_, KeySpend>(
            "SELECT ?1 AS api_key_id,
                    COALESCE(SUM(CASE WHEN created_at >= ?2 THEN cost_usd ELSE 0.0 END), 0.0) AS daily_usd,
                    COALESCE(SUM(CASE WHEN created_at >= ?3 THEN cost_usd ELSE 0.0 END), 0.0) AS weekly_usd,
                    COALESCE(SUM(CASE WHEN created_at >= ?4 THEN cost_usd ELSE 0.0 END), 0.0) AS monthly_usd,
                    COALESCE(SUM(cost_usd), 0.0) AS lifetime_usd,
                    COALESCE(SUM(CASE WHEN created_at >= ?2 THEN prompt_tokens + completion_tokens ELSE 0 END), 0) AS daily_tokens,
                    COALESCE(SUM(CASE WHEN created_at >= ?3 THEN prompt_tokens + completion_tokens ELSE 0 END), 0) AS weekly_tokens,
                    COALESCE(SUM(CASE WHEN created_at >= ?4 THEN prompt_tokens + completion_tokens ELSE 0 END), 0) AS monthly_tokens,
                    COALESCE(SUM(prompt_tokens + completion_tokens), 0) AS lifetime_tokens
             FROM usage_records
             WHERE api_key_id = ?1",
        )
        .bind(api_key_id)
        .bind(daily_since)
        .bind(weekly_since)
        .bind(monthly_since)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn record(model: &str, cost: f64) -> NewUsageRecord {
        NewUsageRecord {
            api_key_id: None,
            requested_model: model.to_string(),
            resolved_provider: None,
            resolved_model: None,
            connection_name: None,
            attempt: 1,
            status: "ok".to_string(),
            prompt_tokens: 2,
            completion_tokens: 3,
            cached_tokens: 0,
            reasoning_tokens: 0,
            cost_usd: cost,
            cost_input_usd: cost,
            cost_output_usd: 0.0,
            cost_reasoning_usd: 0.0,
            latency_ms: 5,
        }
    }

    #[test]
    fn bucket_parses_and_defaults_to_day() {
        assert_eq!(Bucket::parse(None).expect("default"), Bucket::Day);
        assert_eq!(Bucket::parse(Some("")).expect("blank"), Bucket::Day);
        assert_eq!(Bucket::parse(Some("hour")).expect("hour"), Bucket::Hour);
        assert_eq!(Bucket::parse(Some(" day ")).expect("day"), Bucket::Day);
        assert!(Bucket::parse(Some("week")).is_err());
    }

    #[tokio::test]
    async fn timeseries_groups_records_per_bucket_and_model() {
        let db = Db::connect_in_memory().await.expect("db");
        let repository = UsageRepository::new(db.pool.clone());
        repository.record(record("a", 1.0)).await.expect("first");
        repository.record(record("a", 0.5)).await.expect("second");
        repository
            .record(record("b", 2.0))
            .await
            .expect("other model");

        let buckets = repository
            .timeseries(&UsageFilter::default(), Bucket::Hour)
            .await
            .expect("series");

        assert_eq!(buckets.len(), 2, "one row per (bucket, model)");
        assert_eq!(buckets[0].model, "a");
        assert_eq!(buckets[0].requests, 2);
        assert_eq!(buckets[0].cost_usd, 1.5);
        assert_eq!(buckets[0].prompt_tokens, 4);
        assert_eq!(buckets[1].model, "b");
        assert_eq!(buckets[1].requests, 1);
        assert!(
            buckets.iter().all(|row| row.bucket.ends_with(":00:00Z")),
            "records written together share an hour: {buckets:?}"
        );
    }
}
