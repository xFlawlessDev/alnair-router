use serde_json::json;

use super::request::{build_request_body, convert_tools};
use super::stream::completion_chunks;
use crate::model_config::{LlmStreamOptions, ModelConfig, ThinkingLevel};
use crate::providers::codex::{CODEX_OPENAI_BETA, CODEX_ORIGINATOR, CODEX_USER_AGENT};
use crate::types::{
    ContentPart, ImageUrl, ImageUrlContentPart, Message, MessageContent, MessageToolCall,
    TextContentPart,
};

fn config() -> ModelConfig {
    ModelConfig {
        provider_type: crate::types::ProviderType::OpenAiCodex,
        base_url: "https://chatgpt.com/backend-api/codex".to_string(),
        model_id: "gpt-5.5".to_string(),
        api_key: Some("jwt".to_string()),
        auth_style: crate::model_config::AuthStyle::Bearer,
        custom_headers: Default::default(),
        chatgpt_account_id: Some("acct-1".to_string()),
        supports_vision: false,
        supports_thinking: false,
        supports_cache_control: false,
        context_window: 0,
        cost_rates: None,
    }
}

#[test]
fn codex_identity_constants_match_the_cli_contract() {
    assert_eq!(CODEX_ORIGINATOR, "codex_cli_rs");
    assert_eq!(CODEX_OPENAI_BETA, "responses=experimental");
    assert_eq!(CODEX_USER_AGENT, "codex_cli_rs/0.159.0");
}

#[test]
fn system_messages_are_hoisted_into_instructions() {
    let messages = vec![
        Message::new("system", "be brief"),
        Message::new("developer", "cite files"),
        Message::new("user", "hi"),
    ];
    let body = build_request_body(
        &config(),
        &messages,
        &LlmStreamOptions::default(),
        None,
        true,
    );

    assert_eq!(body["instructions"], json!("be brief\n\ncite files"));
    let roles: Vec<&str> = body["input"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item.get("role").and_then(|role| role.as_str()))
        .collect();
    assert_eq!(roles, vec!["user"], "no system item may reach the backend");
}

#[test]
fn the_body_is_stateless_and_asks_for_encrypted_reasoning() {
    let body = build_request_body(
        &config(),
        &[Message::new("user", "hi")],
        &LlmStreamOptions::default(),
        None,
        true,
    );

    assert_eq!(body["store"], json!(false));
    assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
    assert!(body.get("prompt_cache_key").is_none(), "null is dropped");
    assert_eq!(body["stream"], json!(true));
}

#[test]
fn a_missing_output_limit_gets_the_backend_placeholder() {
    let body = build_request_body(
        &config(),
        &[Message::new("user", "hi")],
        &LlmStreamOptions::default(),
        None,
        false,
    );
    assert_eq!(body["max_output_tokens"], json!(32_000));

    let options = LlmStreamOptions {
        max_tokens: Some(512),
        ..Default::default()
    };
    let body = build_request_body(
        &config(),
        &[Message::new("user", "hi")],
        &options,
        None,
        false,
    );
    assert_eq!(body["max_output_tokens"], json!(512));
}

#[test]
fn thinking_levels_map_onto_codex_efforts() {
    let effort = |level| {
        let options = LlmStreamOptions {
            thinking_level: Some(level),
            ..Default::default()
        };
        build_request_body(
            &config(),
            &[Message::new("user", "hi")],
            &options,
            None,
            true,
        )
        .get("reasoning")
        .and_then(|reasoning| reasoning.get("effort"))
        .and_then(|effort| effort.as_str())
        .map(str::to_string)
    };

    assert_eq!(effort(ThinkingLevel::Low).as_deref(), Some("low"));
    assert_eq!(effort(ThinkingLevel::Max).as_deref(), Some("xhigh"));
    assert_eq!(effort(ThinkingLevel::None), None);
}

#[test]
fn tools_are_flat_responses_functions() {
    let tools = vec![json!({
        "type": "function",
        "function": {
            "name": "read_file",
            "description": "read",
            "parameters": { "type": "object", "properties": {} }
        }
    })];
    let converted = convert_tools(Some(&tools));
    let first = &converted[0];

    assert_eq!(first["type"], json!("function"));
    assert_eq!(first["name"], json!("read_file"));
    assert_eq!(first["parameters"]["type"], json!("object"));
    assert!(first.get("function").is_none());
}

