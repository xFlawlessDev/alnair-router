//! Endpoint shape smoke tests: routing, auth, and error bodies.

use alnair_router::config::{RateLimitConfig, RouterConfig};
use alnair_router::db::Db;
use alnair_router::db::repos::api_keys::{ApiKeyRepository, CreateApiKey};
use alnair_router::db::repos::usage::{NewUsageRecord, UsageRepository};
use alnair_router::{AppState, build_router};
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use tower::ServiceExt;

const TEST_SECRET: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

/// Builds an app over a fresh in-memory database with a test encryption key.
async fn app_with_config(mut config: RouterConfig) -> (axum::Router, Db) {
    let db = Db::connect_in_memory().await.expect("db");
    if config.secrets.key.is_none() {
        config.secrets.key = Some(TEST_SECRET.to_string());
    }

    let state = AppState::new(config, db.clone()).expect("state");
    (build_router(state), db)
}

/// Builds an app over a fresh in-memory database.
async fn app(require_api_key: bool) -> (axum::Router, Db) {
    let mut config = RouterConfig::default();
    config.server.require_api_key = require_api_key;
    app_with_config(config).await
}

/// Builds an app whose admin routes are guarded by a bearer token.
async fn app_with_admin_token(token: &str) -> (axum::Router, Db) {
    let mut config = RouterConfig::default();
    config.server.admin_token = Some(token.to_string());
    app_with_config(config).await
}

/// Mints a router-issued client key and returns its plaintext secret.
async fn mint_key(db: &Db, name: &str) -> String {
    ApiKeyRepository::new(db.pool.clone())
        .create(CreateApiKey {
            name: name.to_string(),
            enabled: true,
            rate_limit_per_minute: None,
            monthly_budget_usd: None,
            budget_mode: None,
        })
        .await
        .expect("mint key")
        .secret
}

/// Records a usage row, used to simulate spend for budget checks.
async fn record_usage(db: &Db, api_key_id: &str, cost_usd: f64) {
    UsageRepository::new(db.pool.clone())
        .record(NewUsageRecord {
            api_key_id: Some(api_key_id.to_string()),
            requested_model: "metered".to_string(),
            resolved_provider: None,
            resolved_model: None,
            attempt: 1,
            status: "ok".to_string(),
            prompt_tokens: 1,
            completion_tokens: 1,
            cached_tokens: 0,
            cost_usd,
            latency_ms: 5,
        })
        .await
        .expect("record usage");
}

async fn get(app: &axum::Router, path: &str) -> (StatusCode, serde_json::Value) {
    get_with_auth(app, path, None).await
}

async fn get_with_auth(
    app: &axum::Router,
    path: &str,
    bearer: Option<&str>,
) -> (StatusCode, serde_json::Value) {
    let response = raw_request_with_auth(app, "GET", path, None, bearer).await;
    (response.status(), body_json(response).await)
}

async fn raw_request_with_auth(
    app: &axum::Router,
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
    bearer: Option<&str>,
) -> axum::response::Response {
    let mut builder = Request::builder().method(method).uri(path);

    let body = match body {
        Some(body) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(body.to_string())
        }
        None => Body::empty(),
    };

    if let Some(token) = bearer {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }

    app.clone()
        .oneshot(builder.body(body).unwrap())
        .await
        .expect("request")
}

async fn json_request(
    app: &axum::Router,
    method: &str,
    path: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    json_request_with_auth(app, method, path, body, None).await
}

async fn json_request_with_auth(
    app: &axum::Router,
    method: &str,
    path: &str,
    body: serde_json::Value,
    bearer: Option<&str>,
) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json");

    if let Some(token) = bearer {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }

    let response = app
        .clone()
        .oneshot(builder.body(Body::from(body.to_string())).unwrap())
        .await
        .expect("request");

    (response.status(), body_json(response).await)
}

async fn body_json(response: axum::response::Response) -> serde_json::Value {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    if bytes.is_empty() {
        return serde_json::Value::Null;
    }
    serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
}

#[tokio::test]
async fn health_reports_ok() {
    let (app, _db) = app(false).await;
    let (status, body) = get(&app, "/api/health").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
    assert_eq!(body["service"], "alnair-router");
}

