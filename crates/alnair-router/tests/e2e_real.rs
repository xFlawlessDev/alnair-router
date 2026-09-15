//! Opt-in end-to-end tests against real providers.
//!
//! These tests are `#[ignore]`d so the default suite stays fully offline. Run
//! them deliberately, with credentials:
//!
//! ```bash
//! ALNAIR_ROUTER_E2E_OPENAI_API_KEY=sk-... \
//!   cargo test -p alnair-router --test e2e_real -- --ignored
//! ```
//!
//! Environment:
//! - `ALNAIR_ROUTER_E2E_OPENAI_API_KEY` (required for the OpenAI tests)
//! - `ALNAIR_ROUTER_E2E_OPENAI_BASE_URL` (default `https://api.openai.com/v1`)
//! - `ALNAIR_ROUTER_E2E_OPENAI_MODEL` (default `gpt-4o-mini`)
//! - `ALNAIR_ROUTER_E2E_ANTHROPIC_API_KEY` (required for the Anthropic test)
//! - `ALNAIR_ROUTER_E2E_ANTHROPIC_BASE_URL` (default `https://api.anthropic.com/v1`)
//! - `ALNAIR_ROUTER_E2E_ANTHROPIC_MODEL` (default `claude-3-5-haiku-latest`)

use alnair_router::config::RouterConfig;
use alnair_router::db::Db;
use alnair_router::{AppState, build_router};
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use tower::ServiceExt;

const TEST_SECRET: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

fn env_key(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!("{name} is required; run with --ignored and the documented env vars")
    })
}

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

/// Builds the real router over an in-memory database with one connection, set
/// as the default connection, and no other configuration.
async fn app_with_connection(provider_type: &str, base_url: &str, api_key: &str) -> axum::Router {
    let db = Db::connect_in_memory().await.expect("db");
    let mut config = RouterConfig::default();
    config.secrets.key = Some(TEST_SECRET.to_string());
    config.router.default_connection = Some("e2e".to_string());
    let state = AppState::new(config, db).expect("state");
    let app = build_router(state);

    let response = raw_request(
        &app,
        "POST",
        "/api/connections",
        Some(serde_json::json!({
            "name": "e2e",
            "provider_type": provider_type,
            "base_url": base_url,
            "api_key": api_key,
        })),
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::CREATED,
        "failed to create the e2e connection"
    );

    app
}

async fn raw_request(
    app: &axum::Router,
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
) -> axum::response::Response {
    let mut builder = Request::builder().method(method).uri(path);
    let body = match body {
        Some(body) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(body.to_string())
        }
        None => Body::empty(),
    };

    app.clone()
        .oneshot(builder.body(body).unwrap())
        .await
        .expect("request")
}

async fn body_text(response: axum::response::Response) -> (StatusCode, String) {
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

#[tokio::test]
#[ignore = "requires ALNAIR_ROUTER_E2E_OPENAI_API_KEY"]
async fn openai_chat_completion_round_trip() {
    let api_key = env_key("ALNAIR_ROUTER_E2E_OPENAI_API_KEY");
    let base_url = env_or(
        "ALNAIR_ROUTER_E2E_OPENAI_BASE_URL",
        "https://api.openai.com/v1",
    );
    let model = env_or("ALNAIR_ROUTER_E2E_OPENAI_MODEL", "gpt-4o-mini");
    let app = app_with_connection("openai-compatible", &base_url, &api_key).await;

    let response = raw_request(
        &app,
        "POST",
        "/v1/chat/completions",
        Some(serde_json::json!({
            "model": model,
            "messages": [{ "role": "user", "content": "Reply with the single word: pong" }],
            "max_tokens": 16
        })),
    )
    .await;

    let routed_model = response
        .headers()
        .get("x-router-model")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    assert!(
        routed_model.is_some(),
        "router headers must report the tier"
    );

    let (status, body) = body_text(response).await;
    assert_eq!(status, StatusCode::OK, "unexpected error body: {body}");

    let payload: serde_json::Value = serde_json::from_str(&body).expect("json body");
    let content = payload["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or_default();
    assert!(!content.trim().is_empty(), "empty completion: {body}");
    assert!(payload["usage"]["total_tokens"].as_u64().unwrap_or(0) > 0);
}

#[tokio::test]
#[ignore = "requires ALNAIR_ROUTER_E2E_OPENAI_API_KEY"]
async fn openai_streaming_completion_round_trip() {
    let api_key = env_key("ALNAIR_ROUTER_E2E_OPENAI_API_KEY");
    let base_url = env_or(
        "ALNAIR_ROUTER_E2E_OPENAI_BASE_URL",
        "https://api.openai.com/v1",
    );
    let model = env_or("ALNAIR_ROUTER_E2E_OPENAI_MODEL", "gpt-4o-mini");
    let app = app_with_connection("openai-compatible", &base_url, &api_key).await;

    let response = raw_request(
        &app,
        "POST",
        "/v1/chat/completions",
        Some(serde_json::json!({
            "model": model,
            "stream": true,
            "messages": [{ "role": "user", "content": "Reply with the single word: pong" }],
            "max_tokens": 16
        })),
    )
    .await;

    let (status, body) = body_text(response).await;
    assert_eq!(status, StatusCode::OK, "unexpected error body: {body}");
    assert!(
        body.contains("chat.completion.chunk"),
        "expected OpenAI SSE chunks, got: {body}"
    );
    assert!(body.contains("[DONE]"), "stream must terminate: {body}");
}

#[tokio::test]
#[ignore = "requires ALNAIR_ROUTER_E2E_ANTHROPIC_API_KEY"]
async fn anthropic_messages_round_trip() {
    let api_key = env_key("ALNAIR_ROUTER_E2E_ANTHROPIC_API_KEY");
    let base_url = env_or(
        "ALNAIR_ROUTER_E2E_ANTHROPIC_BASE_URL",
        "https://api.anthropic.com/v1",
    );
    let model = env_or(
        "ALNAIR_ROUTER_E2E_ANTHROPIC_MODEL",
        "claude-3-5-haiku-latest",
    );
    let app = app_with_connection("anthropic-native", &base_url, &api_key).await;

    let response = raw_request(
        &app,
        "POST",
        "/v1/messages",
        Some(serde_json::json!({
            "model": model,
            "max_tokens": 16,
            "messages": [{ "role": "user", "content": "Reply with the single word: pong" }]
        })),
    )
    .await;

    let (status, body) = body_text(response).await;
    assert_eq!(status, StatusCode::OK, "unexpected error body: {body}");

    let payload: serde_json::Value = serde_json::from_str(&body).expect("json body");
    assert_eq!(payload["type"], "message");
    let text = payload["content"][0]["text"].as_str().unwrap_or_default();
    assert!(!text.trim().is_empty(), "empty completion: {body}");
    assert!(payload["usage"]["output_tokens"].as_u64().unwrap_or(0) > 0);
}
