use super::*;
use crate::model_config::ThinkingLevel;
use crate::types::{Message, MessageToolCall};
use futures::StreamExt;

fn convert_for_test(messages: &[Message], options: &LlmStreamOptions) -> AnthropicRequest {
    convert_messages_to_anthropic(messages, None, options, true, true)
}

#[test]
fn completion_json_maps_to_chunks() {
    let parsed: AnthropicCompletion = serde_json::from_str(
            r#"{
                "content": [
                    { "type": "thinking", "thinking": "pondering", "signature": "sig-1" },
                    { "type": "text", "text": "hello" },
                    { "type": "tool_use", "id": "tc1", "name": "read", "input": { "path": "/tmp" } },
                    { "type": "redacted_thinking", "data": "opaque" }
                ],
                "usage": { "input_tokens": 10, "output_tokens": 2 }
            }"#,
        )
        .expect("valid completion");

    let chunks = anthropic_completion_chunks(parsed, None);

    assert!(matches!(&chunks[0], LlmStreamChunk::Thinking(text) if text == "pondering"));
    assert!(matches!(&chunks[1], LlmStreamChunk::ThinkingSignature(sig) if sig == "sig-1"));
    assert!(matches!(&chunks[2], LlmStreamChunk::Text(text) if text == "hello"));
    assert!(matches!(
        &chunks[3],
        LlmStreamChunk::ToolCall { id, name, .. } if id == "tc1" && name == "read"
    ));
    assert!(matches!(&chunks[4], LlmStreamChunk::RedactedThinking(data) if data == "opaque"));
    assert!(matches!(
        &chunks[5],
        LlmStreamChunk::Usage {
            prompt_eval_count: Some(10),
            eval_count: Some(2),
            ..
        }
    ));
    assert!(matches!(&chunks[6], LlmStreamChunk::Done(_)));
    assert_eq!(chunks.len(), 7, "all known block types are preserved");
}

#[test]
fn convert_messages_extracts_system_to_top_level() {
    let messages = vec![
        Message::new("system", "You are helpful"),
        Message::new("user", "hello"),
    ];
    let request = convert_for_test(&messages, &LlmStreamOptions::default());

    assert_eq!(
        request.system,
        Some(vec![AnthropicSystemBlock {
            kind: "text".to_string(),
            text: "You are helpful".to_string(),
            cache_control: None,
        }])
    );
    assert_eq!(request.messages.len(), 1);
    assert_eq!(request.messages[0].role, "user");
}

#[test]
fn convert_messages_maps_tool_calls_to_tool_use_blocks() {
    let mut assistant = Message::new("assistant", "");
    assistant.tool_calls = Some(vec![MessageToolCall {
        id: "tc1".to_string(),
        name: "read_file".to_string(),
        arguments: r#"{"path":"/tmp"}"#.to_string(),
    }]);

    let request = convert_for_test(&[assistant], &LlmStreamOptions::default());
    assert_eq!(request.messages.len(), 1);

    let has_tool_use = request.messages[0].content.iter().any(|block| {
        matches!(
            block,
            AnthropicContentBlock::ToolUse { id, name, input }
                if id == "tc1"
                    && name == "read_file"
                    && *input == serde_json::json!({"path": "/tmp"})
        )
    });
    assert!(has_tool_use);
}

#[test]
fn convert_messages_maps_tool_results_to_tool_result_blocks() {
    let mut tool_message = Message::new("tool", "file content");
    tool_message.tool_call_id = Some("tc1".to_string());

    let request = convert_for_test(&[tool_message], &LlmStreamOptions::default());

    assert_eq!(request.messages.len(), 1);
    assert_eq!(request.messages[0].role, "user");

    let has_tool_result = request.messages[0].content.iter().any(|block| {
        matches!(
            block,
            AnthropicContentBlock::ToolResult { tool_use_id, content, .. }
                if tool_use_id == "tc1" && content == "file content"
        )
    });
    assert!(has_tool_result);
}

