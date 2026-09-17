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
    /// Prompt tokens removed by RTK/Slimmer before dispatch; measured.
    pub saved_rtk_tokens: i64,
    /// Prompt tokens removed by the Headroom proxy; measured.
    pub saved_headroom_tokens: i64,
    /// Completion tokens avoided by the terse directive; estimated.
    pub saved_terse_tokens: i64,
    /// Completion tokens avoided by the caveman directive; estimated.
    pub saved_caveman_tokens: i64,
    /// Completion tokens avoided by the ponytail directive; estimated.
    pub saved_ponytail_tokens: i64,
    /// Money saved by every saver combined, at the serving tier's rate.
    pub saved_cost_usd: f64,
}

/// A single attempt to record. Status is `ok` or `error`.
///
/// `Default` zeroes every figure, so a caller only spells out the columns it
/// actually has — a failed attempt needs no token counts, and a request with no
/// saver enabled needs no savings.
#[derive(Debug, Clone, Default, Deserialize)]
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
    /// What the token-saving pipeline saved on this request. RTK and Headroom
    /// are measured; the directive columns are estimates derived from the
    /// completion.
    pub saved_rtk_tokens: u64,
    pub saved_headroom_tokens: u64,
    pub saved_terse_tokens: u64,
    pub saved_caveman_tokens: u64,
    pub saved_ponytail_tokens: u64,
    pub saved_cost_usd: f64,
}

impl NewUsageRecord {
    /// Attaches the token-saving figures finalized for this request.
    pub fn with_savings(mut self, totals: crate::token_saver::SavingsTotals) -> Self {
        self.saved_rtk_tokens = totals.saved_rtk_tokens;
        self.saved_headroom_tokens = totals.saved_headroom_tokens;
        self.saved_terse_tokens = totals.saved_terse_tokens;
        self.saved_caveman_tokens = totals.saved_caveman_tokens;
        self.saved_ponytail_tokens = totals.saved_ponytail_tokens;
        self.saved_cost_usd = totals.saved_cost_usd;
        self
    }
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

impl UsageSummary {
    /// Token-saver figures for the same filtered window, kept as a nested block
    /// so existing consumers of the cost fields are untouched.
    pub fn savings(&self, totals: SaverSummary) -> UsageSummaryWithSavings {
        UsageSummaryWithSavings {
            requests: self.requests,
            ok_requests: self.ok_requests,
            error_requests: self.error_requests,
            prompt_tokens: self.prompt_tokens,
            completion_tokens: self.completion_tokens,
            cached_tokens: self.cached_tokens,
            reasoning_tokens: self.reasoning_tokens,
            cost_usd: self.cost_usd,
            cost_input_usd: self.cost_input_usd,
            cost_output_usd: self.cost_output_usd,
            cost_reasoning_usd: self.cost_reasoning_usd,
            avg_latency_ms: self.avg_latency_ms,
            savings: totals,
        }
    }
}

/// A [`UsageSummary`] with the token-saver block attached.
///
/// Flattened rather than nested so the wire shape keeps every existing field at
/// the top level and adds one `savings` object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageSummaryWithSavings {
    pub requests: i64,
    pub ok_requests: i64,
    pub error_requests: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub cached_tokens: i64,
    pub reasoning_tokens: i64,
    pub cost_usd: f64,
    pub cost_input_usd: f64,
    pub cost_output_usd: f64,
    pub cost_reasoning_usd: f64,
    pub avg_latency_ms: f64,
    pub savings: SaverSummary,
}

/// Token-saver rollup over a filtered set.
///
/// The input columns are measured; the directive columns are estimates, and
/// [`Self::saved_tokens`] keeps them labelled as such rather than blending the
/// two into one figure the dashboard would present as fact.
#[derive(Debug, Clone, Default, Serialize, Deserialize, FromRow)]
pub struct SaverSummary {
    /// Requests where at least one saver contributed.
    pub requests: i64,
    pub saved_rtk_tokens: i64,
    pub saved_headroom_tokens: i64,
    pub saved_terse_tokens: i64,
    pub saved_caveman_tokens: i64,
    pub saved_ponytail_tokens: i64,
    pub saved_cost_usd: f64,
}

impl SaverSummary {
    /// Prompt tokens removed before dispatch; measured.
    pub fn measured_tokens(&self) -> i64 {
        self.saved_rtk_tokens + self.saved_headroom_tokens
    }

    /// Completion tokens the directives are expected to have avoided; estimated.
    pub fn estimated_tokens(&self) -> i64 {
        self.saved_terse_tokens + self.saved_caveman_tokens + self.saved_ponytail_tokens
    }

