//! Tests for the Anthropic wire shape: thinking, tool pairing, and effort.

use super::*;

fn request(body: serde_json::Value) -> crate::protocol::openai::ChatCompletionRequest {
    let request: MessagesRequest = serde_json::from_value(body).expect("valid request");
    request.into_chat_request().expect("converts")
}

#[test]
fn thinking_budget_maps_to_reasoning_effort() {
    let chat = request(serde_json::json!({
        "model": "claude",
        "max_tokens": 1024,
        "thinking": { "type": "enabled", "budget_tokens": 8192 },
        "messages": [{ "role": "user", "content": "hi" }]
    }));

    assert_eq!(
        chat.generation_options()
            .expect("options")
            .thinking_level
            .as_deref(),
        Some("medium")
    );
}

#[test]
fn disabled_thinking_produces_no_reasoning_effort() {
    let chat = request(serde_json::json!({
        "model": "claude",
        "max_tokens": 1024,
        "thinking": { "type": "disabled" },
        "messages": [{ "role": "user", "content": "hi" }]
    }));

    assert!(
        chat.generation_options()
            .expect("options")
            .thinking_level
            .is_none()
    );
}

#[test]
fn signed_thinking_and_tool_blocks_round_trip() {
    let chat = request(serde_json::json!({
        "model": "claude",
        "max_tokens": 1024,
        "messages": [
            { "role": "user", "content": "hi" },
            { "role": "assistant", "content": [
                { "type": "redacted_thinking", "data": "opaque" },
                { "type": "thinking", "thinking": "reasoning", "signature": "sig-1" },
                { "type": "tool_use", "id": "tc1", "name": "read", "input": { "path": "/tmp" } }
            ]},
            { "role": "user", "content": [
                { "type": "tool_result", "tool_use_id": "tc1", "content": "file" }
            ]}
        ]
    }));

    let messages = chat.into_messages().expect("messages");

    let assistant = messages
        .iter()
        .find(|message| message.role == "assistant")
        .expect("assistant turn");
    assert_eq!(assistant.thinking.as_deref(), Some("reasoning"));
    assert_eq!(assistant.thinking_signature.as_deref(), Some("sig-1"));
    assert_eq!(assistant.redacted_thinking.as_deref(), Some("opaque"));
    assert_eq!(
        assistant
            .tool_calls
            .as_ref()
            .and_then(|calls| calls.first())
            .map(|call| call.id.as_str()),
        Some("tc1")
    );

    let tool = messages
        .iter()
        .find(|message| message.role == "tool")
        .expect("tool result turn");
    assert_eq!(tool.tool_call_id.as_deref(), Some("tc1"));
    assert_eq!(tool.content.as_text(), "file");
}

#[test]
fn tool_result_without_text_preserves_the_pairing() {
    let chat = request(serde_json::json!({
        "model": "claude",
        "max_tokens": 1024,
        "messages": [
            { "role": "assistant", "content": [
                { "type": "tool_use", "id": "tc1", "name": "read", "input": {} }
            ]},
            { "role": "user", "content": [
                { "type": "tool_result", "tool_use_id": "tc1" }
            ]}
        ]
    }));

    let messages = chat.into_messages().expect("messages");

    // The empty assistant turn must survive so the tool_result still pairs.
    assert!(messages.iter().any(|message| message.role == "assistant"));
    assert!(messages.iter().any(
        |message| message.role == "tool" && message.tool_call_id.as_deref() == Some("tc1")
    ));
}
