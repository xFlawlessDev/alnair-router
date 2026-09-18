//! Built-in provider presets for one-click API-key connections.
//!
//! Each preset pins the base URL, wire family and optional default headers for
//! a known upstream, so the dashboard can add a provider without asking for an
//! endpoint. URLs mirror the ones 9Router and OmniRoute ship; users can still
//! override every field after adding.
//!
//! Presets are grouped into three tiers ([`ProviderCategory`]) that the
//! dashboard renders as picker sections. The tables themselves live in
//! [`catalog`].

mod catalog;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use serde::Serialize;

use crate::db::repos::connections::SUPPORTED_PROVIDER_TYPES;

/// How a preset authenticates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderAuth {
    /// Needs an API key; stored encrypted in `connections.api_key`.
    ApiKey,
    /// Reachable without a key: a local server or an open gateway.
    None,
}

/// Tier a preset belongs to; drives the grouping in the dashboard picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCategory {
    /// Paid or pay-as-you-go endpoint behind a key.
    ApiKey,
    /// Hosted provider with a free tier.
    FreeTier,
    /// Server running on the user's own machine.
    Local,
}

impl ProviderCategory {
    /// Grouping order: API key, then free tier, then local.
    fn rank(self) -> u8 {
        match self {
            ProviderCategory::ApiKey => 0,
            ProviderCategory::FreeTier => 1,
            ProviderCategory::Local => 2,
        }
    }
}

/// A built-in upstream endpoint template.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderPreset {
    pub id: &'static str,
    pub label: &'static str,
    /// Wire family, such as `openai-compatible`, `anthropic-native`,
    /// `command-code` or `codebuddy-intl`.
    pub provider_type: &'static str,
    /// Upstream root, without the request path the provider layer appends.
    pub base_url: &'static str,
    pub category: ProviderCategory,
    pub auth: ProviderAuth,
    /// Headers merged into a new connection unless the user overrides them.
    pub default_headers: BTreeMap<&'static str, &'static str>,
    pub api_key_url: Option<&'static str>,
    pub docs_url: Option<&'static str>,
    pub note: Option<&'static str>,
}

/// A preset plus how many connections currently use it.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderPresetView {
    #[serde(flatten)]
    pub preset: ProviderPreset,
    pub configured: usize,
}

/// Every preset a user can pick: grouped by tier, alphabetically inside each.
pub fn presets() -> Vec<ProviderPreset> {
    let mut presets = catalog::api_key_presets();
    presets.extend(catalog::free_tier_presets());
    presets.extend(catalog::local_presets());
    presets.sort_by_key(|preset| (preset.category.rank(), preset.label));
    presets
}

/// Looks up one preset by id.
pub fn find(id: &str) -> Option<ProviderPreset> {
    let id = id.trim().to_ascii_lowercase();
    presets().into_iter().find(|preset| preset.id == id)
}

/// A key-based preset in the API-key tier.
fn api_key(
    id: &'static str,
    label: &'static str,
    provider_type: &'static str,
    base_url: &'static str,
    api_key_url: &'static str,
    note: Option<&'static str>,
) -> ProviderPreset {
    provider(
        id,
        label,
        provider_type,
        base_url,
        ProviderCategory::ApiKey,
        ProviderAuth::ApiKey,
        api_key_url,
        note,
    )
}

/// A key-based preset on a provider that also offers a free tier.
fn free_tier(
    id: &'static str,
    label: &'static str,
    provider_type: &'static str,
    base_url: &'static str,
    api_key_url: &'static str,
    note: Option<&'static str>,
) -> ProviderPreset {
    provider(
        id,
        label,
        provider_type,
        base_url,
        ProviderCategory::FreeTier,
        ProviderAuth::ApiKey,
        api_key_url,
        note,
    )
}

/// A server the user runs themselves; keyless and always OpenAI-compatible.
fn local(id: &'static str, label: &'static str, base_url: &'static str) -> ProviderPreset {
    provider(
        id,
        label,
        "openai-compatible",
        base_url,
        ProviderCategory::Local,
        ProviderAuth::None,
        "",
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn provider(
    id: &'static str,
    label: &'static str,
    provider_type: &'static str,
    base_url: &'static str,
    category: ProviderCategory,
    auth: ProviderAuth,
    api_key_url: &'static str,
    note: Option<&'static str>,
) -> ProviderPreset {
    debug_assert!(SUPPORTED_PROVIDER_TYPES.contains(&provider_type));

    ProviderPreset {
        id,
        label,
        provider_type,
        base_url,
        category,
        auth,
        default_headers: BTreeMap::new(),
        api_key_url: (!api_key_url.is_empty()).then_some(api_key_url),
        docs_url: None,
        note,
    }
}