    pub fn saved_tokens(&self) -> i64 {
        self.measured_tokens() + self.estimated_tokens()
    }

    /// Savers that contributed, largest first.
    pub fn contributions(&self) -> Vec<(&'static str, i64)> {
        let mut entries = vec![
            ("rtk", self.saved_rtk_tokens),
            ("headroom", self.saved_headroom_tokens),
            ("terse", self.saved_terse_tokens),
            ("caveman", self.saved_caveman_tokens),
            ("ponytail", self.saved_ponytail_tokens),
        ];
        entries.retain(|(_, tokens)| *tokens > 0);
        entries.sort_by_key(|(_, tokens)| std::cmp::Reverse(*tokens));
        entries
    }
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

/// Column the usage table is sorted by. Whitelisted so the value can never
/// reach the SQL string as anything but a fixed fragment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortField {
    CreatedAt,
    RequestedModel,
    Connection,
    Status,
    Tokens,
    Cost,
    Latency,
}

impl SortField {
    fn parse(value: &str) -> Result<Self> {
        match value.trim().to_lowercase().as_str() {
            "time" | "created_at" => Ok(SortField::CreatedAt),
            "model" | "requested_model" => Ok(SortField::RequestedModel),
            "connection" => Ok(SortField::Connection),
            "status" => Ok(SortField::Status),
            "tokens" => Ok(SortField::Tokens),
            "cost" => Ok(SortField::Cost),
            "latency" | "latency_ms" => Ok(SortField::Latency),
            other => Err(crate::error::Error::BadRequest(format!(
                "sort must be one of time, model, connection, status, tokens, cost, \
                 latency (got '{other}')"
            ))),
        }
    }

    /// Sort key. `tokens` sums the two columns the table shows as one figure.
    fn expression(self) -> &'static str {
        match self {
            SortField::CreatedAt => "created_at",
            SortField::RequestedModel => "requested_model",
            SortField::Connection => "connection_name",
            SortField::Status => "status",
            SortField::Tokens => "prompt_tokens + completion_tokens",
            SortField::Cost => "cost_usd",
            SortField::Latency => "latency_ms",
        }
    }
}

/// Sort order for the usage table: a whitelisted field plus a direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sort {
    field: SortField,
    descending: bool,
}

impl Default for Sort {
    /// Newest first, the order the live table has always used.
    fn default() -> Self {
        Self {
            field: SortField::CreatedAt,
            descending: true,
        }
    }
}

impl Sort {
    /// Parses `sort` and `order` query values. Absent `sort` keeps the default
    /// order; `order` accepts `asc`/`desc` and defaults to descending.
    pub fn parse(sort: Option<&str>, order: Option<&str>) -> Result<Self> {
        let field = match sort.map(str::trim).filter(|value| !value.is_empty()) {
            Some(value) => SortField::parse(value)?,
            None => SortField::CreatedAt,
        };
        let descending = match order.map(str::trim).filter(|value| !value.is_empty()) {
            None | Some("desc") | Some("descending") => true,
            Some("asc") | Some("ascending") => false,
            Some(other) => {
                return Err(crate::error::Error::BadRequest(format!(
                    "order must be 'asc' or 'desc' (got '{other}')"
                )));
            }
        };
        Ok(Self { field, descending })
    }

