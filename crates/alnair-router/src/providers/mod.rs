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
pub mod model_catalog;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use serde::Serialize;

use crate::db::repos::connections::supported_provider_types;

/// How a preset authenticates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderAuth {
    /// Needs an API key; stored encrypted in `connections.api_key`.
    ApiKey,
    /// Signed in with an OAuth account instead of a key; the connection has no
    /// `api_key` and the executor rotates `oauth_accounts`.
    ///
    /// The explicit rename matters: `rename_all = "snake_case"` renders this
    /// variant as `o_auth`, which would not match the dashboard's `"oauth"`.
    #[serde(rename = "oauth")]
    OAuth,
    /// Reachable without a key: a local server or an open gateway.
    None,
}

/// Tier a preset belongs to; drives the grouping in the dashboard picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCategory {
    /// Paid or pay-as-you-go endpoint behind a key.
    ApiKey,
    /// Subscription channel reached through an OAuth account.
    Subscription,
    /// Hosted provider with a free tier.
    FreeTier,
    /// Hosted web search API, served by `/v1/search` rather than chat.
    Search,
    /// Server running on the user's own machine.
    Local,
}

impl ProviderCategory {
    /// Grouping order: API key, subscription, free tier, search, then local.
    fn rank(self) -> u8 {
        match self {
            ProviderCategory::ApiKey => 0,
            ProviderCategory::Subscription => 1,
            ProviderCategory::FreeTier => 2,
            ProviderCategory::Search => 3,
            ProviderCategory::Local => 4,
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
    /// A warning the dashboard must show for a preset that borrows a
    /// first-party client identity.
    pub risk_notice: Option<&'static str>,
    /// The OAuth preset that signs this connection in, when `auth` is
    /// [`ProviderAuth::OAuth`]. The dashboard opens that login directly instead
    /// of asking for endpoints.
    pub oauth_preset: Option<&'static str>,
    /// Default `search_tier` for a new connection from this preset. Search
    /// providers also inherit this tier at routing time for older connections.
    pub default_search_tier: Option<i64>,
    /// Default `web_fetch_path` for a provider fetch, e.g. `/extract`.
    pub default_web_fetch_path: Option<&'static str>,
    /// Default `web_fetch_shape` matching that extract endpoint.
    pub default_web_fetch_shape: Option<&'static str>,
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
    presets.extend(catalog::subscription_presets());
    presets.extend(catalog::free_tier_presets());
    presets.extend(catalog::search_presets());
    presets.extend(catalog::local_presets());
    presets.sort_by_key(|preset| (preset.category.rank(), preset.label));
    presets
}

/// Looks up one preset by id.
pub fn find(id: &str) -> Option<ProviderPreset> {
    let id = id.trim().to_ascii_lowercase();
    presets().into_iter().find(|preset| preset.id == id)
}

pub(crate) fn search_preset(id: Option<&str>) -> Option<ProviderPreset> {
    id.and_then(find)
        .filter(|preset| preset.category == ProviderCategory::Search)
}

/// An OAuth-backed subscription channel: no key, one login.
fn subscription(
    id: &'static str,
    label: &'static str,
    provider_type: &'static str,
    base_url: &'static str,
    oauth_preset: &'static str,
    note: Option<&'static str>,
    risk_notice: Option<&'static str>,
) -> ProviderPreset {
    ProviderPreset {
        id,
        label,
        provider_type,
        base_url,
        category: ProviderCategory::Subscription,
        auth: ProviderAuth::OAuth,
        default_headers: BTreeMap::new(),
        api_key_url: None,
        docs_url: None,
        note,
        risk_notice,
        oauth_preset: Some(oauth_preset),
        default_search_tier: None,
        default_web_fetch_path: None,
        default_web_fetch_shape: None,
    }
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

/// A key-based web search API. Its base URL is the provider root, and the
/// router appends `/search` for search requests.
///
/// `extract` names the endpoint and body shape this provider serves for a
/// provider fetch, because `/v1/search` and `/v1/web/fetch` do not share a
/// contract across vendors.
fn search(
    id: &'static str,
    label: &'static str,
    base_url: &'static str,
    api_key_url: &'static str,
    note: Option<&'static str>,
    extract: Option<(&'static str, &'static str)>,
) -> ProviderPreset {
    let mut preset = provider(
        id,
        label,
        "openai-compatible",
        base_url,
        ProviderCategory::Search,
        ProviderAuth::ApiKey,
        api_key_url,
        note,
    );
    // A search provider is only reachable by `/v1/search` once it carries a
    // tier, so every search preset joins the first one.
    preset.default_search_tier = Some(1);
    if let Some((path, shape)) = extract {
        preset.default_web_fetch_path = Some(path);
        preset.default_web_fetch_shape = Some(shape);
    }
    preset
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
    debug_assert!(supported_provider_types().contains(&provider_type));

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
        risk_notice: None,
        oauth_preset: None,
        default_search_tier: None,
        default_web_fetch_path: None,
        default_web_fetch_shape: None,
    }
}
