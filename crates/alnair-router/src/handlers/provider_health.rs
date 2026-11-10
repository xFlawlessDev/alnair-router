//! Historical attempt aggregates for the provider health dashboard.

use axum::Json;
use axum::extract::{Query, State};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::error::{Error, Result};
use crate::state::AppState;

#[derive(Deserialize)]
pub struct HealthQuery {
    range: Option<String>,
}

#[derive(Serialize, FromRow)]
struct HealthRow {
    name: String,
    attempts: i64,
    failures: i64,
    later_attempts: i64,
    p95_ms: i64,
}

#[derive(Serialize)]
pub struct HealthResponse {
    since: DateTime<Utc>,
    providers: Vec<HealthRow>,
    models: Vec<HealthRow>,
    references: Vec<HealthRow>,
    overall: Vec<HealthRow>,
}

pub async fn history(
    State(state): State<AppState>,
    Query(query): Query<HealthQuery>,
) -> Result<Json<HealthResponse>> {
    let duration = match query.range.as_deref().unwrap_or("7d") {
        "live" => Duration::minutes(5),
        "1h" => Duration::hours(1),
        "6h" => Duration::hours(6),
        "24h" => Duration::hours(24),
        "7d" => Duration::days(7),
        "30d" => Duration::days(30),
        _ => return Err(Error::BadRequest("invalid health range".into())),
    };
    let since = Utc::now() - duration;
    let providers = aggregate(&state, "connection_name", since).await?;
    let models = aggregate(&state, "COALESCE(resolved_model, requested_model)", since).await?;
    let references = aggregate(&state, "requested_model", since).await?;
    let overall = aggregate(&state, "'all'", since).await?;
    Ok(Json(HealthResponse {
        since,
        providers,
        models,
        references,
        overall,
    }))
}

async fn aggregate(
    state: &AppState,
    expression: &str,
    since: DateTime<Utc>,
) -> Result<Vec<HealthRow>> {
    // `expression` is selected exclusively from the four constants above.
    let sql = format!(
        "WITH ranked AS (
           SELECT {expression} AS name, status, attempt, latency_ms,
             ROW_NUMBER() OVER (PARTITION BY {expression} ORDER BY latency_ms) AS rank,
             COUNT(*) OVER (PARTITION BY {expression}) AS total
           FROM usage_records WHERE created_at >= ? AND {expression} IS NOT NULL
         )
         SELECT name, COUNT(*) AS attempts,
           SUM(CASE WHEN status != 'ok' THEN 1 ELSE 0 END) AS failures,
           SUM(CASE WHEN attempt > 1 THEN 1 ELSE 0 END) AS later_attempts,
           MIN(CASE WHEN rank >= ((total * 95 + 99) / 100) THEN latency_ms END) AS p95_ms
         FROM ranked GROUP BY name ORDER BY name"
    );
    Ok(sqlx::query_as::<_, HealthRow>(&sql)
        .bind(since)
        .fetch_all(&state.pool)
        .await?)
}
