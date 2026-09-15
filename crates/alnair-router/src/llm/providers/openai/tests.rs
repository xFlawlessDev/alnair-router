
use super::*;
use crate::llm::model_config::ThinkingLevel;
use crate::llm::types::GenerationOptions;

#[test]
fn retries_openai_compatible_auto_tool_choice_errors_without_tools() {
    let body = serde_json::json!({
        "model": "Qwen/Qwen3.6-35B-A3B",
        "messages": [],
        "stream": true,
        "tools": [{"type": "function"}]
    });

    assert!(should_retry_openai_without_tools(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"error":{"message":"\"auto\" tool choice requires --enable-auto-tool-choice and --tool-call-parser to be set"}}"#,
        &body,
    ));
}

#[test]
fn does_not_retry_openai_errors_when_tools_were_not_sent() {
    let body = serde_json::json!({
        "model": "Qwen/Qwen3.6-35B-A3B",
        "messages": [],
        "stream": true
    });

    assert!(!should_retry_openai_without_tools(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"error":{"message":"bad request"}}"#,
        &body,
    ));
}

#[test]
fn repairs_truncated_tool_call_arguments_by_closing_containers() {
    let parsed = parse_tool_call_arguments_lossy(
            r#"{"value":{"documents":[{"file_name":"knowledge-graph-sample.md","description":"Knowledge Graph Sample"},{"file_name":"Screenshot 2026-02-02 155344.png","description":"Screenshot image of..."}]}"#,
            "set_structured_output",
        )
        .expect("repair incomplete JSON");

    assert_eq!(
        parsed["value"]["documents"][0]["file_name"],
        "knowledge-graph-sample.md"
    );
    assert_eq!(
        parsed["value"]["documents"][1]["file_name"],
        "Screenshot 2026-02-02 155344.png"
    );
}