    /// `ORDER BY` body, with `created_at DESC` as a stable tiebreak so paging
    /// through equal keys cannot repeat or skip rows.
    fn clause(self) -> String {
        let direction = if self.descending { "DESC" } else { "ASC" };
        format!("{} {direction}, created_at DESC", self.field.expression())
    }
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
                 latency_ms, saved_rtk_tokens, saved_headroom_tokens, saved_terse_tokens,
                 saved_caveman_tokens, saved_ponytail_tokens, saved_cost_usd)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
        .bind(entry.saved_rtk_tokens as i64)
        .bind(entry.saved_headroom_tokens as i64)
        .bind(entry.saved_terse_tokens as i64)
        .bind(entry.saved_caveman_tokens as i64)
        .bind(entry.saved_ponytail_tokens as i64)
        .bind(entry.saved_cost_usd)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list(
        &self,
        limit: i64,
        offset: i64,
        filter: &UsageFilter,
        sort: Sort,
    ) -> Result<Vec<UsageRecord>> {
        let sql = format!(
            "SELECT * FROM usage_records
             WHERE (?1 IS NULL OR api_key_id = ?1)
               AND (?2 IS NULL OR lower(requested_model) LIKE ?2)
               AND (?3 IS NULL OR resolved_provider = ?3)
               AND (?4 IS NULL OR connection_name = ?4)
               AND (?5 IS NULL OR created_at >= ?5)
               AND (?6 IS NULL OR created_at <= ?6)
             ORDER BY {}
             LIMIT ?7 OFFSET ?8",
            sort.clause()
        );

        let rows = sqlx::query_as::<_, UsageRecord>(&sql)
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

    /// Token-saver rollup over the filtered set.
    pub async fn savings(&self, filter: &UsageFilter) -> Result<SaverSummary> {
        let row = sqlx::query_as::<_, SaverSummary>(
            "SELECT
                COALESCE(SUM(CASE WHEN saved_rtk_tokens + saved_headroom_tokens
                     + saved_terse_tokens + saved_caveman_tokens + saved_ponytail_tokens > 0
                     THEN 1 ELSE 0 END), 0) AS requests,
                COALESCE(SUM(saved_rtk_tokens), 0) AS saved_rtk_tokens,
                COALESCE(SUM(saved_headroom_tokens), 0) AS saved_headroom_tokens,
                COALESCE(SUM(saved_terse_tokens), 0) AS saved_terse_tokens,
                COALESCE(SUM(saved_caveman_tokens), 0) AS saved_caveman_tokens,
                COALESCE(SUM(saved_ponytail_tokens), 0) AS saved_ponytail_tokens,
                COALESCE(SUM(saved_cost_usd), 0.0) AS saved_cost_usd
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
            requested_model: model.to_string(),
            attempt: 1,
            status: "ok".to_string(),
            prompt_tokens: 2,
            completion_tokens: 3,
            cost_usd: cost,
            cost_input_usd: cost,
            latency_ms: 5,
            ..Default::default()
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

    #[test]
    fn sort_parses_whitelisted_fields_and_directions() {
        let default = Sort::parse(None, None).expect("default");
        assert_eq!(default.field, SortField::CreatedAt);
        assert!(default.descending, "the table defaults to newest first");

        assert_eq!(
            Sort::parse(Some("cost"), None).expect("cost").field,
            SortField::Cost
        );
        assert!(
            !Sort::parse(Some("cost"), Some("asc"))
                .expect("ascending")
                .descending
        );
        assert!(
            Sort::parse(Some("latency"), Some(" descending "))
                .expect("descending")
                .descending
        );

        // Unknown keys are rejected rather than interpolated into the SQL.
        assert!(Sort::parse(Some("created_at; DROP TABLE usage_records"), None).is_err());
        assert!(Sort::parse(Some("cost"), Some("sideways")).is_err());
    }

    #[tokio::test]
    async fn list_orders_by_the_requested_column() {
        let db = Db::connect_in_memory().await.expect("db");
        let repository = UsageRepository::new(db.pool.clone());
        repository
            .record(record("cheap", 1.0))
            .await
            .expect("first");
        repository
            .record(record("pricey", 9.0))
            .await
            .expect("second");

        let by_cost = repository
            .list(
                10,
                0,
                &UsageFilter::default(),
                Sort::parse(Some("cost"), Some("asc")).expect("sort"),
            )
            .await
            .expect("rows");
        assert_eq!(by_cost[0].requested_model, "cheap");
        assert_eq!(by_cost[1].requested_model, "pricey");

        let by_model = repository
            .list(
                10,
                0,
                &UsageFilter::default(),
                Sort::parse(Some("model"), Some("desc")).expect("sort"),
            )
            .await
            .expect("rows");
        assert_eq!(by_model[0].requested_model, "pricey");
    }

    #[tokio::test]
    async fn list_filters_on_both_ends_of_the_window() {
        let db = Db::connect_in_memory().await.expect("db");
        let repository = UsageRepository::new(db.pool.clone());
        repository.record(record("a", 1.0)).await.expect("row");

        let future = UsageFilter {
            since: Some(Utc::now() + chrono::Duration::hours(1)),
            ..UsageFilter::default()
        };
        let rows = repository
            .list(10, 0, &future, Sort::default())
            .await
            .expect("rows");
        assert!(rows.is_empty(), "since in the future excludes every row");

        let past = UsageFilter {
            until: Some(Utc::now() - chrono::Duration::hours(1)),
            ..UsageFilter::default()
        };
        let rows = repository
            .list(10, 0, &past, Sort::default())
            .await
            .expect("rows");
        assert!(
            rows.is_empty(),
            "until in the past excludes every row, so the month selector works"
        );
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
