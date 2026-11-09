//! Tests for the provider seam: dispatch, credential plumbing, and chunk mapping.

use super::*;

#[test]
fn codebuddy_intl_provider_type_is_dispatchable() {
    assert_eq!(
        provider_type_from_str("codebuddy-intl").expect("provider type"),
        ProviderType::CodeBuddyIntl
    );
}

#[test]
fn codex_is_a_dispatchable_provider_type() {
    assert_eq!(
        provider_type_from_str("openai-codex").expect("provider type"),
        ProviderType::OpenAiCodex
    );
}

/// Builds an unsigned JWT whose payload carries `claims`. Only the claim is ever
/// read, so the signature segment is never verified.
fn jwt_with(claims: serde_json::Value) -> String {
    use base64::Engine;
    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    format!(
        "{}.{}.{}",
        engine.encode(r#"{"alg":"none"}"#),
        engine.encode(claims.to_string()),
        engine.encode("signature")
    )
}

#[test]
fn the_chatgpt_account_id_is_read_from_the_claim_namespace() {
    let token = jwt_with(serde_json::json!({
        "https://api.openai.com/auth": { "chatgpt_account_id": "acct-123" },
        "sub": "user-1"
    }));

    assert_eq!(
        chatgpt_account_id_from_jwt(&token).as_deref(),
        Some("acct-123")
    );
}

#[test]
fn a_token_without_the_claim_yields_no_account_id() {
    assert!(chatgpt_account_id_from_jwt("not-a-jwt").is_none());
    assert!(chatgpt_account_id_from_jwt("a.b").is_none());
    assert!(chatgpt_account_id_from_jwt("a.b.c.d").is_none());
    assert!(
        chatgpt_account_id_from_jwt(&jwt_with(serde_json::json!({ "sub": "user-1" }))).is_none()
    );
    // An empty id is as unusable as an absent one.
    assert!(
        chatgpt_account_id_from_jwt(&jwt_with(serde_json::json!({
            "https://api.openai.com/auth": { "chatgpt_account_id": "   " }
        })))
        .is_none()
    );
}

#[test]
fn codex_requests_carry_the_account_id_and_thinking_support() {
    let token = jwt_with(serde_json::json!({
        "https://api.openai.com/auth": { "chatgpt_account_id": "acct-9" }
    }));
    let config = model_config(
        ProviderType::OpenAiCodex,
        "https://chatgpt.com/backend-api/codex",
        "gpt-5.5",
        Some(&token),
        AuthStyle::Bearer,
        WireApi::Chat,
        BTreeMap::new(),
        None,
        false,
    );

    assert_eq!(config.chatgpt_account_id.as_deref(), Some("acct-9"));
    assert!(config.supports_thinking, "Codex serves a reasoning dial");

    // No other provider derives an account id, even from a Codex-shaped key.
    let other = model_config(
        ProviderType::OpenaiCompatible,
        "https://api.openai.com/v1",
        "gpt-5.5",
        Some(&token),
        AuthStyle::Bearer,
        WireApi::Chat,
        BTreeMap::new(),
        None,
        false,
    );
    assert!(other.chatgpt_account_id.is_none());
}

fn media_message() -> RouterMessage {
    message_parts(
        "user",
        vec![
            MessagePart::Text("apa ini?".to_string()),
            MessagePart::Media(serde_json::json!({
                "type": "input_audio",
                "input_audio": { "data": "UklGRg==", "format": "wav" }
            })),
        ],
    )
}

#[test]
fn media_parts_are_visible_to_the_provider_gate() {
    let messages = vec![media_message()];
    assert_eq!(
        first_media_part_kind(&messages).as_deref(),
        Some("input_audio")
    );

    let text_only = vec![message_text("user", "hello")];
    assert!(first_media_part_kind(&text_only).is_none());
}

#[test]
fn media_parts_are_rejected_on_a_non_openai_provider() {
    let result = stream(
        std::sync::Arc::new(default_registry()),
        "anthropic-native",
        "https://api.anthropic.com/v1",
        "claude-sonnet-4",
        vec![media_message()],
        Some("sk-ant"),
        None,
        RetryPolicy::default(),
        true,
        None,
        BTreeMap::new(),
        None,
        CacheRetention::None,
        AuthStyle::ApiKey,
        WireApi::Chat,
    );

    let error = match result {
        Ok(_) => panic!("media must not reach an anthropic connection"),
        Err(error) => error,
    };
    let text = error.to_string();
    assert!(text.contains("input_audio"), "{text}");
    assert!(text.contains("openai-compatible"), "{text}");
}

#[test]
fn web_search_options_are_rejected_on_a_non_openai_provider() {
    let options = GenerationOptions {
        web_search_options: Some(serde_json::json!({ "search_context_size": "low" })),
        ..Default::default()
    };

    let result = stream(
        std::sync::Arc::new(default_registry()),
        "anthropic-native",
        "https://api.anthropic.com/v1",
        "claude-sonnet-4",
        vec![message_text("user", "hello")],
        Some("sk-ant"),
        Some(&options),
        RetryPolicy::default(),
        true,
        None,
        BTreeMap::new(),
        None,
        CacheRetention::None,
        AuthStyle::ApiKey,
        WireApi::Chat,
    );

    let error = match result {
        Ok(_) => panic!("web_search_options must not reach an anthropic connection"),
        Err(error) => error,
    };
    let text = error.to_string();
    assert!(text.contains("web_search_options"), "{text}");
    assert!(text.contains("openai-compatible"), "{text}");
}

#[test]
fn retry_policy_maps_into_provider_options() {
    let policy = RetryPolicy {
        max_retries_per_tier: 7,
        max_retry_delay_ms: 1_234,
    };

    let options = GenerationOptions::default().to_stream_options(policy);

    assert_eq!(options.max_retries, 7);
    assert_eq!(options.max_retry_delay_ms, 1_234);
}

#[test]
fn retry_policy_default_is_conservative() {
    let policy = RetryPolicy::default();
    assert_eq!(policy.max_retries_per_tier, 2);
    assert_eq!(policy.max_retry_delay_ms, 30_000);
}

#[test]
fn retry_policy_applies_even_without_generation_options() {
    let policy = RetryPolicy {
        max_retries_per_tier: 3,
        max_retry_delay_ms: 456,
    };

    let options = stream_options(None, policy, CacheRetention::None);

    assert_eq!(options.max_retries, 3);
    assert_eq!(options.max_retry_delay_ms, 456);
}

#[test]
fn cache_retention_maps_onto_the_provider_enum() {
    assert_eq!(cache_retention_from_str("none"), CacheRetention::None);
    assert_eq!(cache_retention_from_str("short"), CacheRetention::Short);
    assert_eq!(cache_retention_from_str("long"), CacheRetention::Long);
    assert_eq!(cache_retention_from_str("bogus"), CacheRetention::None);
}

#[test]
fn wire_api_maps_onto_the_provider_enum() {
    assert_eq!(wire_api_from_str("chat"), WireApi::Chat);
    assert_eq!(wire_api_from_str("responses"), WireApi::Responses);
    assert_eq!(
        wire_api_from_str("bogus"),
        WireApi::Chat,
        "an unknown value keeps today's /chat/completions behaviour"
    );
}

#[test]
fn an_openai_compatible_connection_can_carry_reasoning_effort() {
    // Without this gate the effort a client sends is silently dropped for the
    // whole family, which is the largest one.
    let config = model_config(
        ProviderType::OpenaiCompatible,
        "https://api.openai.com/v1",
        "gpt-5.1",
        Some("sk-test"),
        AuthStyle::Bearer,
        WireApi::Chat,
        BTreeMap::new(),
        None,
        false,
    );

    assert!(config.supports_thinking);
}

#[test]
fn effort_tiers_reach_the_provider_including_xhigh_and_max() {
    // `xhigh` and `max` are two distinct OpenAI levels; both land on the
    // provider's top tier, and each dialect then spells it for its endpoint.
    // Asserted through the serialized wire value, because that is the contract.
    for (input, expected) in [
        ("minimal", "minimal"),
        ("low", "low"),
        ("medium", "medium"),
        ("high", "high"),
        ("xhigh", "max"),
        ("max", "max"),
    ] {
        let options = GenerationOptions {
            thinking_level: Some(input.to_string()),
            ..Default::default()
        }
        .to_stream_options(RetryPolicy::default());

        assert_eq!(
            serde_json::to_value(options.thinking_level).expect("serialize"),
            serde_json::json!(expected),
            "tier {input} must reach the provider as {expected}"
        );
    }

    let unknown = GenerationOptions {
        thinking_level: Some("paranoid".to_string()),
        ..Default::default()
    }
    .to_stream_options(RetryPolicy::default());
    assert!(
        unknown.thinking_level.is_none(),
        "an unknown tier sends no effort rather than guessing"
    );
}

#[test]
fn stream_options_carry_the_target_cache_retention() {
    let options = stream_options(None, RetryPolicy::default(), CacheRetention::Long);

    assert_eq!(options.cache_retention, CacheRetention::Long);
}

#[test]
fn auth_style_maps_onto_the_provider_enum() {
    assert_eq!(auth_style_from_str("api_key"), AuthStyle::ApiKey);
    assert_eq!(auth_style_from_str("bearer"), AuthStyle::Bearer);
    // An unrecognised value must degrade to today's behaviour, never
    // silently move the credential into a different header.
    assert_eq!(auth_style_from_str("bogus"), AuthStyle::ApiKey);
    assert_eq!(auth_style_from_str(""), AuthStyle::ApiKey);
}

#[test]
fn wire_api_unknown_values_degrade_to_chat() {
    assert_eq!(wire_api_from_str("chat"), WireApi::Chat);
    assert_eq!(wire_api_from_str("responses"), WireApi::Responses);
    assert_eq!(wire_api_from_str("respones"), WireApi::Chat);
    assert_eq!(wire_api_from_str(""), WireApi::Chat);
}

#[test]
fn cache_retention_unknown_values_degrade_to_none() {
    assert_eq!(cache_retention_from_str("none"), CacheRetention::None);
    assert_eq!(cache_retention_from_str("short"), CacheRetention::Short);
    assert_eq!(cache_retention_from_str("long"), CacheRetention::Long);
    assert_eq!(cache_retention_from_str("forever"), CacheRetention::None);
}

#[test]
fn supported_provider_types_cover_every_provider_with_a_unique_key() {
    let keys = supported_provider_types();

    assert_eq!(
        keys.len(),
        ProviderType::ALL.len(),
        "every provider must be listed exactly once"
    );
    for provider_type in ProviderType::ALL {
        assert!(
            keys.contains(&provider_type.as_key()),
            "{provider_type:?} is missing from the supported list"
        );
        assert!(
            provider_type
                .as_key()
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '-'),
            "{:?} must be a kebab-case key",
            provider_type.as_key()
        );
    }

    let mut unique = keys.to_vec();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), keys.len(), "provider keys must be unique");
}