#[test]
fn malformed_tool_call_arguments_returns_error() {
    let parsed = parse_tool_call_arguments_lossy(r#"["not", "object"]"#, "set_structured_output");

    assert!(parsed.is_err());
}

#[test]
fn repairs_windows_paths_with_bare_backslashes_in_tool_arguments() {
    let parsed = parse_tool_call_arguments_lossy(
            r#"{"value":{"solution":"C:\Users\<user>\AppData\Local\Pongo\Logs contains the diagnostic files."}}"#,
            "set_structured_output",
        )
        .expect("repair invalid backslash escapes");

    assert_eq!(
        parsed["value"]["solution"],
        r#"C:\Users\<user>\AppData\Local\Pongo\Logs contains the diagnostic files."#
    );
}

#[test]
fn splits_standard_embedded_think_tags() {
    let parsed = split_embedded_thinking("<think>Use known geography.</think>\n\nParis.");
    assert_eq!(parsed.thinking.as_deref(), Some("Use known geography."));
    assert_eq!(parsed.text, "Paris.");
}

#[test]
fn splits_provider_thinking_process_content_without_open_tag() {
    let parsed = split_embedded_thinking(
        "Here's a thinking process:\n\n1. Identify the question.\n2. Verify Paris.\n</think>\n\nThe capital of France is Paris.",
    );
    assert_eq!(
        parsed.thinking.as_deref(),
        Some("1. Identify the question.\n2. Verify Paris.")
    );
    assert_eq!(parsed.text, "The capital of France is Paris.");
}

#[test]
fn splits_qwen_plain_reasoning_without_think_tags() {
    let parsed = split_embedded_thinking(
        "The user is greeting me with \"hello dear\". This is a simple greeting and does not require any specific or skills. I should respond with a friendly greeting back. \n\nI will not call any tools. I will simply reply to the user.\n \n\nHello! How can I help you today? ",
    );

    assert_eq!(
        parsed.thinking.as_deref(),
        Some(
            "The user is greeting me with \"hello dear\". This is a simple greeting and does not require any specific or skills. I should respond with a friendly greeting back. \n\nI will not call any tools. I will simply reply to the user."
        )
    );
    assert_eq!(parsed.text, "Hello! How can I help you today?");
}

#[test]
fn streaming_parser_buffers_embedded_thinking_until_close_tag() {
    let mut parser = EmbeddedThinkingStreamParser::default();
    assert!(parser.push("Here's a thinking ").is_empty());
    assert!(parser.push("process:\n\n1. Check fact.").is_empty());
    let chunks = parser.push("</think>\n\nParis.");

    assert_eq!(
        chunks,
        vec![
            ParsedEmbeddedThinkingChunk::Thinking("1. Check fact.".to_string()),
            ParsedEmbeddedThinkingChunk::Text("Paris.".to_string())
        ]
    );
    assert!(parser.flush().is_empty());
}

#[test]
fn streaming_parser_splits_untagged_reasoning_on_flush() {
    let mut parser = EmbeddedThinkingStreamParser::default();
    assert!(
        parser
            .push("The user is greeting me with \"hello dear\". This is a simple greeting.\n\n")
            .is_empty()
    );
    assert!(
        parser
            .push("I will not call any tools. I will simply reply to the user.\n \n\n")
            .is_empty()
    );
    assert!(parser.push("Hello! How can I help you today? ").is_empty());

    assert_eq!(
            parser.flush(),
            vec![
                ParsedEmbeddedThinkingChunk::Thinking(
                    "The user is greeting me with \"hello dear\". This is a simple greeting.\n\nI will not call any tools. I will simply reply to the user."
                        .to_string()
                ),
                ParsedEmbeddedThinkingChunk::Text("Hello! How can I help you today?".to_string())
            ]
        );
}

#[test]
fn streaming_parser_passes_through_normal_content() {
    let mut parser = EmbeddedThinkingStreamParser::default();
    assert_eq!(
        parser.push("The capital "),
        vec![ParsedEmbeddedThinkingChunk::Text(
            "The capital ".to_string()
        )]
    );
    assert_eq!(
        parser.push("is Paris."),
        vec![ParsedEmbeddedThinkingChunk::Text("is Paris.".to_string())]
    );
}

#[test]
fn llm_stream_options_apply_to_openai_body_includes_penalties() {
    let generation = GenerationOptions {
        presence_penalty: Some(0.45),
        frequency_penalty: Some(0.12),
        max_tokens: Some(128),
        ..GenerationOptions::default()
    };
    let options = LlmStreamOptions::from(&generation);

    let mut body = serde_json::json!({});
    options.apply_to_openai_body(&mut body);

    assert_eq!(body["presence_penalty"], serde_json::json!(0.45));
    assert_eq!(body["frequency_penalty"], serde_json::json!(0.12));
    assert_eq!(body["max_tokens"], serde_json::json!(128));
}

#[test]
fn extract_usage_reads_top_level_usage() {
    let response = serde_json::json!({
        "usage": {"prompt_tokens": 120},
        "choices": [{"usage": {"prompt_tokens": 80}}]
    });

    assert_eq!(
        extract_usage(&response),
        Some(&serde_json::json!({"prompt_tokens": 120}))
    );
}

#[test]
fn extract_usage_reads_first_choice_usage() {
    let response = serde_json::json!({
        "choices": [{"usage": {"prompt_tokens": 120}}]
    });

    assert_eq!(
        extract_usage(&response),
        Some(&serde_json::json!({"prompt_tokens": 120}))
    );
}

#[test]
fn extract_reasoning_text_prioritizes_reasoning_content() {
    let message = serde_json::json!({
        "reasoning_content": "primary",
        "reasoning": "secondary",
        "reasoning_text": "tertiary",
        "thinking": "fallback"
    });

    assert_eq!(extract_reasoning_text(&message), Some("primary"));
}

#[test]
fn extract_reasoning_text_reads_reasoning_text() {
    let message = serde_json::json!({"reasoning_text": "provider reasoning"});

    assert_eq!(extract_reasoning_text(&message), Some("provider reasoning"));
}

#[test]
fn thinking_level_medium_sets_reasoning_effort_medium() {
    let options = LlmStreamOptions {
        thinking_level: Some(ThinkingLevel::Medium),
        ..LlmStreamOptions::default()
    };
    let mut body = serde_json::json!({});

    options.apply_to_openai_body(&mut body);

    assert_eq!(body["reasoning_effort"], serde_json::json!("medium"));
}

#[test]
fn thinking_level_none_omits_reasoning_effort() {
    let options = LlmStreamOptions {
        thinking_level: Some(ThinkingLevel::None),
        ..LlmStreamOptions::default()
    };
    let mut body = serde_json::json!({});

    options.apply_to_openai_body(&mut body);

    assert!(body.get("reasoning_effort").is_none());
}

#[test]
fn thinking_levels_map_to_openai_reasoning_effort_values() {
    assert_eq!(
        openai_reasoning_effort(Some(ThinkingLevel::Minimal)),
        Some("minimal")
    );
    assert_eq!(
        openai_reasoning_effort(Some(ThinkingLevel::Low)),
        Some("low")
    );
    assert_eq!(
        openai_reasoning_effort(Some(ThinkingLevel::Max)),
        Some("xhigh")
    );
}

#[test]
fn gpt5_chat_payload_uses_max_completion_tokens_and_omits_unsupported_params() {
    let options = LlmStreamOptions {
        temperature: Some(0.2),
        top_p: Some(0.9),
        presence_penalty: Some(0.3),
        frequency_penalty: Some(0.1),
        max_tokens: Some(256),
        seed: Some(42),
        thinking_level: Some(ThinkingLevel::Medium),
        ..LlmStreamOptions::default()
    };
    let mut body = serde_json::json!({"model": "gpt-5.5", "messages": []});

    options.apply_to_openai_compatible_body(&mut body, true, false);
    apply_openai_chat_model_compatibility("gpt-5.5", &mut body);

    assert_eq!(body["max_completion_tokens"], serde_json::json!(256));
    assert!(body.get("max_tokens").is_none());
    assert!(body.get("temperature").is_none());
    assert!(body.get("top_p").is_none());
    assert!(body.get("presence_penalty").is_none());
    assert!(body.get("frequency_penalty").is_none());
    assert!(body.get("seed").is_none());
    assert_eq!(body["reasoning_effort"], serde_json::json!("medium"));
}

#[test]
fn non_gpt5_chat_payload_keeps_openai_compatible_fields() {
    let options = LlmStreamOptions {
        temperature: Some(0.2),
        max_tokens: Some(128),
        thinking_level: Some(ThinkingLevel::Medium),
        ..LlmStreamOptions::default()
    };
    let mut body = serde_json::json!({"model": "gpt-4o", "messages": []});

    options.apply_to_openai_compatible_body(&mut body, true, false);
    apply_openai_chat_model_compatibility("gpt-4o", &mut body);

    assert_eq!(body["max_tokens"], serde_json::json!(128));
    assert_eq!(body["temperature"], serde_json::json!(0.2));
    assert_eq!(body["reasoning_effort"], serde_json::json!("medium"));
}

#[test]
fn llm_stream_options_apply_to_openai_compatible_body_sets_cache_control() {
    let options = LlmStreamOptions {
        cache_retention: crate::llm::model_config::CacheRetention::Short,
        ..LlmStreamOptions::default()
    };
    let mut body = serde_json::json!({});

    options.apply_to_openai_compatible_body(&mut body, false, true);

    assert_eq!(
        body["cache_control"],
        serde_json::json!({"type": "ephemeral"})
    );
}

#[test]
fn llm_stream_options_apply_to_openai_compatible_body_omits_cache_control_when_disabled() {
    let options = LlmStreamOptions::default();
    let mut body = serde_json::json!({});

    options.apply_to_openai_compatible_body(&mut body, false, false);

    assert!(body.get("cache_control").is_none());
}

#[test]
fn openai_compatible_body_omits_reasoning_without_thinking_support() {
    let options = LlmStreamOptions {
        thinking_level: Some(ThinkingLevel::Medium),
        ..LlmStreamOptions::default()
    };
    let mut body = serde_json::json!({});

    options.apply_to_openai_compatible_body(&mut body, false, false);

    assert!(body.get("reasoning_effort").is_none());
}

#[test]
fn request_diagnostics_summarize_keys_and_tool_names() {
    let body = serde_json::json!({
        "stream": true,
        "model": "gpt-5.5",
        "tools": [
            {"type": "function", "function": {"name": "read_file", "parameters": {"type": "object"}}},
            {"type": "function", "function": {"parameters": {"type": "object"}}}
        ],
        "messages": []
    });

    assert_eq!(summarize_request_keys(&body), "messages,model,stream,tools");
    assert_eq!(
        summarize_tool_names(body.get("tools")),
        "read_file (+1 unnamed)"
    );
}

#[test]
fn sse_line_buffer_keeps_json_split_across_chunks() {
    let mut buffer = String::new();

    let first = drain_complete_sse_lines(
        &mut buffer,
        r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"read_"#,
    );
    let second = drain_complete_sse_lines(
        &mut buffer,
        r#"file","arguments":"{\"path\":"}}]}}]}
"#,
    );

    assert!(first.is_empty());
    assert_eq!(second.len(), 1);
    assert!(second[0].contains(r#""name":"read_file""#));
}

#[test]
fn extracts_tool_name_from_alternate_fields() {
    let tool = serde_json::json!({
        "id": "call_1",
        "name": "read_file",
        "arguments": {"path": "/tmp"}
    });

    assert_eq!(extract_tool_call_name(&tool), Some("read_file"));
    assert_eq!(
        extract_tool_call_arguments_fragment(&tool),
        Some(r#"{"path":"/tmp"}"#.to_string())
    );
}

#[test]
fn extract_tool_call_name_rejects_blank_values() {
    let tool = serde_json::json!({"function": {"name": "   "}});

    assert_eq!(extract_tool_call_name(&tool), None);
}

#[test]
fn finish_reason_becomes_tool_calls_when_tool_calls_exist() {
    assert_eq!(
        finish_reason_for_tool_calls(Some("stop".to_string()), true),
        Some("tool_calls".to_string())
    );
}

#[test]
fn finish_reason_preserves_stop_without_tool_calls() {
    assert_eq!(
        finish_reason_for_tool_calls(Some("stop".to_string()), false),
        Some("stop".to_string())
    );
}

#[test]
fn terminal_stream_finish_reason_defaults_to_stop_without_provider_done() {
    assert_eq!(
        terminal_stream_finish_reason(None, false),
        Some("stop".to_string())
    );
}

#[test]
fn terminal_stream_finish_reason_prefers_tool_calls_when_tools_exist() {
    assert_eq!(
        terminal_stream_finish_reason(None, true),
        Some("tool_calls".to_string())
    );
}

#[test]
fn provider_network_error_text_is_user_visible_and_bounded() {
    let error = ChatError::Provider("connection reset by peer".to_string());

    assert_eq!(
        provider_network_error_text(&error),
        "[provider request failed after retry: provider error: connection reset by peer]"
    );
}

#[tokio::test]
async fn request_network_failure_yields_error_after_retries() {
    let provider = OpenAiProvider::new();
    let config = ModelConfig::openai_compatible("http://127.0.0.1:1", "gpt-4o", None);
    let options = LlmStreamOptions {
        max_retry_delay_ms: 1,
        ..LlmStreamOptions::default()
    };
    let mut stream = provider.stream_with_mode(
        &config,
        vec![Message::new("user", "hello")],
        &options,
        None,
        true,
    );

    let first = tokio::time::timeout(std::time::Duration::from_secs(60), stream.next())
        .await
        .expect("network failure should resolve promptly")
        .expect("provider should emit one item");
    assert!(matches!(first, Err(ChatError::Provider(_))));
}
#[test]
fn build_tool_call_chunk_rejects_empty_tool_name() {
    assert!(
        build_tool_call_chunk("tc1".to_string(), "   ".to_string(), "{}")
            .expect("valid empty args")
            .is_none()
    );
}

#[test]
fn retries_on_unsupported_tools_parameter() {
    let body = serde_json::json!({
        "model": "gpt-5.5",
        "messages": [],
        "tools": [{"type": "function"}]
    });
    assert!(should_retry_openai_without_tools(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"error":{"message":"Unsupported parameter: 'tools' is not supported for this model."}}"#,
        &body,
    ));
}

#[test]
fn retries_on_tool_not_supported_error() {
    let body = serde_json::json!({
        "model": "gpt-5.5",
        "messages": [],
        "tools": [{"type": "function"}]
    });
    assert!(should_retry_openai_without_tools(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"error":{"message":"tools is not supported"}}"#,
        &body,
    ));
}

#[test]
fn retries_on_function_calling_not_supported() {
    let body = serde_json::json!({
        "model": "gpt-5.5",
        "messages": [],
        "tools": [{"type": "function"}]
    });
    assert!(should_retry_openai_without_tools(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"error":{"message":"function calling is not available for model gpt-5.5"}}"#,
        &body,
    ));
}

#[test]
fn retries_on_unsupported_reasoning_effort_parameter() {
    let body = serde_json::json!({
        "model": "gpt-5.5",
        "messages": [],
        "reasoning_effort": "minimal"
    });
    assert!(should_retry_openai_without_reasoning(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"error":{"message":"Unsupported parameter: 'reasoning_effort' is not supported for this model."}}"#,
        &body,
    ));
}

