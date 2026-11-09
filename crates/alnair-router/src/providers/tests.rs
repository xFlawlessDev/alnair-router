use super::*;

#[test]
fn presets_are_unique_and_supported() {
    let presets = presets();
    assert!(presets.len() >= 50, "expected a real catalog");

    let mut ids: Vec<&str> = presets.iter().map(|preset| preset.id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), presets.len(), "ids must be unique");

    for preset in &presets {
        assert!(
            supported_provider_types().contains(&preset.provider_type),
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
        if preset.auth != ProviderAuth::OAuth {
            assert!(
                preset.oauth_preset.is_none(),
                "{} is not an OAuth login",
                preset.id
            );
        }
    }
}

#[test]
fn find_is_case_insensitive() {
    assert_eq!(find("OpenAI").expect("openai").id, "openai");
    assert_eq!(find("ollama").expect("ollama").auth, ProviderAuth::None);
    assert!(find("opencode-free").is_none());
    let codebuddy = find("CODEBUDDY-INTL").expect("codebuddy-intl");
    assert_eq!(codebuddy.provider_type, "codebuddy-intl");
    assert_eq!(codebuddy.base_url, "https://www.codebuddy.ai/v2");
    assert_eq!(codebuddy.auth, ProviderAuth::ApiKey);
    assert!(find("nope").is_none());
}

/// Presets are alphabetical inside their tier, and the tiers keep their order.
#[test]
fn presets_group_by_category_in_order() {
    let presets = presets();

    let ranks: Vec<u8> = presets
        .iter()
        .map(|preset| preset.category.rank())
        .collect();
    assert!(
        ranks.windows(2).all(|pair| pair[0] <= pair[1]),
        "categories must not interleave"
    );

    for chunk in presets.chunk_by(|a, b| a.category == b.category) {
        assert!(
            chunk.windows(2).all(|pair| pair[0].label <= pair[1].label),
            "labels must be sorted inside a category"
        );
    }
}

/// Search providers are key-based presets that the router reaches through
/// `/v1/search`, so each must be a hosted endpoint with a key link.
#[test]
fn search_presets_are_key_based_hosted_endpoints() {
    let search: Vec<_> = presets()
        .into_iter()
        .filter(|preset| preset.category == ProviderCategory::Search)
        .collect();

    for id in ["exa", "firecrawl", "tavily"] {
        let preset = search
            .iter()
            .find(|preset| preset.id == id)
            .unwrap_or_else(|| panic!("{id} should be a search preset"));
        assert_eq!(preset.auth, ProviderAuth::ApiKey);
        assert!(preset.api_key_url.is_some(), "{id} needs a key link");
        assert!(preset.base_url.starts_with("https://"), "{id} needs https");
    }
}

/// Only the presets whose endpoint is account-specific may ship a placeholder.
#[test]
fn only_templated_presets_contain_placeholders() {
    for preset in presets() {
        let templated = preset.base_url.contains('<') || preset.base_url.contains('>');
        let expected = matches!(preset.id, "azure-openai" | "cloudflare");
        assert_eq!(
            templated, expected,
            "{} has an unexpected placeholder",
            preset.id
        );
    }
}

#[test]
fn every_tier_is_populated() {
    let presets = presets();

    for category in [
        ProviderCategory::ApiKey,
        ProviderCategory::Subscription,
        ProviderCategory::FreeTier,
        ProviderCategory::Local,
    ] {
        assert!(
            presets.iter().any(|preset| preset.category == category),
            "{category:?} has no presets"
        );
    }
}

/// An OAuth-backed preset carries no key, names the login it uses, and warns
/// about any borrowed client identity.
#[test]
fn oauth_presets_name_their_login_and_carry_no_key() {
    let oauth_presets: Vec<_> = presets()
        .into_iter()
        .filter(|preset| preset.auth == ProviderAuth::OAuth)
        .collect();
    assert!(!oauth_presets.is_empty(), "expected an OAuth preset");

    for preset in oauth_presets {
        assert!(
            preset.oauth_preset.is_some(),
            "{} must name its OAuth preset",
            preset.id
        );
        assert!(
            preset.api_key_url.is_none(),
            "{} takes no API key",
            preset.id
        );
        assert_eq!(
            preset.category,
            ProviderCategory::Subscription,
            "{} belongs in the subscription tier",
            preset.id
        );

        // The named login must exist and describe the same wire family.
        let login = crate::oauth::presets::find(preset.oauth_preset.unwrap());
        assert_eq!(login.provider_type, Some(preset.provider_type));
        if crate::oauth::presets::pins_client(&login) {
            assert!(
                preset.risk_notice.is_some(),
                "{} borrows a client identity and must warn about it",
                preset.id
            );
        }
    }
}

/// The dashboard switches on these exact strings; `snake_case` alone turns
/// `OAuth` into `o_auth`, which silently disabled the sign-in UI.
#[test]
fn auth_variants_serialize_to_dashboard_strings() {
    for (auth, expected) in [
        (ProviderAuth::ApiKey, "api_key"),
        (ProviderAuth::OAuth, "oauth"),
        (ProviderAuth::None, "none"),
    ] {
        assert_eq!(
            serde_json::to_string(&auth).expect("serialize"),
            format!("\"{expected}\"")
        );
    }
}
