//! Model pricing: dashboard overrides, crawled catalogs, and the lookup cache.
//!
//! Precedence is override → synced → the built-in `known_cost_rates` table in
//! `alnair-llm` (used when the cache has no entry). The cache is loaded lazily
//! and invalidated by every pricing write or sync.

pub mod sources;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use tokio::sync::RwLock;

use crate::error::{Error, Result};
pub use sources::FetchedPrice;

/// USD rates per million tokens.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Price {
    pub input_per_million_usd: f64,
    pub output_per_million_usd: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_per_million_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_per_million_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_per_million_usd: Option<f64>,
}

/// A stored price row, as returned by `GET /api/pricing`.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PricedModel {
    pub model: String,
    pub input_per_million_usd: f64,
    pub output_per_million_usd: f64,
    pub cache_read_per_million_usd: Option<f64>,
    pub cache_write_per_million_usd: Option<f64>,
    pub reasoning_per_million_usd: Option<f64>,
    /// `override` (set from the dashboard) or `sync` (crawled catalog).
    pub source: String,
    pub updated_at: DateTime<Utc>,
}

/// One override submitted through the admin API.
#[derive(Debug, Clone, Deserialize)]
pub struct PriceInput {
    pub model: String,
    pub input_per_million_usd: f64,
    pub output_per_million_usd: f64,
    #[serde(default)]
    pub cache_read_per_million_usd: Option<f64>,
    #[serde(default)]
    pub cache_write_per_million_usd: Option<f64>,
    #[serde(default)]
    pub reasoning_per_million_usd: Option<f64>,
}

impl PriceInput {
    fn validate(&self) -> Result<()> {
        let model = self.model.trim();
        if model.is_empty() {
            return Err(Error::BadRequest("model is required".to_string()));
        }

        for (name, value) in [
            ("input_per_million_usd", Some(self.input_per_million_usd)),
            ("output_per_million_usd", Some(self.output_per_million_usd)),
            (
                "cache_read_per_million_usd",
                self.cache_read_per_million_usd,
            ),
            (
                "cache_write_per_million_usd",
                self.cache_write_per_million_usd,
            ),
            ("reasoning_per_million_usd", self.reasoning_per_million_usd),
        ] {
            if let Some(value) = value
                && (!value.is_finite() || value < 0.0)
            {
                return Err(Error::BadRequest(format!(
                    "{name} must be zero or positive"
                )));
            }
        }

        Ok(())
    }
}

/// Result of the last catalog crawl.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PricingSyncStatus {
    pub source: String,
    pub synced_at: DateTime<Utc>,
    pub model_count: i64,
}

pub struct PricingRepository {
    pool: SqlitePool,
}

