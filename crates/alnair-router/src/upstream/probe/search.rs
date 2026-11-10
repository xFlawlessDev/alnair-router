//! Search providers expose usage or search APIs rather than a model list.
use std::time::Instant;

use super::{PROBE_CONNECT_TIMEOUT, PROBE_TIMEOUT, ProbeOutcome, apply_auth, response_serves_html};
use crate::{
    db::repos::connections::Connection,
    error::{Error, Result},
};

pub(crate) fn is_search(connection: &Connection) -> bool {
    connection.search_tier.is_some()
        || connection.search_path.is_some()
        || matches!(
            connection.provider_id.as_deref(),
            Some("tavily" | "exa" | "firecrawl")
        )
}

pub(super) async fn check(connection: &Connection) -> Result<ProbeOutcome> {
    let base = connection.base_url.trim_end_matches('/');
    let client = reqwest::Client::builder()
        .connect_timeout(PROBE_CONNECT_TIMEOUT)
        .timeout(PROBE_TIMEOUT)
        .build()
        .map_err(|error| Error::Internal(error.to_string()))?;
    let request = match connection.provider_id.as_deref() {
        Some("tavily") => client.get(format!("{base}/usage")),
        Some("firecrawl") => client.get(format!("{base}/team/credit-usage")),
        Some("exa") => client
            .post(format!(
                "{base}{}",
                connection.search_path.as_deref().unwrap_or("/search")
            ))
            .json(&serde_json::json!({"query":"connection health check", "numResults":1})),
        _ => client
            .post(format!(
                "{base}{}",
                connection.search_path.as_deref().unwrap_or("/search")
            ))
            .json(&serde_json::json!({"query":"connection health check", "max_results":1})),
    };
    let started = Instant::now();
    let response = apply_auth(request, connection).send().await.map_err(|_| {
        Error::Upstream("search provider check could not reach the upstream".to_string())
    })?;
    let status = response.status();
    if !status.is_success() {
        return Err(Error::Upstream(format!(
            "search provider check returned {status}"
        )));
    }
    if response_serves_html(&response) {
        return Err(Error::Upstream(
            "search provider check returned HTML instead of API data".to_string(),
        ));
    }
    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|_| Error::Upstream("search provider check returned invalid JSON".to_string()))?;
    let valid = match connection.provider_id.as_deref() {
        Some("tavily") => {
            body.get("key").is_some_and(serde_json::Value::is_object)
                && body
                    .get("account")
                    .is_some_and(serde_json::Value::is_object)
        }
        Some("firecrawl") => {
            body["success"] == true && body.get("data").is_some_and(serde_json::Value::is_object)
        }
        _ => {
            body.get("results").is_some_and(serde_json::Value::is_array)
                || body.get("data").is_some_and(serde_json::Value::is_object)
        }
    };
    if !valid {
        return Err(Error::Upstream(
            "search provider check returned an unexpected API payload".to_string(),
        ));
    }
    Ok(ProbeOutcome {
        models: Vec::new(),
        latency_ms: started.elapsed().as_millis() as u64,
        enumerable: false,
        notice: None,
    })
}
