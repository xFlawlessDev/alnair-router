//! Model reference resolution tests (pure, no database).

use std::collections::BTreeMap;

use alnair_router::Error;
use alnair_router::db::repos::aliases::Alias;
use alnair_router::db::repos::combos::{Combo, ComboEntry};
use alnair_router::db::repos::connections::Connection;
use alnair_router::model::Catalog;
use chrono::Utc;

fn connection(id: &str, name: &str, provider_type: &str, base_url: &str) -> Connection {
    Connection {
        id: id.to_string(),
        name: name.to_string(),
        provider_type: provider_type.to_string(),
        base_url: base_url.to_string(),
        api_key: Some(format!("key-{id}")),
        custom_headers: "{}".to_string(),
        enabled: 1,
        connect_timeout_ms: None,
        idle_timeout_ms: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn alias(prefix: &str, connection_id: &str, model_override: Option<&str>) -> Alias {
    Alias {
        id: format!("alias-{prefix}"),
        prefix: prefix.to_string(),
        connection_id: connection_id.to_string(),
        model_override: model_override.map(str::to_string),
        enabled: 1,
        sort_order: 0,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn combo(id: &str, name: &str) -> Combo {
    Combo {
        id: id.to_string(),
        name: name.to_string(),
        description: None,
        enabled: 1,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn entry(combo_id: &str, model_ref: &str, position: i64) -> ComboEntry {
    ComboEntry {
        id: format!("{combo_id}-{position}"),
        combo_id: combo_id.to_string(),
        model_ref: model_ref.to_string(),
        position,
        enabled: 1,
    }
}

/// Two connections (openai + anthropic) with `glm` and `kr` aliases,
/// one combo that chains them, and `openai-main` as the default.
fn catalog() -> Catalog {
    Catalog {
        connections: vec![
            connection(
                "c1",
                "openai-main",
                "openai-compatible",
                "https://openai.test/v1",
            ),
            connection(
                "c2",
                "anthropic-main",
                "anthropic-native",
                "https://anthropic.test/v1",
            ),
        ],
        aliases: vec![
            alias("glm", "c1", None),
            alias("kr", "c2", Some("claude-4.5-sonnet")),
        ],
        combos: vec![combo("combo1", "free forever")],
        combo_entries: vec![
            entry("combo1", "glm/glm-5.1", 0),
            entry("combo1", "kr/claude-4.5", 1),
        ],
    }
}

fn resolver(catalog: &Catalog) -> alnair_router::Resolver {
    catalog.resolver(Some("openai-main".to_string()), 5)
}

#[test]
fn alias_resolves_model_from_reference() {
    let catalog = catalog();
    let targets = resolver(&catalog).resolve("glm/glm-5.1").expect("resolve");

    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].provider_type, "openai-compatible");
    assert_eq!(targets[0].base_url, "https://openai.test/v1");
    assert_eq!(targets[0].model, "glm-5.1");
    assert_eq!(targets[0].api_key.as_deref(), Some("key-c1"));
    assert_eq!(targets[0].source, "alias:glm");
}

#[test]
fn alias_model_override_wins_over_reference_model() {
    let catalog = catalog();
    let targets = resolver(&catalog).resolve("kr/anything").expect("resolve");

    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].model, "claude-4.5-sonnet");
    assert_eq!(targets[0].provider_type, "anthropic-native");
}

#[test]
fn prefix_lookup_is_case_insensitive() {
    let catalog = catalog();
    let targets = resolver(&catalog).resolve("GLM/glm-5.1").expect("resolve");

    assert_eq!(targets[0].model, "glm-5.1");
    assert_eq!(targets[0].source, "alias:glm");
}

#[test]
fn bare_model_resolves_via_default_connection() {
    let catalog = catalog();
    let targets = resolver(&catalog).resolve("gpt-4o-mini").expect("resolve");

    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].model, "gpt-4o-mini");
    assert_eq!(targets[0].base_url, "https://openai.test/v1");
    assert_eq!(targets[0].source, "default:openai-main");
}

#[test]
fn combo_expands_into_ordered_fallback_chain() {
    let catalog = catalog();
    let targets = resolver(&catalog).resolve("free forever").expect("resolve");

    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0].base_url, "https://openai.test/v1");
    assert_eq!(targets[0].model, "glm-5.1");
    assert_eq!(targets[0].source, "combo:free forever#1");
    assert_eq!(targets[1].base_url, "https://anthropic.test/v1");
    assert_eq!(targets[1].model, "claude-4.5-sonnet");
    assert_eq!(targets[1].source, "combo:free forever#2");
}

#[test]
fn combo_name_lookup_is_case_insensitive() {
    let catalog = catalog();
    let targets = resolver(&catalog).resolve("Free Forever").expect("resolve");
    assert_eq!(targets.len(), 2);
}

#[test]
fn unknown_prefix_is_reported_as_unknown_model() {
    let catalog = catalog();
    let error = resolver(&catalog)
        .resolve("nope/some-model")
        .expect_err("must fail");

    assert!(matches!(error, Error::UnknownModel(ref value) if value == "nope/some-model"));
}

#[test]
fn bare_model_without_default_connection_fails() {
    let catalog = catalog();
    let resolver = catalog.resolver(None, 5);
    let error = resolver.resolve("gpt-4o").expect_err("must fail");

    assert!(matches!(error, Error::UnknownModel(_)));
}

