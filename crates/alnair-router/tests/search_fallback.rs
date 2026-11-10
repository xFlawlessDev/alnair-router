//! `/v1/search` fallback: a primary provider that is out of quota or rate
//! limited hands the request to the search tiers, lowest tier first. Anything
//! else, including a 404 caused by a wrong path, is returned without fallback.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use alnair_router::config::RouterConfig;
use alnair_router::db::Db;
use alnair_router::{AppState, build_router};
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::routing::post;
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

/// Test encryption key, the same shape `routes.rs` uses.
const TEST_SECRET: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

/// Builds the router over a fresh in-memory database.
async fn app() -> (axum::Router, Db) {
    let mut config = RouterConfig::default();
    config.server.require_api_key = false;
    config.secrets.key = Some(TEST_SECRET.to_string());
    let db = Db::connect_in_memory().await.expect("db");
    let state = AppState::new(config, db.clone()).expect("state");
    (build_router(state), db)
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
    let value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, value)
}

/// Creates a connection that searches at `base_url`, optionally in a tier.
async fn create_search_connection(
    app: &axum::Router,
    name: &str,
    base_url: &str,
    tier: Option<i64>,
) {
    let mut payload = json!({
        "name": name,
        "provider_type": "openai-compatible",
        "base_url": base_url,
        "api_key": "sk-search",
        "search_path": "/search",
    });
    if let Some(tier) = tier {
        payload["search_tier"] = json!(tier);
    }
    let (status, body) = json_request(app, "POST", "/api/connections", payload).await;
    assert_eq!(status, StatusCode::CREATED, "unexpected body: {body}");
}

