use crate::types::{ContentPart, Message, MessageContent, TextContentPart};

pub(super) fn transform_messages(messages: Vec<Message>) -> Vec<Message> {
    let mut transformed = vec![Message::new("system", "You are CodeBuddy Code.")];

    transformed.extend(
        messages
            .into_iter()
            .filter(|message| !matches!(message.role.as_str(), "system" | "developer"))
            .map(transform_message),
    );

    transformed
}

fn transform_message(mut message: Message) -> Message {
    if message.role == "user"
        && let MessageContent::Text(text) = message.content
    {
        message.content = MessageContent::Parts(vec![ContentPart::Text(TextContentPart {
            content_type: "text".to_string(),
            text,
        })]);
    }

    message
}

pub(super) fn transform_body(body: &mut serde_json::Value) {
    body["stream"] = serde_json::json!(true);

    match body
        .get("reasoning_effort")
        .and_then(serde_json::Value::as_str)
    {
        Some("none") | Some("off") => {
            if let Some(object) = body.as_object_mut() {
                object.remove("reasoning_effort");
                object.remove("reasoning_summary");
            }
        }
        Some(_) => body["reasoning_summary"] = serde_json::json!("auto"),
        None => {}
    }
}