#[test]
fn empty_reference_is_rejected() {
    let catalog = catalog();
    let error = resolver(&catalog).resolve("   ").expect_err("must fail");
    assert!(matches!(error, Error::BadRequest(_)));
}

#[test]
fn alias_with_empty_model_segment_is_rejected() {
    let catalog = catalog();
    let error = resolver(&catalog).resolve("glm/").expect_err("must fail");
    assert!(matches!(error, Error::BadRequest(_)));
}

#[test]
fn disabled_alias_is_not_resolvable() {
    let mut catalog = catalog();
    catalog.aliases[0].enabled = 0;
    let error = resolver(&catalog)
        .resolve("glm/glm-5.1")
        .expect_err("must fail");

    assert!(matches!(error, Error::UnknownModel(_)));
}

#[test]
fn disabled_connection_makes_alias_unroutable() {
    let mut catalog = catalog();
    catalog.connections[0].enabled = 0;
    let error = resolver(&catalog)
        .resolve("glm/glm-5.1")
        .expect_err("must fail");

    assert!(matches!(error, Error::NoRoute(_)));
}

#[test]
fn disabled_combo_is_not_resolvable() {
    let mut catalog = catalog();
    catalog.combos[0].enabled = 0;
    let error = resolver(&catalog)
        .resolve("free forever")
        .expect_err("must fail");

    // A known-but-disabled combo must not silently fall through to the
    // default connection as if it were a bare model name.
    assert!(matches!(error, Error::NoRoute(_)));
}

#[test]
fn combo_skips_disabled_entries() {
    let mut catalog = catalog();
    catalog.combo_entries[0].enabled = 0;
    let targets = resolver(&catalog).resolve("free forever").expect("resolve");

    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].source, "combo:free forever#2");
}

#[test]
fn nested_combos_are_flattened_in_order() {
    let mut catalog = catalog();
    catalog.combos.push(combo("combo2", "outer"));
    catalog
        .combo_entries
        .push(entry("combo2", "free forever", 0));
    catalog
        .combo_entries
        .push(entry("combo2", "glm/glm-5.1", 1));

    let targets = resolver(&catalog).resolve("outer").expect("resolve");

    // outer → [free forever (2), glm/glm-5.1] = 3 targets.
    assert_eq!(targets.len(), 3);
    assert_eq!(targets[0].model, "glm-5.1");
    assert_eq!(targets[1].model, "claude-4.5-sonnet");
    assert_eq!(targets[2].model, "glm-5.1");
}

#[test]
fn combo_cycle_is_broken_without_hanging() {
    let mut catalog = catalog();
    catalog.combos.push(combo("comboA", "a"));
    catalog.combos.push(combo("comboB", "b"));
    catalog.combo_entries.push(entry("comboA", "b", 0));
    catalog.combo_entries.push(entry("comboB", "a", 0));
    // Give the cycle a real leaf so resolution produces something.
    catalog
        .combo_entries
        .push(entry("comboA", "glm/glm-5.1", 1));

    let targets = resolver(&catalog).resolve("a").expect("resolve");

    // a → b → (a skipped, cycle) ; then a's own leaf.
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].model, "glm-5.1");
}

#[test]
fn combo_with_no_resolvable_entries_is_unknown() {
    let mut catalog = catalog();
    catalog.combos.push(combo("combo3", "empty"));
    let error = resolver(&catalog).resolve("empty").expect_err("must fail");

    assert!(matches!(error, Error::UnknownModel(_)));
}

#[test]
fn deep_nesting_beyond_max_depth_is_rejected() {
    // level0 → level1 → ... → level9, exceeding MAX_DEPTH (8).
    let mut catalog = Catalog::default();
    for level in 0..10 {
        let name = format!("level{level}");
        let mut record = combo(&format!("combo-{name}"), &name);
        record.id = format!("combo-{name}");
        catalog.combos.push(record);
    }
    for level in 0..9 {
        let from = format!("level{level}");
        let to = format!("level{}", level + 1);
        catalog
            .combo_entries
            .push(entry(&format!("combo-{from}"), &to, 0));
    }

    let resolver = catalog.resolver(Some("openai-main".to_string()), 5);
    let error = resolver.resolve("level0").expect_err("depth cap must trip");

    assert!(matches!(error, Error::BadRequest(_)));
}

#[test]
fn max_attempts_truncates_the_chain() {
    let mut catalog = catalog();
    catalog.combos.push(combo("combo4", "long"));
    for position in 0..8 {
        catalog
            .combo_entries
            .push(entry("combo4", "glm/glm-5.1", position));
    }

    let targets = catalog
        .resolver(Some("openai-main".to_string()), 3)
        .resolve("long")
        .expect("resolve");

    assert_eq!(targets.len(), 3);
}

#[test]
fn custom_headers_flow_into_target() {
    let mut catalog = catalog();
    let mut headers = BTreeMap::new();
    headers.insert("X-Org".to_string(), "acme".to_string());
    catalog.connections[0].custom_headers =
        serde_json::to_string(&headers).expect("serialize headers");

    let targets = resolver(&catalog).resolve("glm/glm-5.1").expect("resolve");
    assert_eq!(
        targets[0].custom_headers.get("X-Org").map(String::as_str),
        Some("acme")
    );
}

#[test]
fn malformed_custom_headers_yield_empty_map() {
    let mut catalog = catalog();
    catalog.connections[0].custom_headers = "not json".to_string();

    let targets = resolver(&catalog).resolve("glm/glm-5.1").expect("resolve");
    assert!(targets[0].custom_headers.is_empty());
}
