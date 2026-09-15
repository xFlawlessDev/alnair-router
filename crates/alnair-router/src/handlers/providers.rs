//! `GET /api/providers` — built-in provider presets for one-click setup.

use std::collections::HashMap;

use axum::Json;
use axum::extract::State;
use serde_json::json;

use crate::error::Result;
use crate::providers::{ProviderPresetView, presets};
use crate::state::AppState;

/// Lists the preset catalog with how many connections use each preset.
pub async fn list_providers(State(state): State<AppState>) -> Result<Json<serde_json::Value>> {
    let connections = state.connections().list().await?;
    let mut counts: HashMap<String, usize> = HashMap::new();
    for connection in &connections {
        if let Some(id) = &connection.provider_id {
            *counts.entry(id.clone()).or_default() += 1;
        }
    }

    let data: Vec<ProviderPresetView> = presets()
        .into_iter()
        .map(|preset| {
            let configured = counts.get(preset.id).copied().unwrap_or(0);
            ProviderPresetView { preset, configured }
        })
        .collect();

    Ok(Json(json!({ "object": "list", "data": data })))
}
