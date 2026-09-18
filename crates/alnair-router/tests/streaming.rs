//! Streaming translation tests: what the router writes back to SSE clients.
//!
//! An in-process OpenAI-compatible mock upstream stands in for the provider, so
//! the whole path (provider → chunk seam → SSE) is exercised without network
//! access. The regression these cover is a coding agent seeing a clean `200`
//! with an empty stream, because tool calls and reasoning were dropped on the
//! way out.

use alnair_router::config::RouterConfig;
use alnair_router::db::Db;
use alnair_router::{AppState, build_router};
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use tower::ServiceExt;

const TEST_SECRET: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

/// A text reply that also requests a tool call, then reports usage.
const TOOL_CALL_STREAM: &str = r#"data: {"choices":[{"delta":{"role":"assistant","content":"checking"}}]}

data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"read_file","arguments":"{\"path\":\""}}]}}]}

data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"/tmp\"}"}}]}}]}

data: {"choices":[{"delta":{},"finish_reason":"tool_calls"}]}

data: {"choices":[],"usage":{"prompt_tokens":11,"completion_tokens":4,"total_tokens":15}}

data: [DONE]

"#;

/// A reply that streams reasoning before its content and stops on length.
const REASONING_STREAM: &str = r#"data: {"choices":[{"delta":{"reasoning_content":"weighing options"}}]}

data: {"choices":[{"delta":{"content":"here you go"}}]}

data: {"choices":[{"delta":{},"finish_reason":"length"}]}

data: {"choices":[],"usage":{"prompt_tokens":7,"completion_tokens":2,"total_tokens":9}}

data: [DONE]

"#;