/// Points an alias at a connection, so a request can name it as `prefix/...`.
async fn alias_connection(app: &axum::Router, prefix: &str, connection_name: &str) {
    let (_, connections) = json_request(app, "GET", "/api/connections", json!({})).await;
    let id = connections
        .as_array()
        .and_then(|list| list.iter().find(|c| c["name"] == connection_name))
        .and_then(|c| c["id"].as_str())
        .expect("connection id")
        .to_string();
    let (status, body) = json_request(
        app,
        "POST",
        "/api/aliases",
        json!({ "prefix": prefix, "connection_id": id, "model_override": "chat-model" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "unexpected body: {body}");
}

/// A mock provider that answers every `/v1/search` with a fixed status, tagged
/// with a label, and counts its calls.
async fn spawn_provider(status: u16, label: &'static str) -> (String, Arc<AtomicUsize>) {
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    let router = axum::Router::new().route(
        "/v1/search",
        post(move || {
            let counter = counter.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                let code = StatusCode::from_u16(status).expect("valid status");
                (code, axum::Json(json!({ "from": label })))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock provider");
    let address = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    (format!("http://{address}/v1"), hits)
}

#[tokio::test]
async fn quota_on_the_primary_falls_back_to_a_search_tier() {
    let (app, _db) = app().await;
    let (primary, primary_hits) = spawn_provider(429, "primary").await;
    let (search, search_hits) = spawn_provider(200, "search").await;

    create_search_connection(&app, "chatty", &primary, None).await;
    alias_connection(&app, "chatty", "chatty").await;
    create_search_connection(&app, "exa-like", &search, Some(1)).await;

    let (status, body) = json_request(
        &app,
        "POST",
        "/v1/search",
        json!({ "model": "chatty", "query": "rust" }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "fallback should answer: {body}");
    assert_eq!(body["from"], "search");
    assert_eq!(primary_hits.load(Ordering::SeqCst), 1);
    assert_eq!(search_hits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_404_does_not_fall_back_so_a_wrong_path_stays_visible() {
    let (app, _db) = app().await;
    let (primary, primary_hits) = spawn_provider(404, "primary").await;
    let (search, search_hits) = spawn_provider(200, "search").await;

    create_search_connection(&app, "chatty", &primary, None).await;
    alias_connection(&app, "chatty", "chatty").await;
    create_search_connection(&app, "exa-like", &search, Some(1)).await;

    let (status, body) = json_request(
        &app,
        "POST",
        "/v1/search",
        json!({ "model": "chatty", "query": "rust" }),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "404 must surface: {body}");
    assert_eq!(primary_hits.load(Ordering::SeqCst), 1);
    assert_eq!(search_hits.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn lower_tiers_are_tried_before_higher_ones() {
    let (app, _db) = app().await;
    let (tier_two, two_hits) = spawn_provider(200, "tier-two").await;
    let (tier_one, one_hits) = spawn_provider(200, "tier-one").await;

    create_search_connection(&app, "later", &tier_two, Some(2)).await;
    create_search_connection(&app, "sooner", &tier_one, Some(1)).await;

    let (status, body) = json_request(&app, "POST", "/v1/search", json!({ "query": "rust" })).await;

    assert_eq!(status, StatusCode::OK, "search should answer: {body}");
    assert_eq!(body["from"], "tier-one");
    assert_eq!(one_hits.load(Ordering::SeqCst), 1);
    assert_eq!(two_hits.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn members_of_one_tier_rotate_round_robin() {
    let (app, _db) = app().await;
    let (first, first_hits) = spawn_provider(200, "first").await;
    let (second, second_hits) = spawn_provider(200, "second").await;

    create_search_connection(&app, "alpha", &first, Some(1)).await;
    create_search_connection(&app, "beta", &second, Some(1)).await;

    for _ in 0..4 {
        let (status, body) =
            json_request(&app, "POST", "/v1/search", json!({ "query": "rust" })).await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    let a = first_hits.load(Ordering::SeqCst);
    let b = second_hits.load(Ordering::SeqCst);
    assert_eq!((a, b), (2, 2), "four requests should split evenly");
}

/// A harness may echo its chat model into a search body. The reference does not
/// resolve to any connection, but the search tiers are still a valid route, so
/// the request must reach the configured provider instead of 404ing.
#[tokio::test]
async fn an_unresolvable_model_falls_through_to_the_search_tiers() {
    let (app, _db) = app().await;
    let (search, search_hits) = spawn_provider(200, "search").await;

    create_search_connection(&app, "exa-like", &search, Some(1)).await;

    let (status, body) = json_request(
        &app,
        "POST",
        "/v1/search",
        json!({ "model": "gpt-5.6-luna", "query": "rust" }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "tiers should answer: {body}");
    assert_eq!(body["from"], "search");
    assert_eq!(search_hits.load(Ordering::SeqCst), 1);
}

/// A reference the resolver rejects as malformed is a caller bug. Answering it
/// from a different provider would hide the mistake, so it stays a 400.
#[tokio::test]
async fn a_malformed_model_reference_is_still_an_error() {
    let (app, _db) = app().await;
    let (search, search_hits) = spawn_provider(200, "search").await;

    create_search_connection(&app, "exa-like", &search, Some(1)).await;

    let (status, body) = json_request(
        &app,
        "POST",
        "/v1/search",
        json!({ "model": "combo/", "query": "rust" }),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a malformed reference must surface: {body}"
    );
    assert_eq!(search_hits.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_chat_default_or_resolved_chat_model_does_not_shadow_search() {
    let (app, _db) = app().await;
    let (chat, chat_hits) = spawn_provider(404, "chat").await;
    let (search, search_hits) = spawn_provider(200, "search").await;
    json_request(
        &app,
        "POST",
        "/api/connections",
        json!({
            "name": "chat", "base_url": chat, "provider_type": "openai-compatible"
        }),
    )
    .await;
    alias_connection(&app, "chat", "chat").await;
    let (status, settings) = json_request(
        &app,
        "PATCH",
        "/api/settings",
        json!({"default_connection":"chat"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{settings}");
    create_search_connection(&app, "tavily", &search, Some(1)).await;
    for body in [
        json!({"query":"rust"}),
        json!({"model":"chat", "query":"rust"}),
    ] {
        let (status, body) = json_request(&app, "POST", "/v1/search", body).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["from"], "search");
    }
    assert_eq!(chat_hits.load(Ordering::SeqCst), 0);
    assert_eq!(search_hits.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn exhausted_search_credits_fall_through_on_402() {
    let (app, _db) = app().await;
    let (primary, primary_hits) = spawn_provider(402, "empty").await;
    let (fallback, fallback_hits) = spawn_provider(200, "fallback").await;
    create_search_connection(&app, "empty", &primary, Some(1)).await;
    create_search_connection(&app, "fallback", &fallback, Some(2)).await;
    let (status, body) = json_request(&app, "POST", "/v1/search", json!({"query":"rust"})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["from"], "fallback");
    assert_eq!(primary_hits.load(Ordering::SeqCst), 1);
    assert_eq!(fallback_hits.load(Ordering::SeqCst), 1);
}
