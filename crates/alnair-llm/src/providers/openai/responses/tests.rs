use serde_json::json;

use super::request::{build_request_body, convert_tools};
use super::stream::{
    completion_chunks, handle_event, is_terminal, response_usage, terminal_reason_for_event,
};
use crate::model_config::{AuthStyle, LlmStreamOptions, ModelConfig, ThinkingLevel, WireApi};
use crate::types::{
    ContentPart, ImageUrl, ImageUrlContentPart, LlmStreamChunk, Message, MessageContent,
    MessageToolCall, TextContentPart,
};

fn config() -> ModelConfig {
    ModelConfig {
        provider_type: crate::types::ProviderType::OpenaiCompatible,
        base_url: "https://api.example.com/v1".to_string(),
        model_id: "gpt-5.1".to_string(),
        api_key: Some("sk-test".to_string()),
        auth_style: AuthStyle::Bearer,
        wire_api: WireApi::Responses,
        custom_headers: Default::default(),
        chatgpt_account_id: None,
        supports_vision: false,
        supports_thinking: true,
        supports_cache_control: false,
        context_window: 0,
        cost_rates: None,
    }
}

fn body(
    messages: &[Message],
    options: &LlmStreamOptions,
    tools: Option<&[serde_json::Value]>,
) -> serde_json::Value {
    build_request_body(&config(), messages, options, tools, true)
}

#[test]
fn the_body_uses_input_and_instructions_instead_of_messages() {
    let messages = vec![
        Message::new("system", "be brief"),
        Message::new("developer", "cite files"),
        Message::new("user", "hi"),
    ];

    let body = body(&messages, &LlmStreamOptions::default(), None);

    assert_eq!(body["instructions"], json!("be brief\n\ncite files"));
    assert!(body.get("messages").is_none(), "chat shape must not leak");
    assert_eq!(body["input"][0]["role"], json!("user"));
    assert_eq!(body["input"][0]["content"][0]["type"], json!("input_text"));
    assert_eq!(body["stream"], json!(true));
}

#[test]
fn the_output_limit_is_max_output_tokens() {
    let options = LlmStreamOptions {
        max_tokens: Some(512),
        ..Default::default()
    };

    let body = body(&[Message::new("user", "hi")], &options, None);

    assert_eq!(body["max_output_tokens"], json!(512));
    assert!(body.get("max_tokens").is_none());
}

#[test]
fn sampling_knobs_the_responses_api_does_not_define_are_dropped() {
    let options = LlmStreamOptions {
        temperature: Some(0.2),
        top_p: Some(0.9),
        presence_penalty: Some(1.0),
        frequency_penalty: Some(1.0),
        seed: Some(7),
        stop: Some(vec!["END".to_string()]),
        ..Default::default()
    };

    let body = body(&[Message::new("user", "hi")], &options, None);

    assert_eq!(body["temperature"], json!(0.2));
    assert_eq!(body["top_p"], json!(0.9));
    for rejected in ["presence_penalty", "frequency_penalty", "seed", "stop"] {
        assert!(body.get(rejected).is_none(), "{rejected} must not be sent");
    }
}

#[test]
fn thinking_becomes_a_reasoning_effort() {
    let options = LlmStreamOptions {
        thinking_level: Some(ThinkingLevel::Max),
        ..Default::default()
    };

    let loud = body(&[Message::new("user", "hi")], &options, None);
    assert_eq!(loud["reasoning"]["effort"], json!("max"));

    let quiet = body(
        &[Message::new("user", "hi")],
        &LlmStreamOptions::default(),
        None,
    );
    assert!(quiet.get("reasoning").is_none());
}

#[test]
fn the_top_tier_is_max_not_xhigh() {
    // Responses treats `xhigh` and `max` as two distinct levels; sending
    // `xhigh` would ask for one tier less than the caller did.
    let effort = |level| {
        let options = LlmStreamOptions {
            thinking_level: Some(level),
            ..Default::default()
        };
        body(&[Message::new("user", "hi")], &options, None)["reasoning"]["effort"].clone()
    };

    assert_eq!(effort(ThinkingLevel::Minimal), json!("minimal"));
    assert_eq!(effort(ThinkingLevel::Low), json!("low"));
    assert_eq!(effort(ThinkingLevel::Medium), json!("medium"));
    assert_eq!(effort(ThinkingLevel::High), json!("high"));
    assert_eq!(effort(ThinkingLevel::Max), json!("max"));
}