#[test]
fn the_rejection_message_lists_every_supported_provider() {
    let error = provider_type_from_str("ollama").expect_err("unsupported");
    let message = error.to_string();

    for key in supported_provider_types() {
        assert!(
            message.contains(key),
            "the rejection message must mention `{key}`, got: {message}"
        );
    }
}

#[tokio::test]
async fn collect_aggregates_tool_calls_instead_of_failing() {
    let chunks: ChunkStream = futures::stream::iter(vec![
        Ok(StreamChunk::Text("calling".to_string())),
        Ok(StreamChunk::ToolCall {
            id: "tc1".to_string(),
            name: "read_file".to_string(),
            arguments: r#"{"path":"/tmp"}"#.to_string(),
        }),
        Ok(StreamChunk::Usage(TokenUsage {
            prompt_tokens: 5,
            completion_tokens: 1,
            ..Default::default()
        })),
        Ok(StreamChunk::Done(None)),
    ])
    .boxed();

    let completion = collect(chunks).await.expect("collect");

    assert_eq!(completion.content, "calling");
    assert_eq!(completion.tool_calls.len(), 1);
    assert_eq!(completion.tool_calls[0].name, "read_file");
    assert_eq!(completion.finish_reason.as_deref(), Some("tool_calls"));
    assert_eq!(completion.usage.expect("usage").prompt_tokens, 5);
}

