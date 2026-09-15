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

use crate::db::repos::api_keys::ApiKey;
use crate::db::repos::usage::{Bucket, ModelUsage, UsageBucket, UsageFilter, UsageSummary};
use crate::error::{Error, Result};
use crate::middleware;
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

#[derive(Debug, Serialize)]
pub struct MyUsage {
    pub key: KeyView,
    pub since: Option<chrono::DateTime<chrono::Utc>>,
    pub until: Option<chrono::DateTime<chrono::Utc>>,
    pub bucket: &'static str,
    pub summary: UsageSummary,
    pub models: Vec<ModelUsage>,
    pub timeseries: Vec<UsageBucket>,
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
    let mut filter = UsageFilter::new(Some(key.id), None, None, None, query.since);
    filter.until = query.until;
    let summary = state.usage().summary(&filter).await?;
    let models = state.usage().models(&filter).await?;
    let timeseries = state.usage().timeseries(&filter, bucket).await?;

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
    }))
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