#[test]
fn thinking_level_minimal_sets_budget_1024() {
    let options = LlmStreamOptions {
        thinking_level: Some(ThinkingLevel::Minimal),
        ..LlmStreamOptions::default()
    };

    let request = convert_for_test(&[Message::new("user", "hello")], &options);

    let thinking = request.thinking.expect("thinking config");
    assert_eq!(thinking.kind, "enabled");
    assert_eq!(thinking.budget_tokens, 1024);
    assert_eq!(request.max_tokens, 8192);
}

#[test]
fn signed_thinking_is_replayed_before_tool_use() {
    let options = LlmStreamOptions {
        thinking_level: Some(ThinkingLevel::Low),
        ..LlmStreamOptions::default()
    };
    let mut assistant = Message::new("assistant", "");
    assistant.thinking = Some("reasoning".to_string());
    assistant.thinking_signature = Some("sig-xyz".to_string());
    assistant.tool_calls = Some(vec![MessageToolCall {
        id: "tc1".to_string(),
        name: "read_file".to_string(),
        arguments: r#"{"path":"/tmp"}"#.to_string(),
    }]);
    assistant.redacted_thinking = Some("opaque".to_string());

    let request = convert_for_test(&[assistant], &options);

    let blocks = &request.messages[0].content;
    assert!(matches!(
        blocks[0],
        AnthropicContentBlock::RedactedThinking { ref data } if data == "opaque"
    ));
    assert!(matches!(
        blocks[1],
        AnthropicContentBlock::Thinking { ref thinking, ref signature }
            if thinking == "reasoning" && signature == "sig-xyz"
    ));
    assert!(matches!(
        blocks[2],
        AnthropicContentBlock::ToolUse { ref id, .. } if id == "tc1"
    ));
}

#[test]
fn signed_thinking_is_dropped_when_thinking_disabled() {
    let mut assistant = Message::new("assistant", "answer");
    assistant.thinking = Some("reasoning".to_string());
    assistant.thinking_signature = Some("sig-xyz".to_string());

    let request = convert_for_test(&[assistant], &LlmStreamOptions::default());

    let has_thinking = request.messages[0]
        .content
        .iter()
        .any(|block| matches!(block, AnthropicContentBlock::Thinking { .. }));
    assert!(!has_thinking);
}

#[test]
fn thinking_level_max_bumps_max_tokens_when_needed() {
    let options = LlmStreamOptions {
        max_tokens: Some(1024),
        thinking_level: Some(ThinkingLevel::Max),
        ..LlmStreamOptions::default()
    };

    let request = convert_for_test(&[Message::new("user", "hello")], &options);

    let thinking = request.thinking.expect("thinking config");
    assert_eq!(thinking.budget_tokens, 32768);
    assert_eq!(request.max_tokens, 33792);
}

#[test]
fn thinking_level_ignored_when_model_lacks_capability() {
    let options = LlmStreamOptions {
        thinking_level: Some(ThinkingLevel::Minimal),
        ..LlmStreamOptions::default()
    };

    let request = convert_messages_to_anthropic(
        &[Message::new("user", "hello")],
        None,
        &options,
        false,
        true,
    );

    assert_eq!(request.thinking, None);
}

#[test]
fn cache_marker_applies_to_last_system_block_only() {
    let options = LlmStreamOptions {
        cache_retention: CacheRetention::Short,
        ..LlmStreamOptions::default()
    };
    let request = convert_for_test(
        &[
            Message::new("system", "one"),
            Message::new("system", "two"),
            Message::new("user", "hello"),
        ],
        &options,
    );

    let system = request.system.expect("system blocks");
    assert_eq!(
        system
            .iter()
            .filter(|block| block.cache_control.is_some())
            .count(),
        1
    );
    assert_eq!(
        system[1]
            .cache_control
            .as_ref()
            .map(|cache| cache.kind.as_str()),
        Some("ephemeral")
    );
}

