//! Tests for the capability tier filter: rejection shape and failover.

use super::*;
use crate::upstream::chat_backend::{ResponseFormat, ToolChoice};

fn target(provider_type: &str, wire_api: &str, source: &str) -> ResolvedTarget {
    ResolvedTarget {
        connection_id: format!("c-{source}"),
        connection_name: source.to_string(),
        provider_type: provider_type.to_string(),
        base_url: "https://example.test/v1".to_string(),
        search_path: None,
        search_tier: None,
        model: "some-model".to_string(),
        api_keys: vec!["sk-test".to_string()],
        custom_headers: Default::default(),
        connect_timeout_ms: None,
        idle_timeout_ms: None,
        pricing_model: None,
        cache_retention: "none".to_string(),
        auth_style: "api_key".to_string(),
        wire_api: wire_api.to_string(),
        oauth_account_ids: Vec::new(),
        source: source.to_string(),
    }
}

fn schema_options() -> GenerationOptions {
    GenerationOptions {
        response_format: Some(ResponseFormat::JsonSchema {
            name: "ticket".to_string(),
            schema: serde_json::json!({ "type": "object" }),
            strict: true,
        }),
        ..Default::default()
    }
}

#[test]
fn a_bare_request_keeps_every_tier() {
    let targets = vec![
        target("anthropic-native", "chat", "t1"),
        target("openai-compatible", "chat", "t2"),
    ];

    let kept = filter_targets(targets, &GenerationOptions::default()).expect("kept");
    assert_eq!(kept.len(), 2);
}

#[test]
fn a_request_no_tier_can_serve_is_a_400_naming_the_parameter() {
    let targets = vec![target("anthropic-native", "chat", "combo:claude#1")];

    let error = filter_targets(targets, &schema_options()).expect_err("refused");

    assert!(
        matches!(error, Error::UnsupportedParameter { ref param, .. } if param == "response_format"),
        "expected UnsupportedParameter, got {error:?}"
    );
    assert_eq!(error.status(), axum::http::StatusCode::BAD_REQUEST);

    let message = error.to_string();
    assert!(message.contains("response_format"), "{message}");
    assert!(
        message.contains("ticket"),
        "the message must name the schema the client asked for: {message}"
    );
    assert!(
        message.contains("anthropic-native"),
        "the message must name the protocol that refused: {message}"
    );
}

#[test]
fn failover_reaches_a_tier_that_can_honour_the_request() {
    // Tier 1 cannot do structured output; tier 2 can. The request must succeed
    // on tier 2 rather than 400, which is the whole point of filtering after
    // resolution instead of at parse time.
    let targets = vec![
        target("anthropic-native", "chat", "combo:mix#1"),
        target("openai-compatible", "chat", "combo:mix#2"),
    ];

    let kept = filter_targets(targets, &schema_options()).expect("served by tier 2");

    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].source, "combo:mix#2");
}

#[test]
fn a_parameter_every_tier_accepts_is_left_alone() {
    let options = GenerationOptions {
        temperature: Some(0.3),
        max_tokens: Some(64),
        thinking_level: Some("low".to_string()),
        ..Default::default()
    };
    let targets = vec![
        target("anthropic-native", "chat", "t1"),
        target("openai-codex", "responses", "t2"),
    ];

    let kept = filter_targets(targets, &options).expect("kept");
    assert_eq!(kept.len(), 2);
}

#[test]
fn seed_is_refused_only_where_it_has_no_meaning() {
    let options = GenerationOptions {
        seed: Some(7),
        ..Default::default()
    };

    // Chat completions defines `seed`.
    let chat = vec![target("openai-compatible", "chat", "t1")];
    assert_eq!(filter_targets(chat, &options).expect("kept").len(), 1);

    // Responses does not, and Anthropic does not either.
    let responses = vec![target("openai-compatible", "responses", "t1")];
    let error = filter_targets(responses, &options).expect_err("refused");
    assert!(matches!(
        error,
        Error::UnsupportedParameter { ref param, .. } if param == "seed"
    ));
}

#[test]
fn an_unknown_provider_type_is_not_swallowed_by_the_filter() {
    // A malformed provider type is a configuration fault the executor reports
    // loudly; the filter must not hide it by silently dropping the tier.
    let targets = vec![target("ollama", "chat", "t1")];
    let options = schema_options();

    let kept = filter_targets(targets, &options).expect("left to the executor");
    assert_eq!(kept.len(), 1);
}

#[test]
fn a_named_tool_choice_survives_on_a_protocol_that_can_spell_it() {
    let options = GenerationOptions {
        tool_choice: Some(ToolChoice::Named {
            name: "search".to_string(),
        }),
        ..Default::default()
    };
    let targets = vec![target("anthropic-native", "chat", "t1")];

    assert_eq!(filter_targets(targets, &options).expect("kept").len(), 1);
}

#[test]
fn store_is_refused_on_chat_and_codex_but_not_on_responses() {
    let options = GenerationOptions {
        store: Some(true),
        ..Default::default()
    };

    let responses = vec![target("openai-compatible", "responses", "t1")];
    assert_eq!(filter_targets(responses, &options).expect("kept").len(), 1);

    for (provider, wire) in [("openai-compatible", "chat"), ("openai-codex", "responses")] {
        let targets = vec![target(provider, wire, "t1")];
        let error = filter_targets(targets, &options).expect_err("refused");
        assert!(
            matches!(error, Error::UnsupportedParameter { ref param, .. } if param == "store"),
            "{provider}/{wire} must refuse `store`, got {error:?}"
        );
    }
}