#[tokio::test]
async fn models_list_is_empty_on_a_fresh_database() {
    let (app, _db) = app(false).await;
    let (status, body) = get(&app, "/v1/models").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["object"], "list");
    assert!(body["data"].as_array().expect("array").is_empty());
}

#[tokio::test]
async fn chat_completion_with_an_unknown_model_is_a_404() {
    let (app, _db) = app(false).await;
    let (status, body) = json_request(
        &app,
        "POST",
        "/v1/chat/completions",
        serde_json::json!({
            "model": "nope/nothing",
            "messages": [{ "role": "user", "content": "hi" }]
        }),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["type"], "not_found_error");
}

#[tokio::test]
async fn chat_completion_without_messages_is_a_400() {
    let (app, _db) = app(false).await;
    let (status, body) = json_request(
        &app,
        "POST",
        "/v1/chat/completions",
        serde_json::json!({ "model": "anything", "messages": [] }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["type"], "invalid_request_error");
}

#[tokio::test]
async fn connection_with_ollama_is_rejected() {
    let (app, _db) = app(false).await;
    let (status, body) = json_request(
        &app,
        "POST",
        "/api/connections",
        serde_json::json!({
            "name": "local-ollama",
            "provider_type": "ollama",
            "base_url": "http://127.0.0.1:11434/v1"
        }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("unsupported provider type"),
        "message should name the unsupported type: {body}"
    );
}

#[tokio::test]
async fn connections_round_trip_through_the_admin_api() {
    let (app, _db) = app(false).await;

    let (status, created) = json_request(
        &app,
        "POST",
        "/api/connections",
        serde_json::json!({
            "name": "openai-main",
            "provider_type": "openai-compatible",
            "base_url": "https://api.openai.com/v1",
            "api_key": "sk-test"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["name"], "openai-main");

    let (status, body) = get(&app, "/api/connections").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().expect("array").len(), 1);
}

#[tokio::test]
async fn alias_requires_an_existing_connection() {
    let (app, _db) = app(false).await;
    let (status, body) = json_request(
        &app,
        "POST",
        "/api/aliases",
        serde_json::json!({
            "prefix": "gh",
            "connection_id": "does-not-exist"
        }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("does not exist")
    );
}

#[tokio::test]
async fn aliases_and_combos_appear_in_the_model_list() {
    let (app, _db) = app(false).await;

    let (_, connection) = json_request(
        &app,
        "POST",
        "/api/connections",
        serde_json::json!({
            "name": "glm-main",
            "provider_type": "openai-compatible",
            "base_url": "https://example.invalid/v1"
        }),
    )
    .await;

    let connection_id = connection["id"].as_str().expect("connection id");

    let (status, _) = json_request(
        &app,
        "POST",
        "/api/aliases",
        serde_json::json!({ "prefix": "glm", "connection_id": connection_id }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = json_request(
        &app,
        "POST",
        "/api/combos",
        serde_json::json!({
            "name": "free-forever",
            "entries": ["glm/glm-4.6"]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (_, body) = get(&app, "/v1/models").await;
    let ids: Vec<String> = body["data"]
        .as_array()
        .expect("array")
        .iter()
        .filter_map(|entry| entry["id"].as_str().map(str::to_string))
        .collect();

    assert!(ids.contains(&"glm".to_string()), "alias listed: {ids:?}");
    assert!(
        ids.contains(&"free-forever".to_string()),
        "combo listed: {ids:?}"
    );
}

#[tokio::test]
async fn combos_report_their_resolved_tiers() {
    let (app, _db) = app(false).await;

    let (_, connection) = json_request(
        &app,
        "POST",
        "/api/connections",
        serde_json::json!({
            "name": "glm-main",
            "provider_type": "openai-compatible",
            "base_url": "https://example.invalid/v1"
        }),
    )
    .await;
    let connection_id = connection["id"].as_str().expect("connection id");

    json_request(
        &app,
        "POST",
        "/api/aliases",
        serde_json::json!({ "prefix": "glm", "connection_id": connection_id }),
    )
    .await;

    json_request(
        &app,
        "POST",
        "/api/combos",
        serde_json::json!({
            "name": "free-forever",
            "entries": ["glm/glm-4.6", "glm/glm-4.5"]
        }),
    )
    .await;

    let (status, body) = get(&app, "/v1/models/info").await;
    assert_eq!(status, StatusCode::OK);

    let combo = body["data"]
        .as_array()
        .expect("array")
        .iter()
        .find(|entry| entry["id"] == "free-forever")
        .expect("combo present");

    let tiers: Vec<String> = combo["tiers"]
        .as_array()
        .expect("tiers")
        .iter()
        .filter_map(|tier| tier.as_str().map(str::to_string))
        .collect();

    assert_eq!(tiers, vec!["glm-4.6", "glm-4.5"]);
}

#[tokio::test]
async fn api_key_auth_is_enforced_when_required() {
    let (app, db) = app(true).await;

    // Without a key.
    let (status, body) = json_request(
        &app,
        "POST",
        "/v1/chat/completions",
        serde_json::json!({ "model": "x", "messages": [] }),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["type"], "authentication_error");

    // With a valid key, the request proceeds past auth (and then fails on the
    // unknown model, proving auth passed).
    let created = alnair_router::db::repos::api_keys::ApiKeyRepository::new(db.pool.clone())
        .create(CreateApiKey {
            name: "test".to_string(),
            enabled: true,
            rate_limit_per_minute: None,
            monthly_budget_usd: None,
            budget_mode: None,
        })
        .await
        .expect("mint key");

    let (status, body) = json_request_with_auth(
        &app,
        "POST",
        "/v1/chat/completions",
        serde_json::json!({ "model": "nope/nothing", "messages": [] }),
        Some(&created.secret),
    )
    .await;

    assert_ne!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["type"], "invalid_request_error");
}

#[tokio::test]
async fn invalid_api_key_is_rejected() {
    let (app, _db) = app(true).await;

    let (status, _) = json_request_with_auth(
        &app,
        "POST",
        "/v1/chat/completions",
        serde_json::json!({ "model": "x", "messages": [] }),
        Some("sk-router-not-a-real-key"),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn web_fetch_refuses_private_addresses() {
    let (app, _db) = app(false).await;
    let (status, body) = json_request(
        &app,
        "POST",
        "/v1/web/fetch",
        serde_json::json!({ "url": "http://169.254.169.254/latest/meta-data/" }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("private"),
        "should refuse a metadata address: {body}"
    );
}

#[tokio::test]
async fn web_fetch_refuses_non_http_schemes() {
    let (app, _db) = app(false).await;
    let (status, body) = json_request(
        &app,
        "POST",
        "/v1/web/fetch",
        serde_json::json!({ "url": "file:///etc/passwd" }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("unsupported scheme"),
        "should reject non-http schemes: {body}"
    );
}

#[tokio::test]
async fn admin_routes_require_the_token_when_configured() {
    let (app, _db) = app_with_admin_token("admin-secret").await;

    let (status, body) = get(&app, "/api/connections").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["type"], "authentication_error");

    let (status, _) = get_with_auth(&app, "/api/connections", Some("wrong")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, body) = get_with_auth(&app, "/api/connections", Some("admin-secret")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.as_array().expect("array").is_empty());
}

#[tokio::test]
async fn admin_routes_are_open_when_no_token_is_configured() {
    let (app, _db) = app(false).await;

    let (status, _) = get(&app, "/api/connections").await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn usage_summary_starts_at_zero() {
    let (app, _db) = app(false).await;
    let (status, body) = get(&app, "/api/usage/summary").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["requests"], 0);
    assert_eq!(body["cost_usd"], 0.0);
}

#[tokio::test]
async fn count_tokens_estimates_input() {
    let (app, _db) = app(false).await;
    let (status, body) = json_request(
        &app,
        "POST",
        "/v1/messages/count_tokens",
        serde_json::json!({
            "model": "anything",
            "messages": [{ "role": "user", "content": "hello there" }]
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        body["input_tokens"].as_u64().unwrap_or(0) > 0,
        "estimate should be positive: {body}"
    );
}

#[tokio::test]
async fn anthropic_messages_rejects_an_unknown_model() {
    let (app, _db) = app(false).await;
    let (status, body) = json_request(
        &app,
        "POST",
        "/v1/messages",
        serde_json::json!({
            "model": "nope/nothing",
            "max_tokens": 64,
            "messages": [{ "role": "user", "content": "hi" }]
        }),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["type"], "not_found_error");
}

#[tokio::test]
async fn responses_endpoint_rejects_an_unknown_model() {
    let (app, _db) = app(false).await;
    let (status, _) = json_request(
        &app,
        "POST",
        "/v1/responses",
        serde_json::json!({ "model": "nope/nothing", "input": "hi" }),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Guards the vendored-provider seam: only `upstream/chat_backend.rs` may name
/// `crate::llm`, so the provider layer stays swappable.
#[test]
fn vendored_llm_layer_is_imported_from_exactly_one_file() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();
    visit(&src, &mut offenders);

    assert!(
        offenders.is_empty(),
        "only upstream/chat_backend.rs may reach into the vendored llm layer, \
         but found: {offenders:?}"
    );
}

fn visit(dir: &std::path::Path, offenders: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).expect("readable dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            // The vendored provider layer is allowed to reference itself.
            if path.ends_with("src/llm") {
                continue;
            }
            visit(&path, offenders);
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        if path.ends_with("upstream/chat_backend.rs") {
            continue;
        }

        let contents = std::fs::read_to_string(&path).expect("readable file");
        if contents.contains("crate::llm") || contents.contains("alnair_router::llm") {
            offenders.push(path.display().to_string());
        }
    }
}

#[tokio::test]
async fn per_key_rate_limit_returns_429_with_retry_after() {
    let mut config = RouterConfig::default();
    config.server.require_api_key = true;
    config.rate_limit = RateLimitConfig {
        requests_per_minute: 1,
        burst: 1,
    };
    let (app, db) = app_with_config(config).await;
    let secret = mint_key(&db, "metered").await;

    let first = raw_request_with_auth(
        &app,
        "POST",
        "/v1/chat/completions",
        Some(serde_json::json!({ "model": "nope/nothing", "messages": [] })),
        Some(&secret),
    )
    .await;
    assert_ne!(first.status(), StatusCode::UNAUTHORIZED);

    let second = raw_request_with_auth(
        &app,
        "POST",
        "/v1/chat/completions",
        Some(serde_json::json!({ "model": "nope/nothing", "messages": [] })),
        Some(&secret),
    )
    .await;

    assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(
        second.headers().contains_key(header::RETRY_AFTER),
        "429 should include Retry-After"
    );
}

#[tokio::test]
async fn budget_block_mode_returns_402() {
    let mut config = RouterConfig::default();
    config.server.require_api_key = true;
    let (app, db) = app_with_config(config).await;

    let created = ApiKeyRepository::new(db.pool.clone())
        .create(CreateApiKey {
            name: "budgeted".to_string(),
            enabled: true,
            rate_limit_per_minute: None,
            monthly_budget_usd: Some(0.001),
            budget_mode: Some("block".to_string()),
        })
        .await
        .expect("key");
    record_usage(&db, &created.key.id, 1.0).await;

    let (status, body) = json_request_with_auth(
        &app,
        "POST",
        "/v1/chat/completions",
        serde_json::json!({ "model": "nope/nothing", "messages": [] }),
        Some(&created.secret),
    )
    .await;

    assert_eq!(status, StatusCode::PAYMENT_REQUIRED);
    assert_eq!(body["error"]["type"], "insufficient_quota");
}

#[tokio::test]
async fn budget_warn_mode_passes_with_a_warning_header() {
    let mut config = RouterConfig::default();
    config.server.require_api_key = true;
    let (app, db) = app_with_config(config).await;

    let created = ApiKeyRepository::new(db.pool.clone())
        .create(CreateApiKey {
            name: "warned".to_string(),
            enabled: true,
            rate_limit_per_minute: None,
            monthly_budget_usd: Some(0.001),
            budget_mode: Some("warn".to_string()),
        })
        .await
        .expect("key");
    record_usage(&db, &created.key.id, 1.0).await;

    let response = raw_request_with_auth(
        &app,
        "POST",
        "/v1/chat/completions",
        Some(serde_json::json!({ "model": "nope/nothing", "messages": [] })),
        Some(&created.secret),
    )
    .await;

    assert_ne!(response.status(), StatusCode::PAYMENT_REQUIRED);
    assert!(
        response.headers().contains_key("x-router-budget-warning"),
        "warn mode should set the budget header"
    );
}

#[tokio::test]
async fn key_patch_updates_limits_and_enabled_state() {
    let (app, db) = app(false).await;
    let created = ApiKeyRepository::new(db.pool.clone())
        .create(CreateApiKey {
            name: "editable".to_string(),
            enabled: true,
            rate_limit_per_minute: None,
            monthly_budget_usd: None,
            budget_mode: None,
        })
        .await
        .expect("key");

    let (status, body) = json_request(
        &app,
        "PATCH",
        &format!("/api/keys/{}", created.key.id),
        serde_json::json!({
            "enabled": false,
            "rate_limit_per_minute": 10,
            "monthly_budget_usd": 2.5,
            "budget_mode": "block"
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["enabled"], 0);
    assert_eq!(body["rate_limit_per_minute"], 10);
    assert_eq!(body["monthly_budget_usd"], 2.5);
    assert_eq!(body["budget_mode"], "block");
}

#[tokio::test]
async fn connection_patch_clears_api_key_when_sent_null() {
    let (app, _db) = app(false).await;

    let (_, connection) = json_request(
        &app,
        "POST",
        "/api/connections",
        serde_json::json!({
            "name": "clearable",
            "provider_type": "openai-compatible",
            "base_url": "https://example.invalid/v1",
            "api_key": "sk-clear-me"
        }),
    )
    .await;
    let id = connection["id"].as_str().expect("connection id");

    let (status, body) = json_request(
        &app,
        "PATCH",
        &format!("/api/connections/{id}"),
        serde_json::json!({ "api_key": null }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        body["api_key"].is_null(),
        "null should clear the stored key: {body}"
    );
}

#[tokio::test]
async fn admin_writes_invalidate_the_routing_cache() {
    let (app, _db) = app(false).await;

    let (_, connection) = json_request(
        &app,
        "POST",
        "/api/connections",
        serde_json::json!({
            "name": "cached",
            "provider_type": "openai-compatible",
            "base_url": "https://example.invalid/v1"
        }),
    )
    .await;
    let connection_id = connection["id"].as_str().expect("connection id");

    json_request(
        &app,
        "POST",
        "/api/aliases",
        serde_json::json!({ "prefix": "one", "connection_id": connection_id }),
    )
    .await;

    let model_ids = |body: &serde_json::Value| -> Vec<String> {
        body["data"]
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|entry| entry["id"].as_str().map(str::to_string))
            .collect()
    };

    let (_, first) = get(&app, "/v1/models").await;
    assert!(model_ids(&first).contains(&"one".to_string()));

    // A second admin write must be visible immediately, not after the TTL.
    json_request(
        &app,
        "POST",
        "/api/aliases",
        serde_json::json!({ "prefix": "two", "connection_id": connection_id }),
    )
    .await;

    let (_, second) = get(&app, "/v1/models").await;
    let ids = model_ids(&second);
    assert!(
        ids.contains(&"one".to_string()) && ids.contains(&"two".to_string()),
        "admin writes should invalidate the cached catalog: {ids:?}"
    );
}

#[tokio::test]
async fn probes_are_public_and_report_readiness() {
    let (app, _db) = app_with_admin_token("admin-secret").await;

    // No token needed for liveness/readiness even when admin auth is on.
    let (status, body) = get(&app, "/api/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");

    let (status, body) = get(&app, "/api/ready").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["database"], "ok");
}

#[tokio::test]
async fn readiness_reports_unreachable_upstreams_when_enabled() {
    let mut config = RouterConfig::default();
    config.server.readiness_upstream_checks = true;
    let (app, _db) = app_with_config(config).await;

    json_request(
        &app,
        "POST",
        "/api/connections",
        serde_json::json!({
            "name": "dead-endpoint",
            "provider_type": "openai-compatible",
            "base_url": "http://127.0.0.1:1/v1"
        }),
    )
    .await;

    let (status, body) = get(&app, "/api/ready").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["upstreams"]["unreachable"], 1);
    assert_eq!(body["upstreams"]["reachable"], 0);
}

#[tokio::test]
async fn metrics_endpoint_exposes_prometheus_text() {
    let (app, _db) = app(false).await;

    let response = raw_request_with_auth(&app, "GET", "/api/metrics", None, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .starts_with("text/plain"),
        "metrics should be text/plain"
    );

    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("alnair_router_requests_total"));
    assert!(text.contains("# TYPE alnair_router_attempts_total counter"));
}
