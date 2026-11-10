//! Public model metadata and user-supplied capability overrides.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    error::{Error, Result},
    state::AppState,
};

const CATALOG_URL: &str = "https://models.dev/api.json?type=all";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Capabilities {
    pub context: Option<u64>,
    pub reasoning: Option<bool>,
    pub tool_call: Option<bool>,
    pub reasoning_levels: Option<Vec<String>>,
    pub input_formats: Option<Vec<String>>,
    pub output_formats: Option<Vec<String>>,
}

#[derive(Serialize)]
struct CatalogEntry {
    provider: String,
    model: String,
    capabilities: Capabilities,
}

#[derive(Serialize)]
struct Override {
    scope: String,
    target_id: String,
    capabilities: Capabilities,
}

fn string_list(value: Option<&Value>) -> Option<Vec<String>> {
    value?
        .as_array()?
        .iter()
        .map(|item| item.as_str().map(str::to_string))
        .collect()
}

fn parse_catalog(value: &Value) -> Result<Vec<CatalogEntry>> {
    let providers = value
        .as_object()
        .ok_or_else(|| Error::Upstream("invalid models.dev catalog".into()))?;
    let mut entries = Vec::new();
    for (provider, data) in providers {
        let Some(models) = data.get("models").and_then(Value::as_object) else {
            continue;
        };
        for (model, data) in models {
            entries.push(CatalogEntry {
                provider: provider.clone(),
                model: model.clone(),
                capabilities: Capabilities {
                    context: data.pointer("/limit/context").and_then(Value::as_u64),
                    reasoning: data.get("reasoning").and_then(Value::as_bool),
                    tool_call: data.get("tool_call").and_then(Value::as_bool),
                    reasoning_levels: data
                        .get("reasoning_options")
                        .and_then(Value::as_array)
                        .and_then(|options| {
                            options.iter().find(|option| {
                                option.get("type").and_then(Value::as_str) == Some("effort")
                            })
                        })
                        .and_then(|option| string_list(option.get("values"))),
                    input_formats: string_list(data.pointer("/modalities/input")),
                    output_formats: string_list(data.pointer("/modalities/output")),
                },
            });
        }
    }
    if entries.is_empty() {
        return Err(Error::Upstream(
            "models.dev catalog contains no models".into(),
        ));
    }
    Ok(entries)
}

pub async fn list(State(state): State<AppState>) -> Result<Json<Value>> {
    let catalog: Option<(String, String)> =
        sqlx::query_as("SELECT catalog, updated_at FROM model_capability_catalog WHERE id = 1")
            .fetch_optional(&state.pool)
            .await?;
    let rows: Vec<(String, String, String)> =
        sqlx::query_as("SELECT scope, target_id, data FROM model_capability_overrides")
            .fetch_all(&state.pool)
            .await?;
    let overrides: Vec<Override> = rows
        .into_iter()
        .map(|(scope, target_id, data)| -> Result<_> {
            Ok(Override {
                scope,
                target_id,
                capabilities: serde_json::from_str(&data)
                    .map_err(|e| Error::Internal(e.to_string()))?,
            })
        })
        .collect::<Result<_>>()?;
    let (entries, updated_at) = match catalog {
        Some((data, updated_at)) => (
            serde_json::from_str::<Value>(&data).map_err(|e| Error::Internal(e.to_string()))?,
            Some(updated_at),
        ),
        None => (json!([]), None),
    };
    Ok(Json(
        json!({"entries": entries, "updated_at": updated_at, "overrides": overrides}),
    ))
}

pub async fn sync(State(state): State<AppState>) -> Result<Json<Value>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| Error::Upstream(e.to_string()))?;
    let response = client
        .get(CATALOG_URL)
        .send()
        .await
        .map_err(|e| Error::Upstream(format!("models.dev: {e}")))?
        .error_for_status()
        .map_err(|e| Error::Upstream(format!("models.dev: {e}")))?;
    if response
        .content_length()
        .is_some_and(|length| length > 25_000_000)
    {
        return Err(Error::Upstream("models.dev catalog too large".into()));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|e| Error::Upstream(format!("models.dev: {e}")))?;
    if bytes.len() > 25_000_000 {
        return Err(Error::Upstream("models.dev catalog too large".into()));
    }
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|e| Error::Upstream(format!("models.dev: {e}")))?;
    let entries = parse_catalog(&value)?;
    let data = serde_json::to_string(&entries).map_err(|e| Error::Internal(e.to_string()))?;
    let at = chrono::Utc::now().to_rfc3339();
    sqlx::query("INSERT INTO model_capability_catalog (id, catalog, updated_at) VALUES (1, ?, ?) ON CONFLICT(id) DO UPDATE SET catalog = excluded.catalog, updated_at = excluded.updated_at")
        .bind(data).bind(&at).execute(&state.pool).await?;
    Ok(Json(json!({"count": entries.len(), "updated_at": at})))
}

