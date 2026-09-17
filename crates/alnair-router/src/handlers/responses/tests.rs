use super::*;
use serde_json::json;

fn request(input: serde_json::Value) -> ResponsesRequest {
    serde_json::from_value(json!({
        "model": "sse",
        "input": input,
    }))
    .expect("valid request")
}

#[test]
fn plain_string_input_becomes_one_user_message() {
    let request = request(json!("hello"));
    let messages = build_messages(&request).expect("messages");

    assert_eq!(messages.len(), 1);
    assert!(chat_backend::message_is_text(&messages[0]));
}

#[test]
fn empty_input_is_rejected() {
    assert!(build_messages(&request(json!("   "))).is_err());
    assert!(build_messages(&request(json!([]))).is_err());
}

#[test]
fn instructions_become_a_leading_system_message() {
    let request: ResponsesRequest = serde_json::from_value(json!({
        "model": "sse",
        "instructions": "be terse",
        "input": "hi",
    }))
    .expect("valid request");

    let messages = build_messages(&request).expect("messages");
    assert_eq!(messages.len(), 2);
}

#[test]
fn developer_role_is_mapped_onto_system() {
    let request = request(json!([
        { "role": "developer", "content": "rules" },
        { "role": "user", "content": "hi" }
    ]));

    let messages = build_messages(&request).expect("messages");
    assert_eq!(messages.len(), 2);
}

/// A tool turn is a `function_call` followed by a `function_call_output`; both
/// must survive the round trip or the agent loop stalls.
#[test]
fn function_call_items_round_trip_with_their_outputs() {
    let request = request(json!([
        { "role": "user", "content": "read a file" },
        {
            "type": "function_call",
            "call_id": "call_1",
            "name": "read_file",
            "arguments": "{\"path\":\"/tmp\"}"
        },
        {
            "type": "function_call_output",
            "call_id": "call_1",
            "output": "file contents"
        }
    ]));

    let messages = build_messages(&request).expect("messages");

    assert_eq!(messages.len(), 3, "user, assistant tool call, tool result");
    assert_eq!(messages[1].role, "assistant");
    assert_eq!(messages[2].role, "tool");
}

#[test]
fn consecutive_function_calls_group_into_one_assistant_message() {
    let request = request(json!([
        { "role": "user", "content": "do two things" },
        { "type": "function_call", "call_id": "call_1", "name": "a", "arguments": "{}" },
        { "type": "function_call", "call_id": "call_2", "name": "b", "arguments": "{}" },
        { "type": "function_call_output", "call_id": "call_1", "output": "1" },
        { "type": "function_call_output", "call_id": "call_2", "output": "2" }
    ]));

    let messages = build_messages(&request).expect("messages");

    assert_eq!(
        messages.len(),
        4,
        "user, one assistant call message, two tools"
    );
    assert_eq!(messages[1].role, "assistant");
    assert_eq!(messages[2].role, "tool");
    assert_eq!(messages[3].role, "tool");
}

#[test]
fn function_call_output_without_call_id_is_rejected() {
    let request = request(json!([
        { "role": "user", "content": "hi" },
        { "type": "function_call_output", "output": "orphan" }
    ]));

    assert!(build_messages(&request).is_err());
}

#[test]
fn content_parts_accept_text_and_images() {
    let request = request(json!([
        {
            "role": "user",
            "content": [
                { "type": "input_text", "text": "what is this" },
                { "type": "input_image", "image_url": "https://example.com/a.png" }
            ]
        }
    ]));

    let messages = build_messages(&request).expect("messages");
    assert_eq!(messages.len(), 1);
    assert!(!chat_backend::message_is_text(&messages[0]));
}

/// Responses declares tools flat; providers expect them nested under `function`.
#[test]
fn flat_function_tools_are_nested_for_the_provider_layer() {
    let tools = normalize_tools(Some(vec![json!({
        "type": "function",
        "name": "search",
        "description": "search docs",
        "parameters": { "type": "object", "properties": {} }
    })]))
    .expect("tools");

    assert_eq!(tools[0]["function"]["name"], "search");
    assert_eq!(tools[0]["function"]["description"], "search docs");
    assert_eq!(tools[0]["function"]["parameters"]["type"], "object");
}

#[test]
fn already_nested_tools_pass_through_untouched() {
    let nested = json!({
        "type": "function",
        "function": { "name": "search", "parameters": { "type": "object" } }
    });

    let tools = normalize_tools(Some(vec![nested.clone()])).expect("tools");
    assert_eq!(tools[0], nested);
}

#[test]
fn empty_tool_lists_normalize_to_none() {
    assert!(normalize_tools(Some(Vec::new())).is_none());
    assert!(normalize_tools(None).is_none());
}

#[test]
fn non_function_tools_are_left_alone() {
    let tool = json!({ "type": "web_search_preview" });
    let tools = normalize_tools(Some(vec![tool.clone()])).expect("tools");
    assert_eq!(tools[0], tool);
}

fn with_choice(choice: serde_json::Value) -> ResponsesRequest {
    serde_json::from_value(json!({
        "model": "sse",
        "input": "hi",
        "tools": [{
            "type": "function",
            "name": "search",
            "parameters": { "type": "object", "properties": {} }
        }],
        "tool_choice": choice,
    }))
    .expect("valid request")
}

#[test]
fn tool_choice_none_drops_the_tools() {
    let request = with_choice(json!("none"));
    assert!(resolve_tools(&request).expect("tools").is_none());
}

#[test]
fn tool_choice_auto_keeps_the_tools() {
    let request = with_choice(json!("auto"));
    assert!(resolve_tools(&request).expect("tools").is_some());
}

#[test]
fn tool_choice_required_is_rejected_rather_than_downgraded() {
    let error = resolve_tools(&with_choice(json!("required"))).expect_err("denied");
    assert!(error.to_string().contains("required"), "{error}");
}

#[test]
fn naming_a_function_in_tool_choice_is_rejected() {
    let request = with_choice(json!({ "type": "function", "name": "search" }));
    assert!(resolve_tools(&request).is_err());
}

#[test]
fn absent_tool_choice_keeps_the_tools() {
    let request: ResponsesRequest = serde_json::from_value(json!({
        "model": "sse",
        "input": "hi",
        "tools": [{ "type": "function", "name": "search" }],
    }))
    .expect("valid request");

    assert!(resolve_tools(&request).expect("tools").is_some());
}
