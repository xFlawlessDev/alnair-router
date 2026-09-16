//! Public read-only endpoints for router-issued client keys.
//!
//! `/api/public/*` sits outside the admin-token guard: the caller proves who
//! they are with the same `Authorization: Bearer sk-router-…` key used on
//! `/v1`, and only ever sees its own rows. Access is controlled by
//! `server.public_usage`.

use axum::Json;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};

use chrono::{Datelike, Utc};

use crate::db::repos::api_keys::ApiKey;
use crate::db::repos::usage::{Bucket, KeySpend, ModelUsage, UsageBucket, UsageFilter, UsageSummary};
use crate::error::{Error, Result};
use crate::handlers::catalog::CatalogEntry;
use crate::middleware;
use crate::policy::KeyPolicy;
use crate::pricing::Price;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct UsageQuery {
    /// ISO-8601 lower bound; absent means all time.
    #[serde(default)]
    pub since: Option<chrono::DateTime<chrono::Utc>>,
    /// ISO-8601 upper bound, inclusive; used by the month selector.
    #[serde(default)]
    pub until: Option<chrono::DateTime<chrono::Utc>>,
    /// `hour` or `day`; defaults to `day`.
    #[serde(default)]
    pub bucket: Option<String>,
}

/// Who the caller is, so the page can show which key is connected.
#[derive(Debug, Serialize)]
pub struct KeyView {
    pub name: String,
    pub prefix: String,
}

/// Budget caps for a key, resolved from key + plan. Null means uncapped.
#[derive(Debug, Serialize)]
pub struct PublicBudgetCaps {
    pub daily_budget_usd: Option<f64>,
    pub weekly_budget_usd: Option<f64>,
    pub monthly_budget_usd: Option<f64>,
    pub lifetime_budget_usd: Option<f64>,
    pub daily_token_limit: Option<i64>,
    pub weekly_token_limit: Option<i64>,
    pub monthly_token_limit: Option<i64>,
    pub lifetime_token_limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct MyUsage {
    pub key: KeyView,
    pub since: Option<chrono::DateTime<chrono::Utc>>,
    pub until: Option<chrono::DateTime<chrono::Utc>>,
    pub bucket: &'static str,
    pub summary: UsageSummary,
    pub models: Vec<ModelUsage>,
    pub timeseries: Vec<UsageBucket>,
    pub spend: KeySpend,
    pub budget: PublicBudgetCaps,
}

/// `GET /api/public/usage` — the caller's own rollup, keyed by bearer key.
pub async fn usage(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<UsageQuery>,
) -> Result<Json<MyUsage>> {
    if !state.config_snapshot().server.public_usage {
        return Err(Error::Forbidden(
            "the self-service usage page is disabled".to_string(),
        ));
    }

    let bucket = Bucket::parse(query.bucket.as_deref())?;
    let key = authorize(&state, &headers).await?;
    let mut filter = UsageFilter::new(Some(key.id.clone()), None, None, None, query.since);
    filter.until = query.until;
    let summary = state.usage().summary(&filter).await?;
    let models = state.usage().models(&filter).await?;
    let timeseries = state.usage().timeseries(&filter, bucket).await?;

    // Resolve spend and budget caps (key + plan).
    let now = Utc::now();
    let daily_since = now.date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc();
    let weekly_since = (now - chrono::Duration::days(7)).date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc();
    let monthly_since = now.with_day(1).unwrap().date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc();

    let spend = state
        .usage()
        .spend_for_key(&key.id, daily_since, weekly_since, monthly_since)
        .await?;

    let plan = match &key.plan_id {
        Some(plan_id) => state.key_plans().get(plan_id).await?,
        None => None,
    };
    let policy = KeyPolicy::resolve(&key, plan.as_ref());

    Ok(Json(MyUsage {
        key: KeyView {
            name: key.name,
            prefix: key.prefix,
        },
        since: query.since,
        until: query.until,
        bucket: bucket.as_str(),
        summary,
        models,
        timeseries,
        spend,
        budget: PublicBudgetCaps {
            daily_budget_usd: policy.budgets.daily,
            weekly_budget_usd: policy.budgets.weekly,
            monthly_budget_usd: policy.budgets.monthly,
            lifetime_budget_usd: policy.budgets.lifetime,
            daily_token_limit: policy.token_limits.daily,
            weekly_token_limit: policy.token_limits.weekly,
            monthly_token_limit: policy.token_limits.monthly,
            lifetime_token_limit: policy.token_limits.lifetime,
        },
    }))
}

/// Customer-facing catalog row: no connection names or price provenance.
#[derive(Debug, Serialize)]
pub struct PublicCatalogEntry {
    pub id: String,
    pub kind: &'static str,
    pub tier: Option<usize>,
    pub upstream_model: Option<String>,
    pub price: Option<Price>,
}

#[derive(Debug, Serialize)]
pub struct PublicCatalog {
    /// The key's raw allowlist patterns; empty means every model is allowed.
    pub allowed_models: Vec<String>,
    pub data: Vec<PublicCatalogEntry>,
}

/// `GET /api/public/models` — the catalog rows this key may call.
pub async fn models(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<PublicCatalog>> {
    if !state.config_snapshot().server.public_usage {
        return Err(Error::Forbidden(
            "the self-service usage page is disabled".to_string(),
        ));
    }

    let key = authorize(&state, &headers).await?;
    let plan = match &key.plan_id {
        Some(plan_id) => state.key_plans().get(plan_id).await?,
        None => None,
    };
    let policy = KeyPolicy::resolve(&key, plan.as_ref());

    let data = CatalogEntry::collect(&state)
        .await?
        .into_iter()
        .filter(|entry| accessible(entry, &policy))
        .map(|entry| PublicCatalogEntry {
            id: entry.id,
            kind: entry.kind,
            tier: entry.tier,
            upstream_model: entry.upstream_model,
            price: entry.price,
        })
        .collect();

    Ok(Json(PublicCatalog {
        allowed_models: policy.allowed_models.clone(),
        data,
    }))
}

/// True when the key's allowlist can reach the catalog row.
fn accessible(entry: &CatalogEntry, policy: &KeyPolicy) -> bool {
    if policy.unrestricted() {
        return true;
    }

    match (entry.kind, &entry.upstream_model) {
        // A pinned alias is callable bare, or with any model segment.
        ("alias", Some(model)) => {
            policy.allows(&entry.id) || policy.allows(&format!("{}/{}", entry.id, model))
        }
        // An open alias is reachable when the allowlist names its prefix.
        ("alias", None) => {
            let prefix = format!("{}/", entry.id.to_lowercase());
            policy.allows(&prefix)
                || policy
                    .allowed_models
                    .iter()
                    .any(|pattern| pattern.trim().to_lowercase().starts_with(&prefix))
        }
        // Combos are called by name.
        _ => policy.allows(&entry.id),
    }
}

/// Resolves the bearer key without touching rate limits or last-used stamps.
async fn authorize(state: &AppState, headers: &HeaderMap) -> Result<ApiKey> {
    let secret = middleware::extract_bearer(headers).ok_or_else(|| {
        Error::Unauthorized("missing Authorization: Bearer <key> header".to_string())
    })?;

    match state.api_keys().find_by_secret(&secret).await? {
        Some(key) => Ok(key),
        None => Err(Error::Unauthorized(
            "invalid or disabled API key".to_string(),
        )),
    }
}
