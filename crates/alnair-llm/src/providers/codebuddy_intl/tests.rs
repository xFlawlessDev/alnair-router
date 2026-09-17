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
