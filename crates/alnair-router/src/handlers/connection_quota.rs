//! API-key provider quota, separate from per-account OAuth quota.
use crate::{
    error::{Error, Result},
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Deserialize, Serialize)]
pub struct TavilyQuota {
    pub key: KeyUsage,
    pub account: AccountUsage,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct KeyUsage {
    pub usage: u64,
    pub limit: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AccountUsage {
    pub current_plan: Option<String>,
    pub plan_usage: u64,
    pub plan_limit: Option<u64>,
    pub paygo_usage: Option<u64>,
    pub paygo_limit: Option<u64>,
}

pub async fn get_quota(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<TavilyQuota>> {
    let connection = state
        .connections()
        .get(&id)
        .await?
        .ok_or_else(|| Error::NotFound("connection not found".to_string()))?;
    if connection.provider_id.as_deref() != Some("tavily") {
        return Err(Error::BadRequest(
            "quota is not supported for this API-key provider".to_string(),
        ));
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| Error::Internal("could not initialize quota client".to_string()))?;
    let request = client.get(format!(
        "{}/usage",
        connection.base_url.trim_end_matches('/')
    ));
    let response = crate::upstream::probe::apply_auth(request, &connection)
        .send()
        .await
        .map_err(|_| Error::Upstream("could not reach Tavily quota service".to_string()))?;
    if !response.status().is_success() {
        return Err(Error::Upstream(format!(
            "Tavily quota service returned {}",
            response.status()
        )));
    }
    let quota = response.json::<TavilyQuota>().await.map_err(|_| {
        Error::Upstream("Tavily quota service returned an invalid response".to_string())
    })?;
    Ok(Json(quota))
}
