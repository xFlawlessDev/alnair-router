use super::request::{transform_body, transform_messages};
use crate::types::{ContentPart, Message, MessageContent};

#[test]
fn messages_use_codebuddy_system_prompt_and_text_blocks() {
    let messages = transform_messages(vec![
        Message::new("system", "caller system"),
        Message::new("developer", "caller developer"),
        Message::new("user", "hello"),
        Message::new("assistant", "hi"),
    ]);

    assert_eq!(messages[0].role, "system");
    assert_eq!(messages[0].content.as_text(), "You are CodeBuddy Code.");
    assert_eq!(messages.len(), 3);
    assert!(matches!(
        messages[1].content,
        MessageContent::Parts(ref parts)
            if matches!(parts.first(), Some(ContentPart::Text(part)) if part.text == "hello")
    ));
}

#[test]
fn messages_preserve_structured_content_and_tool_metadata() {
    let mut message = Message::new("user", "ignored");
    message.content =
        MessageContent::Parts(vec![ContentPart::Text(crate::types::TextContentPart {
            content_type: "text".to_string(),
            text: "structured".to_string(),
        })]);

    let transformed = transform_messages(vec![message]);
    assert!(matches!(transformed[1].content, MessageContent::Parts(_)));
}

#[test]
fn body_forces_stream_and_adds_reasoning_summary() {
    let mut body = serde_json::json!({"stream": false, "reasoning_effort": "medium"});
    transform_body(&mut body);

    assert_eq!(body["stream"], true);
    assert_eq!(body["reasoning_effort"], "medium");
    assert_eq!(body["reasoning_summary"], "auto");
}

#[test]
fn body_removes_disabled_reasoning() {
    for value in ["none", "off"] {
        let mut body = serde_json::json!({"reasoning_effort": value});
        transform_body(&mut body);
        assert!(body.get("reasoning_effort").is_none());
        assert!(body.get("reasoning_summary").is_none());
    }
}

#[test]
fn body_without_reasoning_is_only_forced_to_stream() {
    let mut body = serde_json::json!({"model": "claude-sonnet-4", "stream": false});
    transform_body(&mut body);

    assert_eq!(body["stream"], true);
    assert_eq!(body["model"], "claude-sonnet-4");
    assert!(
        body.get("reasoning_summary").is_none(),
        "a body that asked for no reasoning must not gain a summary flag"
    );
}

#[test]
fn body_transform_is_idempotent() {
    let mut body = serde_json::json!({"stream": false, "reasoning_effort": "low"});
    transform_body(&mut body);
    let once = body.clone();
    transform_body(&mut body);

    assert_eq!(body, once, "applying the transform twice must not drift");
}

#[test]
fn assistant_and_tool_messages_keep_their_shape() {
    let mut assistant = Message::new("assistant", "calling a tool");
    assistant.tool_calls = Some(vec![crate::types::MessageToolCall {
        id: "tc1".to_string(),
        name: "read_file".to_string(),
        arguments: "{}".to_string(),
    }]);
    let tool = Message::new("tool", "result");

    let transformed = transform_messages(vec![assistant, tool]);

    assert_eq!(transformed.len(), 3, "system prompt plus both messages");
    assert_eq!(transformed[1].role, "assistant");
    assert_eq!(
        transformed[1]
            .tool_calls
            .as_ref()
            .expect("tool calls")
            .len(),
        1
    );
    assert_eq!(transformed[2].role, "tool");
    assert_eq!(transformed[2].content.as_text(), "result");
}

#[test]
fn system_and_developer_messages_are_replaced_not_forwarded() {
    let transformed = transform_messages(vec![
        Message::new("system", "caller system"),
        Message::new("developer", "caller developer"),
    ]);

    assert_eq!(transformed.len(), 1);
    assert_eq!(transformed[0].content.as_text(), "You are CodeBuddy Code.");
}
