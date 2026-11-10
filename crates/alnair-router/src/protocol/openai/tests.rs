//! Tests for the OpenAI wire shape: content parts, web search, and mapping.

use super::*;

use crate::error::Result;
use crate::upstream::chat_backend::{self, RouterMessage};

fn message_from(content: serde_json::Value) -> Result<RouterMessage> {
    let request: ChatCompletionRequest = serde_json::from_value(serde_json::json!({
        "model": "gpt-4o-audio-preview",
        "messages": [ { "role": "user", "content": content } ],
    }))
    .expect("valid request");
    request
        .into_messages()
        .map(|mut messages| messages.remove(0))
}

fn parts_of(message: &RouterMessage) -> Vec<serde_json::Value> {
    chat_backend::message_parts_json(message).expect("expected a multipart message")
}

#[test]
fn an_input_audio_part_survives_the_round_trip_verbatim() {
    let message = message_from(serde_json::json!([
        { "type": "text", "text": "apa ini?" },
        { "type": "input_audio", "input_audio": { "data": "UklGRg==", "format": "wav" } }
    ]))
    .expect("message");

    let parts = parts_of(&message);
    assert_eq!(parts.len(), 2);
    let raw = &parts[1];
    assert_eq!(raw["type"], "input_audio");
    assert_eq!(raw["input_audio"]["data"], "UklGRg==");
    assert_eq!(raw["input_audio"]["format"], "wav");
}

#[test]
fn url_shaped_media_parts_are_carried_through() {
    for (kind, payload) in [
        ("audio_url", serde_json::json!({ "url": "https://x/a.mp3" })),
        ("video_url", serde_json::json!({ "url": "https://x/a.mp4" })),
        ("file", serde_json::json!({ "file_data": "JVBERi0=" })),
    ] {
        let message = message_from(serde_json::json!([{ "type": kind, kind: payload }]))
            .unwrap_or_else(|error| panic!("{kind} rejected: {error}"));

        let parts = parts_of(&message);
        let raw = &parts[0];
        assert_eq!(raw["type"], kind);
    }
}

#[test]
fn unknown_part_types_are_rejected_instead_of_dropped() {
    let error = message_from(serde_json::json!([{ "type": "input_smell", "smell": "?" }]))
        .expect_err("unknown part must be rejected");

    assert!(error.to_string().contains("unsupported content part type"));
}

fn web_search_request(options: serde_json::Value) -> ChatCompletionRequest {
    serde_json::from_value(serde_json::json!({
        "model": "gpt-5-search-api",
        "messages": [ { "role": "user", "content": "hi" } ],
        "web_search_options": options,
    }))
    .expect("valid request")
}

#[test]
fn web_search_options_reach_the_generation_options_untouched() {
    let request = web_search_request(serde_json::json!({
        "search_context_size": "medium",
        "user_location": { "type": "approximate", "country": "ID" }
    }));

    request
        .validate_web_search_options()
        .expect("valid options");

    let options = request.generation_options().expect("options");
    let carried = options.web_search_options.expect("carried through");
    assert_eq!(carried["search_context_size"], "medium");
    assert_eq!(carried["user_location"]["country"], "ID");
}

#[test]
fn a_malformed_web_search_options_is_rejected() {
    for options in [
        serde_json::json!("medium"),
        serde_json::json!([{ "search_context_size": "low" }]),
        serde_json::json!({ "search_context_size": 3 }),
        serde_json::json!({ "search_context_size": "huge" }),
    ] {
        let request = web_search_request(options.clone());
        assert!(
            request.validate_web_search_options().is_err(),
            "{options} should be rejected"
        );
    }
}

#[test]
fn a_request_without_web_search_options_is_valid() {
    let request: ChatCompletionRequest = serde_json::from_value(serde_json::json!({
        "model": "gpt-4o",
        "messages": [ { "role": "user", "content": "hi" } ],
    }))
    .expect("valid request");

    request
        .validate_web_search_options()
        .expect("absent options are fine");
    assert!(
        request
            .generation_options()
            .expect("options")
            .web_search_options
            .is_none()
    );
}

#[test]
fn a_media_part_missing_its_payload_is_rejected() {
    let error = message_from(serde_json::json!([{ "type": "input_audio" }]))
        .expect_err("payload-less part must be rejected");
    assert!(error.to_string().contains("missing its"));

    let error = message_from(serde_json::json!([
        { "type": "input_audio", "input_audio": { "data": "  ", "format": "wav" } }
    ]))
    .expect_err("empty audio data must be rejected");
    assert!(error.to_string().contains("non-empty base64"));

    let error = message_from(serde_json::json!([
        { "type": "input_audio", "input_audio": { "data": "UklGRg==" } }
    ]))
    .expect_err("audio without a format must be rejected");
    assert!(error.to_string().contains("format"));

    let error = message_from(serde_json::json!([
        { "type": "video_url", "video_url": { "url": "" } }
    ]))
    .expect_err("empty video url must be rejected");
    assert!(error.to_string().contains("non-empty `url`"));
}