#[test]
fn does_not_retry_reasoning_error_without_reasoning_effort() {
    let body = serde_json::json!({
        "model": "gpt-4o",
        "messages": []
    });
    assert!(!should_retry_openai_without_reasoning(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"error":{"message":"Unsupported parameter: reasoning_effort"}}"#,
        &body,
    ));
}

#[test]
fn does_not_retry_reasoning_auth_error() {
    let body = serde_json::json!({
        "model": "gpt-5.5",
        "messages": [],
        "reasoning_effort": "medium"
    });
    assert!(!should_retry_openai_without_reasoning(
        reqwest::StatusCode::UNAUTHORIZED,
        r#"{"error":{"message":"Invalid API key for reasoning_effort request"}}"#,
        &body,
    ));
}

#[test]
fn does_not_retry_auth_error_with_tools() {
    let body = serde_json::json!({
        "model": "gpt-5.5",
        "messages": [],
        "tools": [{"type": "function"}]
    });
    assert!(!should_retry_openai_without_tools(
        reqwest::StatusCode::UNAUTHORIZED,
        r#"{"error":{"message":"Invalid API key"}}"#,
        &body,
    ));
}

#[test]
fn does_not_retry_rate_limit_with_tools() {
    let body = serde_json::json!({
        "model": "gpt-5.5",
        "messages": [],
        "tools": [{"type": "function"}]
    });
    assert!(!should_retry_openai_without_tools(
        reqwest::StatusCode::TOO_MANY_REQUESTS,
        r#"{"error":{"message":"Rate limit exceeded"}}"#,
        &body,
    ));
}

#[test]
fn does_not_retry_invalid_tool_schema_error() {
    let body = serde_json::json!({
        "model": "gpt-5.5",
        "messages": [],
        "tools": [{"type": "function"}]
    });
    assert!(!should_retry_openai_without_tools(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"error":{"message":"Invalid tool schema: missing required property 'parameters'."}}"#,
        &body,
    ));
}

#[test]
fn does_not_retry_invalid_tool_name_error() {
    let body = serde_json::json!({
        "model": "gpt-5.5",
        "messages": [],
        "tools": [{"type": "function"}]
    });
    assert!(!should_retry_openai_without_tools(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"error":{"message":"Invalid tool name: must match pattern ^[a-zA-Z0-9_-]+$."}}"#,
        &body,
    ));
}

#[test]
fn does_not_retry_without_tools_in_body() {
    let body = serde_json::json!({
        "model": "gpt-5.5",
        "messages": [],
    });
    assert!(!should_retry_openai_without_tools(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"error":{"message":"Unsupported parameter: tools"}}"#,
        &body,
    ));
}