/// An Anthropic-native Messages stream that emits a signed thinking block.
const ANTHROPIC_THINKING_STREAM: &str = r#"event: message_start
data: {"type":"message_start","message":{"usage":{"input_tokens":5,"output_tokens":1}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"pondering"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig-abc"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"answer"}}

event: content_block_stop
data: {"type":"content_block_stop","index":1}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":4}}

"#;

/// Builds an app over a fresh in-memory database, without client auth.
async fn app() -> axum::Router {
    let db = Db::connect_in_memory().await.expect("db");
    let mut config = RouterConfig::default();
    config.secrets.key = Some(TEST_SECRET.to_string());
    let state = AppState::new(config, db).expect("state");
    build_router(state)
}

/// Serves `body` as an SSE completion on the OpenAI-compatible chat path.
async fn spawn_sse_upstream(body: &'static str) -> String {
    let router = axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(move || async move {
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/event-stream")],
                body,
            )
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock upstream");
    let address = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    format!("http://{address}/v1")
}

/// Serves `body` as an SSE Messages response on the Anthropic-native path.
async fn spawn_anthropic_upstream(body: &'static str) -> String {
    let router = axum::Router::new().route(
        "/v1/messages",
        axum::routing::post(move || async move {
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/event-stream")],
                body,
            )
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock upstream");
    let address = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    format!("http://{address}/v1")
}

async fn json_request(
    app: &axum::Router,
    method: &str,
    path: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .expect("request");

    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();

    let payload = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    };

    (status, payload)
}

/// Points a fresh alias at the mock upstream and returns its model reference.
async fn route_model_to(app: &axum::Router, base_url: &str) -> String {
    route_model_with_type(app, base_url, "openai-compatible").await
}

async fn route_model_with_type(app: &axum::Router, base_url: &str, provider_type: &str) -> String {
    let (status, connection) = json_request(
        app,
        "POST",
        "/api/connections",
        serde_json::json!({
            "name": "sse-upstream",
            "provider_type": provider_type,
            "base_url": base_url,
            "api_key": "sk-test"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "unexpected body: {connection}");

    let connection_id = connection["id"].as_str().expect("connection id");
    let (status, alias) = json_request(
        app,
        "POST",
        "/api/aliases",
        serde_json::json!({
            "prefix": "sse",
            "connection_id": connection_id,
            "model_override": "test-model"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "unexpected body: {alias}");

    "sse".to_string()
}

/// POSTs a request and returns the raw body, which for these is SSE text.
async fn post_stream(
    app: &axum::Router,
    path: &str,
    body: serde_json::Value,
) -> (StatusCode, String, Option<String>) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .expect("request");

    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();

    (
        status,
        String::from_utf8_lossy(&bytes).to_string(),
        content_type,
    )
}

/// Extracts the JSON payloads of an SSE body, skipping keep-alives and `[DONE]`.
fn sse_payloads(body: &str) -> Vec<serde_json::Value> {
    body.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter(|data| *data != "[DONE]")
        .filter_map(|data| serde_json::from_str(data).ok())
        .collect()
}

#[tokio::test]
async fn streaming_forwards_tool_calls_and_finish_reason() {
    let app = app().await;
    let base_url = spawn_sse_upstream(TOOL_CALL_STREAM).await;
    let model = route_model_to(&app, &base_url).await;

    let (status, body, content_type) = post_stream(
        &app,
        "/v1/chat/completions",
        serde_json::json!({
            "model": model,
            "stream": true,
            "messages": [{ "role": "user", "content": "read a file" }]
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "unexpected body: {body}");
    assert_eq!(content_type.as_deref(), Some("text/event-stream"));
    assert!(
        body.contains("data: [DONE]"),
        "stream must terminate: {body}"
    );

    let payloads = sse_payloads(&body);

    // The stream opens with the assistant role, as OpenAI clients expect.
    assert_eq!(
        payloads[0]["choices"][0]["delta"]["role"], "assistant",
        "first chunk should announce the role: {body}"
    );

    // The whole tool call is reassembled and forwarded, not swallowed.
    let call = payloads
        .iter()
        .find_map(|payload| payload["choices"][0]["delta"]["tool_calls"].as_array())
        .expect("a tool_calls delta should reach the client")
        .first()
        .cloned()
        .expect("one tool call");

    assert_eq!(call["index"], 0);
    assert_eq!(call["id"], "call_1");
    assert_eq!(call["type"], "function");
    assert_eq!(call["function"]["name"], "read_file");
    assert_eq!(call["function"]["arguments"], r#"{"path":"/tmp"}"#);

    // `finish_reason` must reflect the tool call; `stop` sends agents in circles.
    let finish_reason = payloads
        .iter()
        .find_map(|payload| payload["choices"][0]["finish_reason"].as_str())
        .expect("a finish_reason should reach the client");
    assert_eq!(finish_reason, "tool_calls");
}

#[tokio::test]
async fn streaming_forwards_reasoning_content() {
    let app = app().await;
    let base_url = spawn_sse_upstream(REASONING_STREAM).await;
    let model = route_model_to(&app, &base_url).await;

    let (status, body, _) = post_stream(
        &app,
        "/v1/chat/completions",
        serde_json::json!({
            "model": model,
            "stream": true,
            "messages": [{ "role": "user", "content": "think" }]
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "unexpected body: {body}");
    let payloads = sse_payloads(&body);

    let reasoning = payloads
        .iter()
        .find_map(|payload| payload["choices"][0]["delta"]["reasoning_content"].as_str())
        .expect("reasoning should reach the client");
    assert_eq!(reasoning, "weighing options");

    let content = payloads
        .iter()
        .find_map(|payload| payload["choices"][0]["delta"]["content"].as_str())
        .expect("content should still reach the client");
    assert_eq!(content, "here you go");

    let finish_reason = payloads
        .iter()
        .find_map(|payload| payload["choices"][0]["finish_reason"].as_str())
        .expect("a finish_reason should reach the client");
    assert_eq!(finish_reason, "length");
}

#[tokio::test]
async fn streaming_usage_chunk_is_opt_in() {
    let app = app().await;
    let base_url = spawn_sse_upstream(REASONING_STREAM).await;
    let model = route_model_to(&app, &base_url).await;

    let (status, body, _) = post_stream(
        &app,
        "/v1/chat/completions",
        serde_json::json!({
            "model": model,
            "stream": true,
            "stream_options": { "include_usage": true },
            "messages": [{ "role": "user", "content": "think" }]
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "unexpected body: {body}");

    let usage = sse_payloads(&body)
        .into_iter()
        .find_map(|payload| payload.get("usage").cloned())
        .expect("include_usage should produce a usage chunk");
    assert_eq!(usage["prompt_tokens"], 7);
    assert_eq!(usage["completion_tokens"], 2);
    assert_eq!(usage["total_tokens"], 9);

    // Without the flag the same upstream emits no usage chunk.
    let (status, body, _) = post_stream(
        &app,
        "/v1/chat/completions",
        serde_json::json!({
            "model": model,
            "stream": true,
            "messages": [{ "role": "user", "content": "think" }]
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "unexpected body: {body}");
    assert!(
        !sse_payloads(&body)
            .iter()
            .any(|payload| payload.get("usage").is_some()),
        "usage must stay off unless requested: {body}"
    );
}

#[tokio::test]
async fn anthropic_streaming_forwards_tool_use_blocks() {
    let app = app().await;
    let base_url = spawn_sse_upstream(TOOL_CALL_STREAM).await;
    let model = route_model_to(&app, &base_url).await;

    let (status, body, _) = post_stream(
        &app,
        "/v1/messages",
        serde_json::json!({
            "model": model,
            "stream": true,
            "max_tokens": 64,
            "messages": [{ "role": "user", "content": "read a file" }]
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "unexpected body: {body}");
    let payloads = sse_payloads(&body);

    assert_eq!(
        payloads[0]["type"], "message_start",
        "Anthropic streams open with message_start: {body}"
    );

    let block_start = payloads
        .iter()
        .find(|payload| {
            payload["type"] == "content_block_start"
                && payload["content_block"]["type"] == "tool_use"
        })
        .unwrap_or_else(|| panic!("a tool_use block should open: {body}"));

    assert_eq!(block_start["content_block"]["id"], "call_1");
    assert_eq!(block_start["content_block"]["name"], "read_file");
    let index = block_start["index"].clone();

    let delta = payloads
        .iter()
        .find(|payload| payload["delta"]["type"] == "input_json_delta")
        .unwrap_or_else(|| panic!("the tool arguments should stream: {body}"));
    assert_eq!(delta["delta"]["type"], "input_json_delta");
    assert_eq!(delta["delta"]["partial_json"], r#"{"path":"/tmp"}"#);
    assert_eq!(delta["index"], index, "the delta belongs to the open block");

    // Every opened block is closed before the message terminates.
    let closed = payloads
        .iter()
        .any(|payload| payload["type"] == "content_block_stop" && payload["index"] == index);
    assert!(closed, "the tool_use block should be closed: {body}");

    let stop_reason = payloads
        .iter()
        .find_map(|payload| payload["delta"]["stop_reason"].as_str())
        .unwrap_or_else(|| panic!("message_delta should carry a stop_reason: {body}"));
    assert_eq!(stop_reason, "tool_use");
}

#[tokio::test]
async fn anthropic_streaming_reports_max_tokens_and_usage() {
    let app = app().await;
    let base_url = spawn_sse_upstream(REASONING_STREAM).await;
    let model = route_model_to(&app, &base_url).await;

    let (status, body, _) = post_stream(
        &app,
        "/v1/messages",
        serde_json::json!({
            "model": model,
            "stream": true,
            "max_tokens": 64,
            "messages": [{ "role": "user", "content": "think" }]
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "unexpected body: {body}");
    let payloads = sse_payloads(&body);

    // Reasoning becomes a thinking block rather than being dropped.
    let thinking = payloads
        .iter()
        .find(|payload| {
            payload["type"] == "content_block_delta" && payload["delta"]["type"] == "thinking_delta"
        })
        .unwrap_or_else(|| panic!("reasoning should stream as thinking: {body}"));
    assert_eq!(thinking["delta"]["thinking"], "weighing options");

    let message_delta = payloads
        .iter()
        .find(|payload| payload["type"] == "message_delta")
        .unwrap_or_else(|| panic!("message_delta should close the message: {body}"));

    assert_eq!(message_delta["delta"]["stop_reason"], "max_tokens");
    assert_eq!(
        message_delta["usage"]["output_tokens"], 2,
        "output tokens should be reported: {body}"
    );

    assert!(
        body.contains("event: message_stop"),
        "unexpected body: {body}"
    );
}

/// Extracts the `event:` names of an SSE body, in order.
fn sse_event_names(body: &str) -> Vec<String> {
    body.lines()
        .filter_map(|line| line.strip_prefix("event: "))
        .map(str::to_string)
        .collect()
}

#[tokio::test]
async fn responses_streaming_emits_the_documented_event_order() {
    let app = app().await;
    let base_url = spawn_sse_upstream(REASONING_STREAM).await;
    let model = route_model_to(&app, &base_url).await;

    let (status, body, content_type) = post_stream(
        &app,
        "/v1/responses",
        serde_json::json!({
            "model": model,
            "stream": true,
            "input": "think"
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "unexpected body: {body}");
    assert_eq!(content_type.as_deref(), Some("text/event-stream"));

    let events = sse_event_names(&body);

    // The stream is bookended by the created/completed pair.
    assert_eq!(events.first().map(String::as_str), Some("response.created"));
    assert_eq!(
        events.last().map(String::as_str),
        Some("response.completed")
    );

    // Reasoning and text arrive as deltas on the documented item types.
    let payloads = sse_payloads(&body);
    let reasoning = payloads
        .iter()
        .find(|payload| payload["type"] == "response.reasoning_summary_text.delta")
        .unwrap_or_else(|| panic!("reasoning should stream: {body}"));
    assert_eq!(reasoning["delta"], "weighing options");

    let text = payloads
        .iter()
        .find(|payload| payload["type"] == "response.output_text.delta")
        .unwrap_or_else(|| panic!("text should stream: {body}"));
    assert_eq!(text["delta"], "here you go");

    // Every event carries a sequence number, and they are strictly increasing.
    let sequences: Vec<u64> = payloads
        .iter()
        .filter_map(|payload| payload["sequence_number"].as_u64())
        .collect();
    assert_eq!(sequences.len(), payloads.len(), "all events are numbered");
    assert!(
        sequences.windows(2).all(|pair| pair[0] < pair[1]),
        "sequence numbers must increase: {sequences:?}"
    );

    // The final response reports usage and the assembled text.
    let completed = payloads
        .iter()
        .find(|payload| payload["type"] == "response.completed")
        .expect("a completed event should close the stream");
    assert_eq!(completed["response"]["status"], "completed");
    assert_eq!(completed["response"]["usage"]["input_tokens"], 7);
    assert_eq!(completed["response"]["usage"]["output_tokens"], 2);
    // Both items are reported in the terminal response, in arrival order.
    assert_eq!(completed["response"]["output"][0]["type"], "reasoning");
    assert_eq!(
        completed["response"]["output"][0]["summary"][0]["text"], "weighing options",
        "the reasoning item should be in the final output: {body}"
    );
    assert_eq!(completed["response"]["output"][1]["type"], "message");
    assert_eq!(
        completed["response"]["output"][1]["content"][0]["text"], "here you go",
        "the text item should be in the final output: {body}"
    );
}

#[tokio::test]
async fn responses_streaming_reports_tool_calls_as_output_items() {
    let app = app().await;
    let base_url = spawn_sse_upstream(TOOL_CALL_STREAM).await;
    let model = route_model_to(&app, &base_url).await;

    let (status, body, _) = post_stream(
        &app,
        "/v1/responses",
        serde_json::json!({
            "model": model,
            "stream": true,
            "input": "read a file",
            "tools": [{
                "type": "function",
                "name": "read_file",
                "parameters": { "type": "object", "properties": {} }
            }]
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "unexpected body: {body}");
    let payloads = sse_payloads(&body);

    let added = payloads
        .iter()
        .find(|payload| {
            payload["type"] == "response.output_item.added"
                && payload["item"]["type"] == "function_call"
        })
        .unwrap_or_else(|| panic!("a function_call item should be opened: {body}"));
    assert_eq!(added["item"]["call_id"], "call_1");
    assert_eq!(added["item"]["name"], "read_file");

    let arguments_done = payloads
        .iter()
        .find(|payload| payload["type"] == "response.function_call_arguments.done")
        .unwrap_or_else(|| panic!("arguments should be completed: {body}"));
    assert_eq!(arguments_done["arguments"], r#"{"path":"/tmp"}"#);

    // The call is reported in the terminal response so agents can act on it.
    let completed = payloads
        .iter()
        .find(|payload| payload["type"] == "response.completed")
        .expect("a completed event should close the stream");
    let call = completed["response"]["output"]
        .as_array()
        .expect("output array")
        .iter()
        .find(|item| item["type"] == "function_call")
        .unwrap_or_else(|| panic!("the tool call should be in the final output: {body}"));
    assert_eq!(call["name"], "read_file");
    assert_eq!(call["arguments"], r#"{"path":"/tmp"}"#);
}

#[tokio::test]
async fn responses_non_streaming_returns_a_json_response_object() {
    let app = app().await;
    let base_url = spawn_sse_upstream(REASONING_STREAM).await;
    let model = route_model_to(&app, &base_url).await;

    let (status, body) = json_request(
        &app,
        "POST",
        "/v1/responses",
        serde_json::json!({
            "model": model,
            "input": "think"
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "unexpected body: {body}");
    assert_eq!(body["object"], "response");
    assert_eq!(body["status"], "completed");
    assert_eq!(body["output"][0]["content"][0]["text"], "here you go");
    assert_eq!(body["usage"]["input_tokens"], 7);
    assert_eq!(body["usage"]["total_tokens"], 9);
}

#[tokio::test]
async fn anthropic_native_streaming_forwards_signed_thinking() {
    let app = app().await;
    let base_url = spawn_anthropic_upstream(ANTHROPIC_THINKING_STREAM).await;
    let model = route_model_with_type(&app, &base_url, "anthropic-native").await;

    let (status, body, _) = post_stream(
        &app,
        "/v1/messages",
        serde_json::json!({
            "model": model,
            "stream": true,
            "max_tokens": 64,
            "thinking": { "type": "enabled", "budget_tokens": 4096 },
            "messages": [{ "role": "user", "content": "think" }]
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "unexpected body: {body}");
    let payloads = sse_payloads(&body);

    let thinking = payloads
        .iter()
        .find(|payload| payload["delta"]["type"] == "thinking_delta")
        .unwrap_or_else(|| panic!("thinking should stream: {body}"));
    assert_eq!(thinking["delta"]["thinking"], "pondering");

    let signature = payloads
        .iter()
        .find(|payload| payload["delta"]["type"] == "signature_delta")
        .unwrap_or_else(|| panic!("the thinking signature must reach the client: {body}"));
    assert_eq!(signature["delta"]["signature"], "sig-abc");
    assert_eq!(
        signature["index"], thinking["index"],
        "the signature belongs to the open thinking block"
    );
}
