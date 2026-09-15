//! `GET /v1/models` — lists configured aliases and combos in OpenAI shape.

use axum::Json;
use axum::extract::State;
use serde::Serialize;

use crate::error::Result;
use crate::model::Catalog;
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct ModelList {
    pub object: &'static str,
    pub data: Vec<ModelObject>,
}

#[derive(Debug, Serialize)]
pub struct ModelObject {
    pub id: String,
    pub object: &'static str,
    pub created: i64,
    pub owned_by: String,
    /// Router-specific provenance: `alias`, `combo`, or `connection`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub router_kind: Option<&'static str>,
}

/// Lists every model reference a caller can ask for.
pub async fn list_models(State(state): State<AppState>) -> Result<Json<ModelList>> {
    let catalog = Catalog::load(&state.pool, &state.cipher).await?;
    let created = chrono::Utc::now().timestamp();

    let mut data = Vec::new();

    for alias in catalog.aliases.iter().filter(|a| a.is_enabled()) {
        data.push(ModelObject {
            id: alias.prefix.clone(),
            object: "model",
            created,
            owned_by: "alnair-router".to_string(),
            router_kind: Some("alias"),
        });
    }

    for combo in catalog.combos.iter().filter(|c| c.is_enabled()) {
        data.push(ModelObject {
            id: combo.name.clone(),
            object: "model",
            created,
            owned_by: "alnair-router".to_string(),
            router_kind: Some("combo"),
        });
    }

    for connection in catalog.connections.iter().filter(|c| c.is_enabled()) {
        data.push(ModelObject {
            id: connection.name.clone(),
            object: "model",
            created,
            owned_by: "alnair-router".to_string(),
            router_kind: Some("connection"),
        });
    }

    data.sort_by(|a, b| a.id.cmp(&b.id));

    Ok(Json(ModelList {
        object: "list",
        data,
    }))
}

#[derive(Debug, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub router_kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tiers: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub struct ModelInfoList {
    pub object: &'static str,
    pub data: Vec<ModelInfo>,
}

/// Per-reference metadata for the configured aliases and combos.
pub async fn models_info(State(state): State<AppState>) -> Result<Json<ModelInfoList>> {
    let catalog = Catalog::load(&state.pool, &state.cipher).await?;
    let resolver = catalog.resolver(
        state.config.router.default_connection.clone(),
        state.config.router.max_attempts,
    );

    let mut data = Vec::new();

    for alias in catalog.aliases.iter().filter(|a| a.is_enabled()) {
        let connection = catalog
            .connections
            .iter()
            .find(|c| c.id == alias.connection_id);

        data.push(ModelInfo {
            id: alias.prefix.clone(),
            router_kind: "alias",
            provider_type: connection.map(|c| c.provider_type.clone()),
            base_url: connection.map(|c| c.base_url.clone()),
            upstream_model: alias.model_override.clone(),
            tiers: None,
        });
    }

    for combo in catalog.combos.iter().filter(|c| c.is_enabled()) {
        // Resolution reports the concrete models each tier maps to.
        let tiers = resolver
            .resolve(&combo.name)
            .map(|targets| targets.iter().map(|t| t.model.clone()).collect())
            .ok();

        data.push(ModelInfo {
            id: combo.name.clone(),
            router_kind: "combo",
            provider_type: None,
            base_url: None,
            upstream_model: None,
            tiers,
        });
    }

    data.sort_by(|a, b| a.id.cmp(&b.id));

    Ok(Json(ModelInfoList {
        object: "list",
        data,
    }))
}