#[test]
fn tool_results_and_calls_use_the_item_shape() {
    let mut assistant = Message::new("assistant", "");
    assistant.tool_calls = Some(vec![MessageToolCall {
        id: "call-1".to_string(),
        name: "read_file".to_string(),
        arguments: "{\"path\":\"a\"}".to_string(),
    }]);
    let mut result = Message::new("tool", "contents");
    result.tool_call_id = Some("call-1".to_string());

    let body = body(&[assistant, result], &LlmStreamOptions::default(), None);

    assert_eq!(body["input"][0]["type"], json!("function_call"));
    assert_eq!(body["input"][0]["call_id"], json!("call-1"));
    assert_eq!(body["input"][0]["name"], json!("read_file"));
    assert_eq!(body["input"][1]["type"], json!("function_call_output"));
    assert_eq!(body["input"][1]["call_id"], json!("call-1"));
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

    let body = body(&[message], &LlmStreamOptions::default(), None);
    let content = &body["input"][0]["content"];

    assert_eq!(content[0]["type"], json!("input_text"));
    assert_eq!(content[1]["type"], json!("input_image"));
    assert_eq!(content[1]["image_url"], json!("data:image/png;base64,AAA"));
}

#[test]
fn tools_are_declared_flat_and_an_empty_list_is_omitted() {
    let tools = vec![json!({
        "type": "function",
        "function": {
            "name": "read_file",
            "description": "read",
            "parameters": { "type": "object", "properties": {} }
        }
    })];

    let declared = convert_tools(Some(&tools)).expect("tools");
    assert_eq!(declared[0]["name"], json!("read_file"));
    assert!(declared[0].get("function").is_none());

    assert!(convert_tools(None).is_none());
    assert!(convert_tools(Some(&[])).is_none());

    let body = body(
        &[Message::new("user", "hi")],
        &LlmStreamOptions::default(),
        Some(&tools),
    );
    assert_eq!(body["tool_choice"], json!("auto"));
    assert_eq!(body["tools"][0]["name"], json!("read_file"));
}

#[test]
fn text_and_reasoning_deltas_stream_and_blank_ones_are_skipped() {
    let text = handle_event(
        &json!({ "type": "response.output_text.delta", "delta": "hello" }),
        None,
    )
    .expect("no error")
    .expect("a chunk");
    assert!(matches!(text, LlmStreamChunk::Text(text) if text == "hello"));

    let empty = handle_event(
        &json!({ "type": "response.output_text.delta", "delta": "" }),
        None,
    )
    .expect("no error");
    assert!(empty.is_none());

    let reasoning = handle_event(
        &json!({ "type": "response.reasoning_summary_text.delta", "delta": "why" }),
        None,
    )
    .expect("no error")
    .expect("a chunk");
    assert!(matches!(reasoning, LlmStreamChunk::Thinking(text) if text == "why"));

    let raw = handle_event(
        &json!({ "type": "response.reasoning_text.delta", "delta": "raw" }),
        None,
    )
    .expect("no error")
    .expect("a chunk");
    assert!(matches!(raw, LlmStreamChunk::Thinking(text) if text == "raw"));
}

#[test]
fn lifecycle_noise_is_ignored_and_terminals_are_recognized() {
    for noise in [
        "response.created",
        "response.in_progress",
        "response.output_item.added",
        "response.content_part.added",
        "response.web_search_call.searching",
        "response.function_call_arguments.delta",
    ] {
        let event = json!({ "type": noise });
        assert!(
            handle_event(&event, None).expect("no error").is_none(),
            "{noise} must not produce a chunk"
        );
        assert!(!is_terminal(&event), "{noise} must not close the stream");
    }

    for terminal in [
        "response.completed",
        "response.incomplete",
        "response.failed",
    ] {
        assert!(is_terminal(&json!({ "type": terminal })));
    }
}