#[test]
fn tool_schema_preserves_nested_fields() {
    let tools = vec![serde_json::json!({
        "type": "function",
        "function": {
            "name": "search",
            "description": "Search docs",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Search text"}
                },
                "required": ["query"],
                "additionalProperties": false
            }
        }
    })];

    let converted = convert_tools_to_anthropic(Some(&tools), None).expect("tools");

    assert_eq!(converted[0].input_schema["additionalProperties"], false);
}

#[test]
fn generation_options_are_forwarded_to_request() {
    let options = LlmStreamOptions {
        temperature: Some(0.3),
        top_p: Some(0.8),
        top_k: Some(40),
        stop: Some(vec!["\n\n".to_string()]),
        ..LlmStreamOptions::default()
    };

    let request = convert_for_test(&[Message::new("user", "hello")], &options);

    assert_eq!(request.temperature, Some(0.3));
    assert_eq!(request.top_p, Some(0.8));
    assert_eq!(request.top_k, Some(40));
    assert_eq!(request.stop_sequences, Some(vec!["\n\n".to_string()]));
}

#[test]
fn thinking_drops_sampling_parameters() {
    let options = LlmStreamOptions {
        temperature: Some(0.3),
        top_p: Some(0.8),
        top_k: Some(40),
        thinking_level: Some(ThinkingLevel::Low),
        ..LlmStreamOptions::default()
    };

    let request = convert_for_test(&[Message::new("user", "hello")], &options);

    assert!(request.thinking.is_some());
    assert_eq!(request.temperature, None);
    assert_eq!(request.top_p, None);
    assert_eq!(request.top_k, None);
}

#[test]
fn long_cache_retention_sets_one_hour_ttl() {
    let options = LlmStreamOptions {
        cache_retention: CacheRetention::Long,
        ..LlmStreamOptions::default()
    };

    let request = convert_for_test(
        &[
            Message::new("system", "big prompt"),
            Message::new("user", "hi"),
        ],
        &options,
    );

    let system = request.system.as_ref().expect("system blocks");
    assert_eq!(
        system[0]
            .cache_control
            .as_ref()
            .and_then(|c| c.ttl.as_deref()),
        Some("1h")
    );
    assert!(request.uses_extended_cache_ttl());
}

#[test]
fn short_cache_retention_does_not_request_extended_ttl() {
    let options = LlmStreamOptions {
        cache_retention: CacheRetention::Short,
        ..LlmStreamOptions::default()
    };

    let request = convert_for_test(&[Message::new("user", "hi")], &options);

    assert!(!request.uses_extended_cache_ttl());
}

#[test]
fn consecutive_same_role_messages_are_merged() {
    let messages = vec![
        Message::new("user", "first"),
        Message::new("user", "second"),
    ];

    let request = convert_for_test(&messages, &LlmStreamOptions::default());

    assert_eq!(request.messages.len(), 1);
    assert_eq!(request.messages[0].content.len(), 2);
}

#[tokio::test]
async fn missing_api_key_fails_before_http_request() {
    let provider = AnthropicNativeProvider::new();
    let config = ModelConfig::anthropic("http://127.0.0.1:1", "claude-test", None);
    let options = LlmStreamOptions::default();
    let mut stream = provider.stream(&config, vec![Message::new("user", "hello")], &options, None);

    let error = stream
        .next()
        .await
        .expect("first stream item")
        .expect_err("missing key should fail");

    assert!(error.to_string().contains("Anthropic API key is required"));
}

#[test]
fn cache_marker_omitted_when_capability_disabled() {
    let options = LlmStreamOptions {
        cache_retention: CacheRetention::Short,
        ..LlmStreamOptions::default()
    };
    let request = convert_messages_to_anthropic(
        &[Message::new("user", "hello")],
        None,
        &options,
        true,
        false,
    );

    let has_marker = request.messages.iter().any(|message| {
        message.content.iter().any(|block| match block {
            AnthropicContentBlock::Text { cache_control, .. }
            | AnthropicContentBlock::ToolResult { cache_control, .. }
            | AnthropicContentBlock::Image { cache_control, .. } => cache_control.is_some(),
            AnthropicContentBlock::Thinking { .. }
            | AnthropicContentBlock::RedactedThinking { .. }
            | AnthropicContentBlock::ToolUse { .. } => false,
        })
    });
    assert!(!has_marker);
}