impl PricingRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Every stored price, overrides included, ordered by model then source.
    pub async fn list(&self) -> Result<Vec<PricedModel>> {
        let rows = sqlx::query_as::<_, PricedModel>(
            "SELECT * FROM model_prices ORDER BY model, source DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Inserts or updates overrides; synced rows with the same model are kept
    /// but shadowed by the lookup precedence.
    pub async fn upsert_overrides(&self, inputs: &[PriceInput]) -> Result<usize> {
        let now = Utc::now();
        let mut transaction = self.pool.begin().await?;
        for input in inputs {
            input.validate()?;
            sqlx::query(
                "INSERT INTO model_prices
                    (model, input_per_million_usd, output_per_million_usd, cache_read_per_million_usd,
                     cache_write_per_million_usd, reasoning_per_million_usd, source, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'override', ?7)
                 ON CONFLICT (model, source) DO UPDATE SET
                    input_per_million_usd = excluded.input_per_million_usd,
                    output_per_million_usd = excluded.output_per_million_usd,
                    cache_read_per_million_usd = excluded.cache_read_per_million_usd,
                    cache_write_per_million_usd = excluded.cache_write_per_million_usd,
                    reasoning_per_million_usd = excluded.reasoning_per_million_usd,
                    updated_at = excluded.updated_at",
            )
            .bind(input.model.trim())
            .bind(input.input_per_million_usd)
            .bind(input.output_per_million_usd)
            .bind(input.cache_read_per_million_usd)
            .bind(input.cache_write_per_million_usd)
            .bind(input.reasoning_per_million_usd)
            .bind(now)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        Ok(inputs.len())
    }

    /// Deletes one override, or every override when `model` is `None`.
    pub async fn delete_overrides(&self, model: Option<&str>) -> Result<u64> {
        let result = match model {
            Some(model) => {
                sqlx::query("DELETE FROM model_prices WHERE source = 'override' AND model = ?")
                    .bind(model)
                    .execute(&self.pool)
                    .await?
            }
            None => {
                sqlx::query("DELETE FROM model_prices WHERE source = 'override'")
                    .execute(&self.pool)
                    .await?
            }
        };
        Ok(result.rows_affected())
    }

    /// Replaces the whole synced table in one transaction. Public so tests and
    /// future importers can seed the table without a network crawl.
    pub async fn replace_synced(&self, fetched: &[FetchedPrice], at: DateTime<Utc>) -> Result<u64> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query("DELETE FROM model_prices WHERE source = 'sync'")
            .execute(&mut *transaction)
            .await?;

        for entry in fetched {
            sqlx::query(
                "INSERT INTO model_prices
                    (model, input_per_million_usd, output_per_million_usd, cache_read_per_million_usd,
                     cache_write_per_million_usd, reasoning_per_million_usd, source, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'sync', ?7)",
            )
            .bind(&entry.model)
            .bind(entry.price.input_per_million_usd)
            .bind(entry.price.output_per_million_usd)
            .bind(entry.price.cache_read_per_million_usd)
            .bind(entry.price.cache_write_per_million_usd)
            .bind(entry.price.reasoning_per_million_usd)
            .bind(at)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;

        Ok(fetched.len() as u64)
    }

    /// Records the outcome of a crawl, keeping only the newest run.
    pub async fn record_sync(&self, status: &PricingSyncStatus) -> Result<()> {
        sqlx::query(
            "INSERT INTO pricing_sync_runs (id, source, synced_at, model_count)
             VALUES (1, ?1, ?2, ?3)
             ON CONFLICT (id) DO UPDATE SET
                source = excluded.source,
                synced_at = excluded.synced_at,
                model_count = excluded.model_count",
        )
        .bind(&status.source)
        .bind(status.synced_at)
        .bind(status.model_count)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn sync_status(&self) -> Result<Option<PricingSyncStatus>> {
        let row = sqlx::query_as::<_, PricingSyncStatus>(
            "SELECT source, synced_at, model_count FROM pricing_sync_runs WHERE id = 1",
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }
}

/// One resolved price row plus where it came from.
#[derive(Debug, Clone, Serialize)]
pub struct PriceMatch {
    /// Requested model id.
    pub model: String,
    /// Catalog key that answered; differs from `model` when only a prefixed or
    /// relaying variant matched by leaf.
    pub matched: String,
    pub source: String,
    pub price: Price,
}

/// Merged price table with an exact index and a leaf index.
///
/// Leaf resolution makes `azure/gpt-5.6-luna` answer a `gpt-5.6-luna` request
/// and vice versa, which is what relays and provider-prefixed catalogs need.
/// When several rows share a leaf the best one wins: canonical keys (no
/// `vendor/` prefix) first, then the cheapest input rate, then alphabetical.
pub struct PricingIndex {
    by_key: HashMap<String, PriceMatch>,
    by_leaf: HashMap<String, PriceMatch>,
}

impl PricingIndex {
    fn build(rows: Vec<PricedModel>) -> Self {
        let mut by_key = HashMap::new();
        let mut by_leaf: HashMap<String, PriceMatch> = HashMap::new();

        for row in rows {
            let entry = PriceMatch {
                model: row.model.clone(),
                matched: row.model.clone(),
                source: row.source,
                price: Price {
                    input_per_million_usd: row.input_per_million_usd,
                    output_per_million_usd: row.output_per_million_usd,
                    cache_read_per_million_usd: row.cache_read_per_million_usd,
                    cache_write_per_million_usd: row.cache_write_per_million_usd,
                    reasoning_per_million_usd: row.reasoning_per_million_usd,
                },
            };

            // Rows arrive sync-first, so an override with the same key wins.
            by_key.insert(entry.matched.clone(), entry.clone());

            let leaf = leaf_of(&entry.matched).to_string();
            match by_leaf.get(&leaf) {
                Some(existing) if !better_match(&entry, existing) => {}
                _ => {
                    by_leaf.insert(leaf, entry);
                }
            }
        }

        Self { by_key, by_leaf }
    }

    /// Exact id first, then the last path segment.
    pub fn find(&self, model: &str) -> Option<PriceMatch> {
        let entry = self
            .by_key
            .get(model)
            .or_else(|| self.by_leaf.get(leaf_of(model)))?;

        Some(PriceMatch {
            model: model.to_string(),
            ..entry.clone()
        })
    }
}

/// Last path segment of a model id.
fn leaf_of(model: &str) -> &str {
    model.rsplit('/').next().unwrap_or(model)
}

/// Ranking for rows that share a leaf: canonical first, then cheapest input,
/// then alphabetical so the result is stable.
fn better_match(candidate: &PriceMatch, current: &PriceMatch) -> bool {
    let canonical = |entry: &PriceMatch| !entry.matched.contains('/');
    if canonical(candidate) != canonical(current) {
        return canonical(candidate);
    }

    match candidate
        .price
        .input_per_million_usd
        .partial_cmp(&current.price.input_per_million_usd)
    {
        Some(std::cmp::Ordering::Less) => true,
        Some(std::cmp::Ordering::Greater) => false,
        _ => candidate.matched < current.matched,
    }
}

/// Caches the merged price table for per-request lookups.
pub struct PricingCache {
    pool: SqlitePool,
    inner: RwLock<Option<Arc<PricingIndex>>>,
}

impl PricingCache {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            inner: RwLock::new(None),
        }
    }

    /// Returns the merged index, loading it on first use.
    pub async fn snapshot(&self) -> Result<Arc<PricingIndex>> {
        if let Some(index) = self.inner.read().await.as_ref() {
            return Ok(index.clone());
        }

        let mut guard = self.inner.write().await;
        if let Some(index) = guard.as_ref() {
            return Ok(index.clone());
        }

        let rows = sqlx::query_as::<_, PricedModel>(
            "SELECT * FROM model_prices
             ORDER BY CASE source WHEN 'sync' THEN 0 ELSE 1 END",
        )
        .fetch_all(&self.pool)
        .await?;

        let index = Arc::new(PricingIndex::build(rows));
        *guard = Some(index.clone());
        Ok(index)
    }

    /// Drops the cache so the next lookup reloads it.
    pub async fn invalidate(&self) {
        *self.inner.write().await = None;
    }

    /// Price for one model, resolving by leaf when the exact id misses.
    pub async fn price_for(&self, model: &str) -> Option<Price> {
        self.match_for(model).await.map(|entry| entry.price)
    }

    /// Full match information, used by the dashboard's "test match" tool.
    pub async fn match_for(&self, model: &str) -> Option<PriceMatch> {
        self.snapshot().await.ok()?.find(model)
    }
}

/// Crawls `source_url`, replaces the synced table, and refreshes the cache.
pub async fn sync_from_source(cache: &PricingCache, source_url: &str) -> Result<PricingSyncStatus> {
    let response = reqwest::Client::new()
        .get(source_url)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|error| {
            Error::Upstream(format!("cannot fetch pricing from '{source_url}': {error}"))
        })?;

    if !response.status().is_success() {
        return Err(Error::Upstream(format!(
            "pricing source '{source_url}' returned {}",
            response.status()
        )));
    }

    let body = response
        .text()
        .await
        .map_err(|error| Error::Upstream(format!("cannot read the pricing payload: {error}")))?;
    let fetched = sources::parse(source_url, &body)?;

    let status = PricingSyncStatus {
        source: source_url.to_string(),
        synced_at: Utc::now(),
        model_count: fetched.len() as i64,
    };

    let repository = PricingRepository::new(cache.pool.clone());
    repository
        .replace_synced(&fetched, status.synced_at)
        .await?;
    repository.record_sync(&status).await?;
    cache.invalidate().await;

    Ok(status)
}
