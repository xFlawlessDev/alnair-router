use serde_json::json;

use super::request::{
    CLI_VERSION, MAX_TOKENS_CEILING, ToolNames, build_cli_body, cli_headers, official_options,
    wire_model,
};
use super::stream::{CliEvent, UsageTotals, decode_line, event_from};
use crate::model_config::{LlmStreamOptions, ModelConfig};
use crate::types::{ChatError, LlmStreamChunk, Message, MessageToolCall};

fn config(model: &str) -> ModelConfig {
    ModelConfig::openai_compatible(
        "https://api.commandcode.ai",
        model,
        Some("user_test".to_string()),
    )
}

fn assistant_with_call(id: &str, name: &str, arguments: &str) -> Message {
    let mut message = Message::new("assistant", "calling");
    message.tool_calls = Some(vec![MessageToolCall {
        id: id.to_string(),
        name: name.to_string(),
        arguments: arguments.to_string(),
    }]);
    message
}

fn tool_result(id: &str, content: &str) -> Message {
    let mut message = Message::new("tool", content);
    message.tool_call_id = Some(id.to_string());
    message
}

#[test]
fn decode_line_accepts_plain_and_prefixed_json() {
    assert_eq!(
        decode_line(r#"{"type":"text-delta"}"#),
        Some(json!({ "type": "text-delta" }))
    );
    assert_eq!(
        decode_line(r#"data: {"type":"text-delta"}"#),
        Some(json!({ "type": "text-delta" }))
    );
    assert_eq!(
        decode_line(r#"data:{"type":"text-delta"}"#),
        Some(json!({ "type": "text-delta" }))
    );
}

#[test]
fn decode_line_skips_noise() {
    assert!(decode_line("").is_none());
    assert!(decode_line(": keep-alive").is_none());
    assert!(decode_line("event: message").is_none());
    assert!(decode_line("data: [DONE]").is_none());
    assert!(decode_line("not json").is_none());
}

#[test]
fn events_map_onto_chunks() {
    let mut usage = UsageTotals::default();

    let text = event_from(&json!({ "type": "text-delta", "text": "hi" }), &mut usage);
    assert_eq!(text, CliEvent::Text("hi".to_string()));

    let reasoning = event_from(
        &json!({ "type": "reasoning-delta", "text": "thinking" }),
        &mut usage,
    );
    assert_eq!(reasoning, CliEvent::Reasoning("thinking".to_string()));

    let call = event_from(
        &json!({
            "type": "tool-call",
            "toolCallId": "tc1",
            "toolName": "read_file",
            "input": { "path": "/tmp" }
        }),
        &mut usage,
    );
    assert_eq!(
        call,
        CliEvent::ToolCall {
            id: "tc1".to_string(),
            name: "read_file".to_string(),
            input: json!({ "path": "/tmp" }),
        }
    );

    let error = event_from(
        &json!({ "type": "error", "error": { "message": "boom" } }),
        &mut usage,
    );
    assert_eq!(error, CliEvent::Error("boom".to_string()));

    let ignored = event_from(&json!({ "type": "finish-step" }), &mut usage);
    assert_eq!(ignored, CliEvent::Ignored);
}

#[test]
fn tool_call_without_an_id_still_produces_one() {
    let mut usage = UsageTotals::default();
    let event = event_from(
        &json!({ "type": "tool-call", "toolName": "read_file" }),
        &mut usage,
    );

    match event {
        CliEvent::ToolCall { id, input, .. } => {
            assert!(!id.is_empty());
            assert_eq!(input, json!({}));
        }
        other => panic!("expected a tool call, got {other:?}"),
    }
}

#[test]
fn finish_reasons_are_normalized() {
    let mut usage = UsageTotals::default();

    for (raw, expected) in [
        ("tool-calls", "tool_calls"),
        ("toolUse", "tool_calls"),
        ("max_tokens", "length"),
        ("max_output_tokens", "length"),
        ("stop", "stop"),
        ("something-new", "stop"),
    ] {
        let event = event_from(
            &json!({ "type": "finish", "finishReason": raw }),
            &mut usage,
        );
        assert_eq!(event, CliEvent::Finish(expected.to_string()), "raw: {raw}");
    }
}

#[test]
fn usage_reads_nested_details() {
    let mut usage = UsageTotals::default();
    event_from(
        &json!({
            "type": "finish-step",
            "usage": {
                "inputTokens": 100,
                "outputTokens": 20,
                "inputTokenDetails": { "cachedTokens": 40 },
                "outputTokenDetails": { "reasoningTokens": 5 }
            }
        }),
        &mut usage,
    );

    assert_eq!(usage.counts(), (Some(100), Some(40), Some(20), Some(5)));
}

#[test]
fn usage_falls_back_to_detail_totals() {
    let mut usage = UsageTotals::default();
    event_from(
        &json!({
            "type": "finish",
            "usage": {
                "inputTokenDetails": { "noCacheTokens": 60, "cachedTokens": 40 },
                "outputTokenDetails": { "textTokens": 15, "reasoningTokens": 5 }
            }
        }),
        &mut usage,
    );

    assert_eq!(usage.counts(), (Some(100), Some(40), Some(20), Some(5)));
}

#[test]
fn usage_keeps_the_newest_count() {
    let mut usage = UsageTotals::default();
    event_from(
        &json!({ "type": "text-delta", "usage": { "inputTokens": 10 } }),
        &mut usage,
    );
    event_from(
        &json!({ "type": "finish", "usage": { "inputTokens": 25 } }),
        &mut usage,
    );

    assert_eq!(usage.counts().0, Some(25));
}

#[test]
fn wire_models_are_vendor_prefixed() {
    assert_eq!(
        wire_model("deepseek/deepseek-v4-pro"),
        "deepseek/deepseek-v4-pro"
    );
    assert_eq!(
        wire_model("command-code/deepseek/deepseek-v4-pro"),
        "deepseek/deepseek-v4-pro"
    );
    assert_eq!(wire_model("cmd/gpt-5.6-luna"), "gpt-5.6-luna");
    assert_eq!(wire_model("mimo-v2.5"), "xiaomi/mimo-v2.5");
    assert_eq!(wire_model("mimo-v2.5-pro"), "xiaomi/mimo-v2.5-pro");
    assert_eq!(wire_model("kimi-k3"), "kimi-k3");
}

#[test]
fn cli_body_splits_the_system_prompt_and_keeps_pairs() {
    let messages = vec![
        Message::new("system", "be brief"),
        Message::new("user", "hello"),
        assistant_with_call("tc1", "read_file", r#"{"path":"/tmp"}"#),
        tool_result("tc1", "file contents"),
        // No tool result for this one: the endpoint rejects dangling calls.
        assistant_with_call("tc2", "grep", r#"{"pattern":"x"}"#),
    ];

    let (body, _names) = build_cli_body(
        &config("deepseek/deepseek-v4-pro"),
        &messages,
        &LlmStreamOptions::default(),
        None,
    );

    assert_eq!(body["config"]["environment"], "external");
    assert_eq!(body["params"]["stream"], true);
    assert_eq!(body["params"]["system"], "be brief");
    assert_eq!(body["params"]["model"], "deepseek/deepseek-v4-pro");

    let params = body["params"]["messages"].as_array().expect("messages");
    assert_eq!(params.len(), 4, "system is lifted out of the message list");

    let parts = params[1]["content"].as_array().expect("assistant parts");
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[1]["type"], "tool-call");
    assert_eq!(parts[1]["input"], json!({ "path": "/tmp" }));

    let result = params[2]["content"][0].clone();
    assert_eq!(result["type"], "tool-result");
    assert_eq!(result["toolName"], "read_file");
    assert_eq!(result["output"]["value"], "file contents");

    // The dangling call is dropped, but its text survives.
    let dangling = params[3]["content"].as_array().expect("assistant parts");
    assert_eq!(dangling.len(), 1);
    assert_eq!(dangling[0]["type"], "text");
}

#[test]
fn cli_body_renames_reserved_tools_and_maps_them_back() {
    let tools = vec![json!({
        "type": "function",
        "function": {
            "name": "tool_search",
            "description": "search",
            "parameters": { "type": "object" }
        }
    })];

    let (body, names) = build_cli_body(
        &config("kimi-k3"),
        &[Message::new("user", "hi")],
        &LlmStreamOptions::default(),
        Some(&tools),
    );

    assert_eq!(body["params"]["tools"][0]["name"], "alnair_tool_search");
    assert_eq!(body["params"]["tools"][0]["input_schema"]["type"], "object");
    assert_eq!(names.client_name("alnair_tool_search"), "tool_search");
    assert_eq!(names.client_name("read_file"), "read_file");
}

#[test]
fn cli_body_clamps_max_tokens_and_maps_thinking() {
    let mut options = LlmStreamOptions {
        max_tokens: Some(MAX_TOKENS_CEILING + 5_000),
        ..Default::default()
    };
    options.thinking_level = Some(crate::model_config::ThinkingLevel::High);

    let (body, _names) = build_cli_body(
        &config("kimi-k3"),
        &[Message::new("user", "hi")],
        &options,
        None,
    );

    assert_eq!(body["params"]["max_tokens"], MAX_TOKENS_CEILING);
    assert_eq!(body["params"]["reasoning_effort"], "high");
}

#[test]
fn cli_body_omits_absent_options() {
    let (body, _names) = build_cli_body(
        &config("kimi-k3"),
        &[Message::new("user", "hi")],
        &LlmStreamOptions::default(),
        None,
    );

    assert!(body["params"].get("max_tokens").is_none());
    assert!(body["params"].get("reasoning_effort").is_none());
}

#[test]
fn official_options_clamp_max_tokens() {
    let options = LlmStreamOptions {
        max_tokens: Some(MAX_TOKENS_CEILING + 1),
        temperature: Some(0.2),
        ..Default::default()
    };

    let clamped = official_options(&options);

    assert_eq!(clamped.max_tokens, Some(MAX_TOKENS_CEILING));
    assert_eq!(clamped.temperature, Some(0.2), "other options pass through");

    let untouched = official_options(&LlmStreamOptions::default());
    assert_eq!(untouched.max_tokens, None);
}

#[test]
fn cli_headers_pin_the_cli_version() {
    let headers = cli_headers();
    let version = headers
        .iter()
        .find(|(name, _)| *name == "x-command-code-version")
        .expect("version header");

    assert_eq!(version.1, CLI_VERSION);
    assert!(
        headers.iter().any(|(name, _)| *name == "x-cli-environment"),
        "the CLI transport identifies itself"
    );
}

#[test]
fn tool_names_start_empty() {
    assert_eq!(ToolNames::default().client_name("anything"), "anything");
}

/// Pins the OpenAI provider's error shape that the transport fallback keys on.
#[test]
fn entitlement_rejections_are_recognized() {
    let failure = |message: &str| Err(ChatError::Provider(message.to_string()));

    assert!(super::is_entitlement_rejection(&failure(
        r#"OpenAI API error 403 Forbidden: {"error":{"code":"upgrade_required"}}"#
    )));
    assert!(super::is_entitlement_rejection(&failure(
        "OpenAI API error 404 Not Found: no such path"
    )));

    assert!(!super::is_entitlement_rejection(&failure(
        "OpenAI API error 500 Internal Server Error"
    )));
    assert!(!super::is_entitlement_rejection(&failure(
        "OpenAI API error 429 Too Many Requests"
    )));
    assert!(!super::is_entitlement_rejection(&Ok(LlmStreamChunk::Text(
        "hello".to_string()
    ))));
}

/// Connections created before this family existed carry the Provider API path
/// in their base URL.
#[test]
fn base_urls_normalize_to_the_host() {
    assert_eq!(
        super::host_base("https://api.commandcode.ai/provider/v1"),
        "https://api.commandcode.ai"
    );
    assert_eq!(
        super::host_base("https://api.commandcode.ai/provider/v1/"),
        "https://api.commandcode.ai"
    );
    assert_eq!(
        super::host_base("https://api.commandcode.ai"),
        "https://api.commandcode.ai"
    );
}