#[tokio::test]
async fn collect_marks_plain_completions_as_stopped() {
    let chunks: ChunkStream = futures::stream::iter(vec![
        Ok(StreamChunk::Text("hello".to_string())),
        Ok(StreamChunk::Done(None)),
    ])
    .boxed();

    let completion = collect(chunks).await.expect("collect");

    assert_eq!(completion.finish_reason.as_deref(), Some("stop"));
    assert!(completion.tool_calls.is_empty());
}

#[tokio::test]
async fn collect_defaults_the_finish_reason_when_no_done_chunk_arrives() {
    // A provider that closes the stream without a terminal chunk still
    // completed; the collector must not hand callers an empty reason, which
    // would let a truncated answer look like an ordinary stop.
    let chunks: ChunkStream =
        futures::stream::iter(vec![Ok(StreamChunk::Text("partial".to_string()))]).boxed();

    let completion = collect(chunks).await.expect("collect");

    assert_eq!(completion.finish_reason.as_deref(), Some("stop"));
}

#[tokio::test]
async fn collect_defaults_tool_calls_when_no_done_chunk_arrives() {
    let chunks: ChunkStream = futures::stream::iter(vec![Ok(StreamChunk::ToolCall {
        id: "tc1".to_string(),
        name: "read_file".to_string(),
        arguments: "{}".to_string(),
    })])
    .boxed();

    let completion = collect(chunks).await.expect("collect");

    assert_eq!(completion.finish_reason.as_deref(), Some("tool_calls"));
}

#[test]
fn a_rejected_request_is_a_bad_request_not_an_upstream_failure() {
    let error = to_stream_chunk(Err(alnair_llm::ChatError::BadRequest(
        "Anthropic API key is required".to_string(),
    )))
    .expect_err("must fail");

    assert!(
        matches!(error, Error::BadRequest(ref message) if message.contains("API key")),
        "a provider-side request rejection must stay a 400, got {error:?}"
    );
}

#[test]
fn an_upstream_error_stays_an_upstream_failure() {
    let error = to_stream_chunk(Err(alnair_llm::ChatError::Provider(
        "OpenAI API error 500: boom".to_string(),
    )))
    .expect_err("must fail");

    assert!(
        matches!(error, Error::Upstream(_)),
        "a provider failure must stay a 502, got {error:?}"
    );
}