#[test]
fn a_completed_tool_call_is_emitted_from_its_final_item() {
    // The argument delta events carry no `name`, so the call is only complete
    // once the item lifecycle closes it.
    let ignored = handle_event(
        &json!({
            "type": "response.function_call_arguments.delta",
            "item_id": "fc-1",
            "output_index": 1,
            "delta": "{\"path\":"
        }),
        None,
    )
    .expect("no error");
    assert!(ignored.is_none(), "a partial call must not be emitted");

    let chunk = handle_event(
        &json!({
            "type": "response.output_item.done",
            "output_index": 1,
            "item": {
                "type": "function_call",
                "id": "fc_1",
                "call_id": "call-1",
                "name": "read_file",
                "arguments": "{\"path\":\"a\"}"
            }
        }),
        None,
    )
    .expect("no error")
    .expect("a chunk");

    match chunk {
        LlmStreamChunk::ToolCall {
            id,
            name,
            arguments,
        } => {
            assert_eq!(
                id, "call-1",
                "the call_id pairs the result, not the item id"
            );
            assert_eq!(name, "read_file");
            assert_eq!(arguments["path"], json!("a"));
        }
        other => panic!("expected a tool call, got {other:?}"),
    }

    // A completed message item carries no call.
    assert!(
        handle_event(
            &json!({
                "type": "response.output_item.done",
                "item": { "type": "message", "id": "msg_1", "content": [] }
            }),
            None,
        )
        .expect("no error")
        .is_none()
    );
}

#[test]
fn a_failed_turn_surfaces_the_upstream_message() {
    let error = handle_event(
        &json!({
            "type": "response.failed",
            "response": { "error": { "code": "server_error", "message": "the model fell over" } }
        }),
        None,
    )
    .expect_err("an error");

    assert!(
        error.to_string().contains("the model fell over"),
        "got: {error}"
    );
    assert_eq!(
        terminal_reason_for_event(&json!({ "type": "response.failed" })),
        None
    );
}

#[test]
fn an_output_limit_stop_is_reported_as_length() {
    let event = json!({
        "type": "response.incomplete",
        "response": { "status": "incomplete", "incomplete_details": { "reason": "max_output_tokens" } }
    });

    assert_eq!(terminal_reason_for_event(&event).as_deref(), Some("length"));
    assert_eq!(
        terminal_reason_for_event(&json!({
            "type": "response.completed",
            "response": { "status": "completed" }
        }))
        .as_deref(),
        Some("stop")
    );
}

#[test]
fn usage_is_read_from_the_responses_counters() {
    let body = json!({
        "status": "completed",
        "usage": {
            "input_tokens": 10,
            "output_tokens": 4,
            "input_tokens_details": { "cached_tokens": 2 },
            "output_tokens_details": { "reasoning_tokens": 1 }
        }
    });

    match response_usage(&body, None).expect("usage") {
        LlmStreamChunk::Usage {
            prompt_eval_count,
            cached_prompt_eval_count,
            eval_count,
            reasoning_eval_count,
            ..
        } => {
            assert_eq!(prompt_eval_count, Some(10));
            assert_eq!(cached_prompt_eval_count, Some(2));
            assert_eq!(eval_count, Some(4));
            assert_eq!(reasoning_eval_count, Some(1));
        }
        other => panic!("expected usage, got {other:?}"),
    }

    assert!(response_usage(&json!({ "usage": {} }), None).is_none());
    assert!(response_usage(&json!({}), None).is_none());
}

#[test]
fn a_one_shot_body_yields_text_reasoning_tool_call_usage_and_done() {
    let body = json!({
        "status": "completed",
        "output": [
            { "type": "reasoning", "summary": [{ "type": "summary_text", "text": "thinking" }] },
            { "type": "message", "content": [{ "type": "output_text", "text": "hello" }] },
            {
                "type": "function_call",
                "id": "fc_1",
                "call_id": "call-9",
                "name": "read_file",
                "arguments": "{\"path\":\"a\"}"
            }
        ],
        "usage": { "input_tokens": 10, "output_tokens": 4 }
    });

    let chunks = completion_chunks(&body, None);

    assert!(matches!(&chunks[0], LlmStreamChunk::Thinking(text) if text == "thinking"));
    assert!(matches!(&chunks[1], LlmStreamChunk::Text(text) if text == "hello"));
    assert!(matches!(
        &chunks[2],
        LlmStreamChunk::ToolCall { id, name, .. } if id == "call-9" && name == "read_file"
    ));
    assert!(matches!(&chunks[3], LlmStreamChunk::Usage { .. }));
    assert!(matches!(
        chunks.last(),
        Some(LlmStreamChunk::Done(reason)) if reason.as_deref() == Some("stop")
    ));
}
