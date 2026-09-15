//! `GET /api/models` — the admin model catalog: provider, model id, price.

use axum::Json;
use axum::extract::State;
use serde::Serialize;
use serde_json::json;

use crate::error::Result;
use crate::pricing::Price;
use crate::state::AppState;

/// One routable model reference and the upstream target it lands on.
#[derive(Debug, Serialize)]
pub struct CatalogEntry {
    /// Model id to pass as `model` on `/v1` calls.
    pub id: String,
    /// `alias` or `combo`.
    pub kind: &'static str,
    /// Connection that serves this row.
    pub provider: String,
    pub provider_type: String,
    /// Concrete upstream model; `None` for aliases that accept any model.
    pub upstream_model: Option<String>,
    /// 1-based tier for combos; `None` for aliases.
    pub tier: Option<usize>,
    pub price: Option<Price>,
    /// Catalog key that answered, when it differs from the upstream model.
    pub price_matched: Option<String>,
    /// `override` or `sync`.
    pub price_source: Option<String>,
}

impl CatalogEntry {
    fn priced(mut self, found: Option<crate::pricing::PriceMatch>) -> Self {
        if let Some(found) = found {
            self.price = Some(found.price);
            self.price_matched = Some(found.matched);
            self.price_source = Some(found.source);
        }
        self
    }
}

/// Lists every enabled alias and combo with its provider and catalog price.
///
/// Combo names expand to one row per resolved tier, matching what a request
/// would actually try.
pub async fn models_catalog(State(state): State<AppState>) -> Result<Json<serde_json::Value>> {
    let snapshot = state.catalog_snapshot().await?;
    let catalog = &snapshot.catalog;
    let resolver = snapshot.resolver.as_ref();
    let mut data = Vec::new();

    for alias in catalog.aliases.iter().filter(|alias| alias.is_enabled()) {
        let Some(connection) = catalog
            .connections
            .iter()
            .find(|connection| connection.id == alias.connection_id && connection.is_enabled())
        else {
            continue;
        };

        let price = match &alias.model_override {
            Some(model) => state.pricing_cache.match_for(model).await,
            None => None,
        };

        data.push(
            CatalogEntry {
                id: alias.prefix.clone(),
                kind: "alias",
                provider: connection.name.clone(),
                provider_type: connection.provider_type.clone(),
                upstream_model: alias.model_override.clone(),
                tier: None,
                price: None,
                price_matched: None,
                price_source: None,
            }
            .priced(price),
        );
    }

    for combo in catalog.combos.iter().filter(|combo| combo.is_enabled()) {
        let targets = resolver.resolve(&combo.name).unwrap_or_default();
        for (index, target) in targets.iter().enumerate() {
            let pricing_key = target.pricing_model.as_deref().unwrap_or(&target.model);
            let price = state.pricing_cache.match_for(pricing_key).await;

            data.push(
                CatalogEntry {
                    id: combo.name.clone(),
                    kind: "combo",
                    provider: target.connection_name.clone(),
                    provider_type: target.provider_type.clone(),
                    upstream_model: Some(target.model.clone()),
                    tier: Some(index + 1),
                    price: None,
                    price_matched: None,
                    price_source: None,
                }
                .priced(price),
            );
        }
    }

    data.sort_by(|a, b| {
        a.id.to_ascii_lowercase()
            .cmp(&b.id.to_ascii_lowercase())
            .then(a.tier.cmp(&b.tier))
    });

    Ok(Json(json!({ "object": "list", "data": data })))
}
