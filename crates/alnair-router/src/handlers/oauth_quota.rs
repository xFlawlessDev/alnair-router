//! Account quota snapshots from the Codex usage endpoint.
use std::time::Duration;

use axum::{
    Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};

use crate::{
    error::{Error, Result},
    state::AppState,
};

const CODEX_USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";

#[derive(Debug, Deserialize, Serialize)]
pub struct QuotaWindow {
    pub used_percent: f64,
    pub limit_window_seconds: Option<u64>,
    pub reset_at: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct QuotaLimits {
    pub allowed: Option<bool>,
    pub limit_reached: Option<bool>,
    pub primary_window: Option<QuotaWindow>,
    pub secondary_window: Option<QuotaWindow>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AdditionalQuota {
    pub limit_name: Option<String>,
    pub metered_feature: Option<String>,
    pub rate_limit: Option<QuotaLimits>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct QuotaSnapshot {
    pub plan_type: Option<String>,
    pub rate_limit: Option<QuotaLimits>,
    #[serde(default, deserialize_with = "nullable_limits")]
    pub additional_rate_limits: Vec<AdditionalQuota>,
}

fn nullable_limits<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Vec<AdditionalQuota>, D::Error> {
    Ok(Option::<Vec<AdditionalQuota>>::deserialize(deserializer)?.unwrap_or_default())
}

pub async fn get_quota(
    State(state): State<AppState>,
    Path((connection_id, account_id)): Path<(String, String)>,
) -> Result<Json<QuotaSnapshot>> {
    let account = state
        .oauth_accounts()
        .get(&connection_id, &account_id)
        .await?
        .ok_or_else(|| Error::NotFound("OAuth account not found".to_string()))?;
    if account.provider_key != "openai-codex" {
        return Err(Error::BadRequest(
            "quota is not supported for this OAuth provider".to_string(),
        ));
    }
    let token = state.oauth_tokens.access_token(&account_id).await?;
    // Refresh may update the stored credential, so re-read the account identity.
    let account = state
        .oauth_accounts()
        .get(&connection_id, &account_id)
        .await?
        .ok_or_else(|| Error::NotFound("OAuth account not found".to_string()))?;
    let chatgpt_account_id = account
        .credential()
        .chatgpt_account_id
        .as_deref()
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| {
            Error::BadRequest("OAuth account has no ChatGPT account ID; sign in again".to_string())
        })?;
    Ok(Json(
        fetch_quota(CODEX_USAGE_URL, &token, chatgpt_account_id).await?,
    ))
}

async fn fetch_quota(url: &str, token: &str, account_id: &str) -> Result<QuotaSnapshot> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| Error::Internal("could not initialize quota client".to_string()))?;
    let response = client
        .get(url)
        .bearer_auth(token)
        .header("chatgpt-account-id", account_id)
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::USER_AGENT, "codex_cli_rs/0.159.0")
        .send()
        .await
        .map_err(|_| Error::Upstream("could not reach Codex quota service".to_string()))?;
    if !response.status().is_success() {
        return Err(Error::Upstream(format!(
            "Codex quota service returned {}",
            response.status()
        )));
    }
    let snapshot: QuotaSnapshot = response.json().await.map_err(|_| {
        Error::Upstream("Codex quota service returned an invalid response".to_string())
    })?;
    if snapshot.rate_limit.is_none() && snapshot.additional_rate_limits.is_empty() {
        return Err(Error::Upstream(
            "Codex quota service returned no rate limits".to_string(),
        ));
    }
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        http::{HeaderMap, StatusCode},
        routing::get,
    };

    #[tokio::test]
    async fn fetches_windows_with_account_authentication() {
        async fn handler(headers: HeaderMap) -> (StatusCode, Json<serde_json::Value>) {
            assert_eq!(headers["authorization"], "Bearer token");
            assert_eq!(headers["chatgpt-account-id"], "account");
            (
                StatusCode::OK,
                Json(serde_json::json!({"plan_type":"plus", "rate_limit": {
                    "primary_window": {"used_percent": 42, "limit_window_seconds": 18000, "reset_at": 1800000000},
                    "secondary_window": null
                }})),
            )
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, Router::new().route("/usage", get(handler)))
                .await
                .unwrap()
        });
        let result = fetch_quota(&format!("http://{addr}/usage"), "token", "account")
            .await
            .unwrap();
        assert_eq!(result.plan_type.as_deref(), Some("plus"));
        assert_eq!(
            result
                .rate_limit
                .unwrap()
                .primary_window
                .unwrap()
                .used_percent,
            42.0
        );
        task.abort();
    }
}