#[test]
fn tool_calls_and_results_use_function_call_items() {
    let mut assistant = Message::new("assistant", "");
    assistant.tool_calls = Some(vec![MessageToolCall {
        id: "call-1".to_string(),
        name: "read_file".to_string(),
        arguments: "{\"path\":\"a\"}".to_string(),
    }]);
    let mut result = Message::new("tool", "contents");
    result.tool_call_id = Some("call-1".to_string());

    let body = build_request_body(
        &config(),
        &[assistant, result],
        &LlmStreamOptions::default(),
        None,
        true,
    );

    assert_eq!(body["input"][0]["type"], json!("function_call"));
    assert_eq!(body["input"][0]["call_id"], json!("call-1"));
    assert_eq!(body["input"][1]["type"], json!("function_call_output"));
    assert_eq!(body["input"][1]["output"], json!("contents"));
}

#[test]
fn multipart_user_messages_keep_images() {
    let message = Message {
        role: "user".to_string(),
        content: MessageContent::Parts(vec![
            ContentPart::Text(TextContentPart {
                content_type: "text".to_string(),
                text: "look".to_string(),
            }),
            ContentPart::ImageUrl(ImageUrlContentPart {
                content_type: "image_url".to_string(),
                image_url: ImageUrl {
                    url: "data:image/png;base64,AAA".to_string(),
                },
            }),
        ]),
        thinking: None,
        thinking_signature: None,
        redacted_thinking: None,
        tool_call_id: None,
        tool_calls: None,
        cache_control: false,
    };

    let body = build_request_body(
        &config(),
        &[message],
        &LlmStreamOptions::default(),
        None,
        true,
    );
    let content = &body["input"][0]["content"];

    assert_eq!(content[0]["type"], json!("input_text"));
    assert_eq!(content[1]["type"], json!("input_image"));
}

#[test]
fn completion_chunks_carry_text_reasoning_and_usage() {
    let body = json!({
        "status": "completed",
        "output": [
            { "type": "reasoning", "summary": [{ "type": "summary_text", "text": "thinking" }] },
            { "type": "message", "content": [{ "type": "output_text", "text": "hello" }] },
            { "type": "function_call", "call_id": "call-9", "name": "read_file", "arguments": "{\"path\":\"a\"}" }
        ],
        "usage": {
            "input_tokens": 10,
            "output_tokens": 4,
            "input_tokens_details": { "cached_tokens": 2 },
            "output_tokens_details": { "reasoning_tokens": 1 }
        }
    });

    let chunks = completion_chunks(&body, None);

    assert!(
        matches!(&chunks[0], crate::types::LlmStreamChunk::Thinking(text) if text == "thinking")
    );
    assert!(matches!(&chunks[1], crate::types::LlmStreamChunk::Text(text) if text == "hello"));
    assert!(matches!(
        &chunks[2],
        crate::types::LlmStreamChunk::ToolCall { id, name, .. }
            if id == "call-9" && name == "read_file"
    ));
    match &chunks[3] {
        crate::types::LlmStreamChunk::Usage {
            prompt_eval_count,
            cached_prompt_eval_count,
            eval_count,
            reasoning_eval_count,
            ..
        } => {
            assert_eq!(*prompt_eval_count, Some(10));
            assert_eq!(*cached_prompt_eval_count, Some(2));
            assert_eq!(*eval_count, Some(4));
            assert_eq!(*reasoning_eval_count, Some(1));
        }
        other => panic!("expected usage, got {other:?}"),
    }
    assert!(matches!(
        &chunks[4],
        crate::types::LlmStreamChunk::Done(reason) if reason.as_deref() == Some("stop")
    ));
}

#[test]
fn an_incomplete_turn_reports_length() {
    let body = json!({ "status": "incomplete", "output": [] });
    let chunks = completion_chunks(&body, None);

    assert!(matches!(
        chunks.last(),
        Some(crate::types::LlmStreamChunk::Done(reason)) if reason.as_deref() == Some("length")
    ));
}