#[test]
fn prior_unsigned_thinking_is_stripped_from_history() {
    let mut assistant = Message::new("assistant", "answer");
    assistant.thinking = Some("private reasoning".to_string());

    let request = convert_for_test(&[assistant], &LlmStreamOptions::default());

    assert_eq!(request.messages[0].content.len(), 1);
}

#[test]
fn stream_parser_reads_message_delta_usage() {
    let mut pending = HashMap::new();
    let mut usage = serde_json::Map::new();
    let (chunks, terminal) = handle_anthropic_sse_line(
            r#"data: {"type":"message_delta","usage":{"input_tokens":10,"cache_read_input_tokens":4,"output_tokens":2}}"#,
            &mut pending,
            &mut usage,
            None,
        )
        .expect("usage event");

    assert!(!terminal);
    assert!(matches!(
        chunks.first(),
        Some(LlmStreamChunk::Usage {
            prompt_eval_count: Some(10),
            cached_prompt_eval_count: Some(4),
            eval_count: Some(2),
            ..
        })
    ));
}

#[test]
fn stream_usage_merges_message_start_and_final_delta() {
    let mut pending = HashMap::new();
    let mut usage = serde_json::Map::new();
    handle_anthropic_sse_line(
        r#"data: {"type":"message_start","message":{"usage":{"input_tokens":100,"cache_read_input_tokens":20,"output_tokens":1}}}"#,
        &mut pending,
        &mut usage,
        None,
    )
    .expect("start");

    let (chunks, terminal) = handle_anthropic_sse_line(
        r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":50}}"#,
        &mut pending,
        &mut usage,
        None,
    )
    .expect("delta");

    assert!(terminal);
    assert!(matches!(
        chunks.first(),
        Some(LlmStreamChunk::Usage {
            prompt_eval_count: Some(100),
            cached_prompt_eval_count: Some(20),
            eval_count: Some(50),
            ..
        })
    ));
}

#[test]
fn stream_parser_reports_terminal_done() {
    let mut pending = HashMap::new();
    let mut usage = serde_json::Map::new();
    let (chunks, terminal) = handle_anthropic_sse_line(
        r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"}}"#,
        &mut pending,
        &mut usage,
        None,
    )
    .expect("done event");

    assert!(terminal);
    assert!(
        matches!(chunks.first(), Some(LlmStreamChunk::Done(Some(reason))) if reason == "end_turn")
    );
}

#[test]
fn stream_parser_handles_unterminated_final_sse_line() {
    let mut buffer = SseLineBuffer::default();
    assert!(buffer.push(br#"data:{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}"#).is_empty());
    let line = buffer.finish().expect("leftover line");
    let mut pending = HashMap::new();
    let mut usage = serde_json::Map::new();

    let (chunks, terminal) =
        handle_anthropic_sse_line(&line, &mut pending, &mut usage, None).expect("line");

    assert!(!terminal);
    assert!(matches!(chunks.first(), Some(LlmStreamChunk::Text(text)) if text == "hi"));
}

#[test]
fn stream_parser_rejects_malformed_tool_input() {
    let mut pending = HashMap::new();
    let mut usage = serde_json::Map::new();
    handle_anthropic_sse_line(
            r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"search"}}"#,
            &mut pending,
            &mut usage,
            None,
        )
        .expect("start");
    handle_anthropic_sse_line(
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"not json"}}"#,
            &mut pending,
            &mut usage,
            None,
        )
        .expect("delta");

    let error = handle_anthropic_sse_line(
        r#"data: {"type":"content_block_stop","index":0}"#,
        &mut pending,
        &mut usage,
        None,
    )
    .expect_err("malformed input must fail");

    assert!(
        error
            .to_string()
            .contains("provider returned malformed tool JSON")
    );
}
