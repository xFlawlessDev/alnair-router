//! Built-in provider presets for one-click API-key connections.
//!
//! Each preset pins the base URL, wire family and optional default headers for
//! a known upstream, so the dashboard can add a provider without asking for an
//! endpoint. URLs mirror the ones 9Router and OmniRoute ship; users can still
//! override every field after adding.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::db::repos::connections::SUPPORTED_PROVIDER_TYPES;

/// How a preset authenticates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderAuth {
    /// Needs an API key; stored encrypted in `connections.api_key`.
    ApiKey,
    /// Local server that usually runs without a key.
    None,
}

/// A built-in upstream endpoint template.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderPreset {
    pub id: &'static str,
    pub label: &'static str,
    /// Wire family: `openai-compatible` or `anthropic-native`.
    pub provider_type: &'static str,
    pub base_url: &'static str,
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

/// Every preset a user can pick, alphabetically by label.
pub fn presets() -> Vec<ProviderPreset> {
    let mut presets = vec![
        api_key(
            "anthropic",
            "Anthropic",
            "anthropic-native",
            "https://api.anthropic.com",
            "https://console.anthropic.com/settings/keys",
            None,
        ),
        api_key(
            "cerebras",
            "Cerebras",
            "openai-compatible",
            "https://api.cerebras.ai/v1",
            "https://cloud.cerebras.ai",
            None,
        ),
        api_key(
            "chutes",
            "Chutes",
            "openai-compatible",
            "https://llm.chutes.ai/v1",
            "https://chutes.ai/app/api",
            None,
        ),
        api_key(
            "cohere",
            "Cohere",
            "openai-compatible",
            "https://api.cohere.ai/compatibility/v1",
            "https://dashboard.cohere.com/api-keys",
            Some("OpenAI-compatibility endpoint."),
        ),
        api_key(
            "deepseek",
            "DeepSeek",
            "openai-compatible",
            "https://api.deepseek.com",
            "https://platform.deepseek.com/api_keys",
            None,
        ),
        api_key(
            "fireworks",
            "Fireworks AI",
            "openai-compatible",
            "https://api.fireworks.ai/inference/v1",
            "https://fireworks.ai/account/api-keys",
            None,
        ),
        api_key(
            "gemini",
            "Google Gemini",
            "openai-compatible",
            "https://generativelanguage.googleapis.com/v1beta/openai",
            "https://aistudio.google.com/app/apikey",
            Some("Gemini's OpenAI-compatibility endpoint."),
        ),
        api_key(
            "groq",
            "Groq",
            "openai-compatible",
            "https://api.groq.com/openai/v1",
            "https://console.groq.com/keys",
            None,
        ),
        api_key(
            "hyperbolic",
            "Hyperbolic",
            "openai-compatible",
            "https://api.hyperbolic.xyz/v1",
            "https://app.hyperbolic.xyz/settings",
            None,
        ),
        api_key(
            "mistral",
            "Mistral",
            "openai-compatible",
            "https://api.mistral.ai/v1",
            "https://console.mistral.ai/api-keys",
            None,
        ),
        api_key(
            "moonshot",
            "Moonshot (Kimi)",
            "openai-compatible",
            "https://api.moonshot.ai/v1",
            "https://platform.moonshot.ai/console/api-keys",
            None,
        ),
        api_key(
            "nebius",
            "Nebius AI Studio",
            "openai-compatible",
            "https://api.studio.nebius.ai/v1",
            "https://studio.nebius.com/settings/api-keys",
            None,
        ),
        api_key(
            "nvidia",
            "NVIDIA NIM",
            "openai-compatible",
            "https://integrate.api.nvidia.com/v1",
            "https://build.nvidia.com/settings/api-keys",
            None,
        ),
        api_key(
            "openai",
            "OpenAI",
            "openai-compatible",
            "https://api.openai.com/v1",
            "https://platform.openai.com/api-keys",
            None,
        ),
        api_key(
            "openrouter",
            "OpenRouter",
            "openai-compatible",
            "https://openrouter.ai/api/v1",
            "https://openrouter.ai/settings/keys",
            Some("Optional HTTP-Referer / X-Title headers can be added below."),
        ),
        api_key(
            "perplexity",
            "Perplexity",
            "openai-compatible",
            "https://api.perplexity.ai",
            "https://www.perplexity.ai/settings/api",
            None,
        ),
        api_key(
            "siliconflow",
            "SiliconFlow",
            "openai-compatible",
            "https://api.siliconflow.com/v1",
            "https://cloud.siliconflow.com/account/ak",
            None,
        ),
        api_key(
            "together",
            "Together AI",
            "openai-compatible",
            "https://api.together.xyz/v1",
            "https://api.together.xyz/settings/api-keys",
            None,
        ),
        api_key(
            "xai",
            "xAI (Grok)",
            "openai-compatible",
            "https://api.x.ai/v1",
            "https://console.x.ai",
            None,
        ),
        api_key(
            "zai",
            "Z.ai (GLM)",
            "openai-compatible",
            "https://api.z.ai/api/paas/v4",
            "https://z.ai/manage-apikey/apikey-list",
            Some("Coding-plan keys use a different endpoint; override the base URL."),
        ),
        local("lmstudio", "LM Studio", "http://localhost:1234/v1"),
        local("ollama", "Ollama", "http://localhost:11434/v1"),
        local("vllm", "vLLM", "http://localhost:8000/v1"),
    ];

    presets.sort_by_key(|preset| preset.label);
    presets
}

/// Looks up one preset by id.
pub fn find(id: &str) -> Option<ProviderPreset> {
    let id = id.trim().to_ascii_lowercase();
    presets().into_iter().find(|preset| preset.id == id)
}

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
        ProviderAuth::ApiKey,
        api_key_url,
        note,
    )
}

fn local(id: &'static str, label: &'static str, base_url: &'static str) -> ProviderPreset {
    provider(
        id,
        label,
        "openai-compatible",
        base_url,
        ProviderAuth::None,
        "",
        None,
    )
}

fn provider(
    id: &'static str,
    label: &'static str,
    provider_type: &'static str,
    base_url: &'static str,
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
        auth,
        default_headers: BTreeMap::new(),
        api_key_url: (!api_key_url.is_empty()).then_some(api_key_url),
        docs_url: None,
        note,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_unique_and_supported() {
        let presets = presets();
        assert!(presets.len() >= 20, "expected a real catalog");

        let mut ids: Vec<&str> = presets.iter().map(|preset| preset.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), presets.len(), "ids must be unique");

        for preset in &presets {
            assert!(
                SUPPORTED_PROVIDER_TYPES.contains(&preset.provider_type),
                "{} has an unsupported wire family",
                preset.id
            );
            assert!(
                preset.base_url.starts_with("http"),
                "{} needs a base URL",
                preset.id
            );
            if preset.auth == ProviderAuth::ApiKey {
                assert!(
                    preset.api_key_url.is_some(),
                    "{} needs a key link",
                    preset.id
                );
            }
        }
    }

    #[test]
    fn find_is_case_insensitive() {
        assert_eq!(find("OpenAI").expect("openai").id, "openai");
        assert_eq!(find("ollama").expect("ollama").auth, ProviderAuth::None);
        assert!(find("nope").is_none());
    }
}