fn check_scope(scope: &str, id: &str) -> Result<()> {
    if !matches!(scope, "alias" | "combo") || id.trim().is_empty() {
        return Err(Error::BadRequest(
            "invalid capability override target".into(),
        ));
    }
    Ok(())
}

pub async fn save(
    State(state): State<AppState>,
    Path((scope, id)): Path<(String, String)>,
    Json(input): Json<Capabilities>,
) -> Result<StatusCode> {
    check_scope(&scope, &id)?;
    if input.context == Some(0) {
        return Err(Error::BadRequest("context must be positive".into()));
    }
    let exists: bool = match scope.as_str() {
        "alias" => {
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM aliases WHERE id = ?)")
                .bind(&id)
                .fetch_one(&state.pool)
                .await?
        }
        _ => {
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM combos WHERE id = ?)")
                .bind(&id)
                .fetch_one(&state.pool)
                .await?
        }
    };
    if !exists {
        return Err(Error::NotFound("capability override target".into()));
    }
    let data = serde_json::to_string(&input).map_err(|e| Error::Internal(e.to_string()))?;
    sqlx::query("INSERT INTO model_capability_overrides (scope, target_id, data) VALUES (?, ?, ?) ON CONFLICT(scope, target_id) DO UPDATE SET data = excluded.data")
        .bind(scope).bind(id).bind(data).execute(&state.pool).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove(
    State(state): State<AppState>,
    Path((scope, id)): Path<(String, String)>,
) -> Result<StatusCode> {
    check_scope(&scope, &id)?;
    sqlx::query("DELETE FROM model_capability_overrides WHERE scope = ? AND target_id = ?")
        .bind(scope)
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_known_fields_without_inventing_missing_values() {
        let entries = parse_catalog(&json!({"provider": {"models": {"model": {
            "limit": {"context": 1000000}, "reasoning": true, "tool_call": false,
            "reasoning_options": [{"type": "effort", "values": ["low", "high"]}],
            "modalities": {"input": ["text", "image"]}
        }}}}))
        .unwrap();
        assert_eq!(entries[0].capabilities.context, Some(1_000_000));
        assert_eq!(entries[0].capabilities.tool_call, Some(false));
        assert_eq!(
            entries[0].capabilities.reasoning_levels.as_ref().unwrap(),
            &["low", "high"]
        );
        assert_eq!(entries[0].capabilities.output_formats, None);
        assert!(parse_catalog(&json!({})).is_err());
    }

    #[tokio::test]
    async fn override_round_trips_and_can_be_removed() {
        let db = crate::Db::connect_in_memory().await.unwrap();
        let mut config = crate::RouterConfig::default();
        config.secrets.key = Some("ab".repeat(32));
        let state = AppState::new(config, db).unwrap();
        sqlx::query("INSERT INTO combos (id, name, enabled, created_at, updated_at) VALUES ('test', 'test', 1, datetime('now'), datetime('now'))")
            .execute(&state.pool).await.unwrap();
        let value = Capabilities {
            context: Some(128_000),
            tool_call: Some(false),
            ..Default::default()
        };
        save(
            State(state.clone()),
            Path(("combo".into(), "test".into())),
            Json(value),
        )
        .await
        .unwrap();
        let Json(result) = list(State(state.clone())).await.unwrap();
        assert_eq!(result["overrides"][0]["capabilities"]["context"], 128_000);
        assert_eq!(result["overrides"][0]["capabilities"]["tool_call"], false);
        remove(State(state.clone()), Path(("combo".into(), "test".into())))
            .await
            .unwrap();
        let Json(result) = list(State(state)).await.unwrap();
        assert!(result["overrides"].as_array().unwrap().is_empty());
    }
}
