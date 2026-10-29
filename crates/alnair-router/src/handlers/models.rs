//! `GET /v1/models` — lists configured aliases and combos in OpenAI shape.

use axum::Json;
use axum::extract::State;
use serde::Serialize;

use crate::error::{Error, Result};
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct ModelList {
    pub object: &'static str,
    pub data: Vec<ModelObject>,
    /// Why a listed model may be missing or stale, when any entry came from a
    /// compiled-in catalog rather than the upstream. Absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notice: Option<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct ModelObject {
    pub id: String,
    pub object: &'static str,
    pub created: i64,
    pub owned_by: String,
    /// Router-specific provenance: `alias`, `combo`, or `builtin` for a model
    /// that came from a provider's compiled-in catalog.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub router_kind: Option<&'static str>,
}

/// Lists every model reference a caller can ask for.
///
/// Connections are deliberately absent: only a combo name, an alias prefix and
/// a bare model name resolve, so a connection name here would 404 at request
/// time. A connection becomes addressable only once an alias points at it.
///
/// A provider with a compiled-in catalog (ChatGPT/Codex) additionally lists its
/// models under the alias prefix that reaches it, because the backend publishes
/// no listing of its own. Those entries are a snapshot, so the reply carries the
/// catalog's notice.
pub async fn list_models(State(state): State<AppState>) -> Result<Json<ModelList>> {
    let catalog = state.catalog_snapshot().await?.catalog;
    let created = chrono::Utc::now().timestamp();

    let mut data = Vec::new();
    let mut notice: Option<&'static str> = None;

    for alias in catalog.aliases.iter().filter(|a| a.is_enabled()) {
        data.push(ModelObject {
            id: alias.prefix.clone(),
            object: "model",
            created,
            owned_by: "alnair-router".to_string(),
            router_kind: Some("alias"),
        });

        // The alias prefix is what a caller puts in front of the id
        // (`codex/gpt-5.5`), so the built-in models appear under it.
        let Some(connection) = catalog
            .connections
            .iter()
            .find(|connection| connection.id == alias.connection_id)
        else {
            continue;
        };
        let Some(models) = crate::providers::model_catalog::models_for(&connection.provider_type)
        else {
            continue;
        };

        notice = crate::providers::model_catalog::notice_for(&connection.provider_type);
        for model in models {
            data.push(ModelObject {
                id: format!("{}/{}", alias.prefix, model.id),
                object: "model",
                created,
                owned_by: "alnair-router".to_string(),
                router_kind: Some("builtin"),
            });
        }
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

    data.sort_by(|a, b| a.id.cmp(&b.id));

    Ok(Json(ModelList {
        object: "list",
        data,
        notice,
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
    let snapshot = state.catalog_snapshot().await?;
    let catalog = snapshot.catalog;
    let resolver = snapshot.resolver.as_ref();

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

/// `GET /v1/models/{id}` — one model object, as the OpenAI SDKs expect.
///
/// Ids are matched the same way the router resolves them: an alias prefix, a
/// combo name, or a `{alias prefix}/{builtin model}` reference. A connection
/// name is deliberately not addressable, so the reply stays consistent with
/// what a completion request would accept.
pub async fn retrieve_model(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<ModelObject>> {
    // The route is a catch-all so a built-in reference (`codex/gpt-5.5`) can be
    // retrieved; axum may hand back a leading slash on an empty segment.
    let id = id.trim_start_matches('/').to_string();
    let catalog = state.catalog_snapshot().await?.catalog;
    let created = chrono::Utc::now().timestamp();

    let alias = catalog
        .aliases
        .iter()
        .find(|a| a.is_enabled() && a.prefix == id);
    let combo = catalog
        .combos
        .iter()
        .find(|c| c.is_enabled() && c.name == id);

    let router_kind = match (&alias, &combo) {
        (Some(_), _) => "alias",
        (None, Some(_)) => "combo",
        (None, None) => {
            if is_builtin_reference(&catalog, &id) {
                "builtin"
            } else {
                return Err(Error::NotFound(format!(
                    "no alias, combo or built-in model named '{id}'"
                )));
            }
        }
    };

    Ok(Json(ModelObject {
        id,
        object: "model",
        created,
        owned_by: "alnair-router".to_string(),
        router_kind: Some(router_kind),
    }))
}

/// True when `id` is `{alias prefix}/{model}` and that model is in the
/// connection's compiled-in catalog.
fn is_builtin_reference(catalog: &crate::model::resolver::Catalog, id: &str) -> bool {
    let Some((prefix, model)) = id.split_once('/') else {
        return false;
    };
    let Some(alias) = catalog
        .aliases
        .iter()
        .find(|alias| alias.is_enabled() && alias.prefix == prefix)
    else {
        return false;
    };
    let Some(connection) = catalog
        .connections
        .iter()
        .find(|connection| connection.id == alias.connection_id)
    else {
        return false;
    };
    crate::providers::model_catalog::models_for(&connection.provider_type)
        .is_some_and(|models| models.iter().any(|entry| entry.id == model))
}
