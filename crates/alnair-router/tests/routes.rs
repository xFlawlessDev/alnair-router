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

/// Builds a file-backed app: `VACUUM INTO` snapshots need a real database.
async fn file_app() -> (axum::Router, Db, tempfile::TempDir) {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut config = RouterConfig::default();
    config.secrets.key = Some(TEST_SECRET.to_string());
    config.server.require_api_key = true;
    config.storage.url = format!(
        "sqlite://{}?mode=rwc",
        directory.path().join("router.sqlite").display()
    );

    let db = Db::connect(&config).await.expect("db");
    db.migrate().await.expect("migrate");
    let state = AppState::new(config, db.clone()).expect("state");
    (build_router(state), db, directory)
}

/// Mints a router-issued client key and returns its plaintext secret.
async fn mint_key(db: &Db, name: &str) -> String {
    ApiKeyRepository::new(db.pool.clone())
        .create(CreateApiKey {
            name: name.to_string(),
            enabled: true,
            rate_limit_per_minute: None,
            daily_budget_usd: None,
            weekly_budget_usd: None,
            monthly_budget_usd: None,
            lifetime_budget_usd: None,
            daily_token_limit: None,
            weekly_token_limit: None,
            monthly_token_limit: None,
            lifetime_token_limit: None,
            budget_mode: None,
            plan_id: None,
            allowed_models: None,
            expires_at: None,
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
            connection_name: None,
            attempt: 1,
            status: "ok".to_string(),
            prompt_tokens: 1,
            completion_tokens: 1,
            cached_tokens: 0,
            reasoning_tokens: 0,
            cost_usd,
            cost_input_usd: 0.0,
            cost_output_usd: cost_usd,
            cost_reasoning_usd: 0.0,
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

/// POSTs a raw binary body, for the backup restore endpoint.
async fn post_bytes(
    app: &axum::Router,
    path: &str,
    bytes: Vec<u8>,
) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header(header::CONTENT_TYPE, "application/octet-stream")
                .body(Body::from(bytes))
                .unwrap(),
        )
        .await
        .expect("request");
    (response.status(), body_json(response).await)
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
            daily_budget_usd: None,
            weekly_budget_usd: None,
            monthly_budget_usd: None,
            lifetime_budget_usd: None,
            daily_token_limit: None,
            weekly_token_limit: None,
            monthly_token_limit: None,
            lifetime_token_limit: None,
            budget_mode: None,
            plan_id: None,
            allowed_models: None,
            expires_at: None,
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

/// Guards the provider seam: only `upstream/chat_backend.rs` may reference the
/// `alnair_llm` crate, so the provider layer stays swappable.
#[test]
fn vendored_llm_layer_is_imported_from_exactly_one_file() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();
    visit(&src, &mut offenders);

    assert!(
        offenders.is_empty(),
        "only upstream/chat_backend.rs may reach into the alnair_llm crate, \
         but found: {offenders:?}"
    );
}

fn visit(dir: &std::path::Path, offenders: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).expect("readable dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
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
        // Doc/comment mentions are fine; the guard is about real dependencies.
        let code = contents
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<String>();
        if code.contains("alnair_llm") || code.contains("alnair-llm") {
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
            daily_budget_usd: None,
            weekly_budget_usd: None,
            monthly_budget_usd: Some(0.001),
            lifetime_budget_usd: None,
            daily_token_limit: None,
            weekly_token_limit: None,
            monthly_token_limit: None,
            lifetime_token_limit: None,
            budget_mode: Some("block".to_string()),
            plan_id: None,
            allowed_models: None,
            expires_at: None,
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
            daily_budget_usd: None,
            weekly_budget_usd: None,
            monthly_budget_usd: Some(0.001),
            lifetime_budget_usd: None,
            daily_token_limit: None,
            weekly_token_limit: None,
            monthly_token_limit: None,
            lifetime_token_limit: None,
            budget_mode: Some("warn".to_string()),
            plan_id: None,
            allowed_models: None,
            expires_at: None,
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
async fn daily_budget_block_mode_returns_402() {
    let mut config = RouterConfig::default();
    config.server.require_api_key = true;
    let (app, db) = app_with_config(config).await;

    let created = ApiKeyRepository::new(db.pool.clone())
        .create(CreateApiKey {
            name: "daily-capped".to_string(),
            enabled: true,
            rate_limit_per_minute: None,
            daily_budget_usd: Some(0.001),
            weekly_budget_usd: None,
            monthly_budget_usd: None,
            lifetime_budget_usd: None,
            daily_token_limit: None,
            weekly_token_limit: None,
            monthly_token_limit: None,
            lifetime_token_limit: None,
            budget_mode: Some("block".to_string()),
            plan_id: None,
            allowed_models: None,
            expires_at: None,
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
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("daily budget"),
        "the exhausted window should be named: {body}"
    );
}

#[tokio::test]
async fn lifetime_budget_block_mode_returns_402() {
    let mut config = RouterConfig::default();
    config.server.require_api_key = true;
    let (app, db) = app_with_config(config).await;

    let created = ApiKeyRepository::new(db.pool.clone())
        .create(CreateApiKey {
            name: "lifetime-capped".to_string(),
            enabled: true,
            rate_limit_per_minute: None,
            daily_budget_usd: None,
            weekly_budget_usd: None,
            monthly_budget_usd: None,
            lifetime_budget_usd: Some(0.001),
            daily_token_limit: None,
            weekly_token_limit: None,
            monthly_token_limit: None,
            lifetime_token_limit: None,
            budget_mode: Some("block".to_string()),
            plan_id: None,
            allowed_models: None,
            expires_at: None,
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
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("lifetime budget"),
        "the exhausted window should be named: {body}"
    );
}

#[tokio::test]
async fn token_limit_block_mode_returns_402() {
    let mut config = RouterConfig::default();
    config.server.require_api_key = true;
    let (app, db) = app_with_config(config).await;

    let created = ApiKeyRepository::new(db.pool.clone())
        .create(CreateApiKey {
            name: "token-capped".to_string(),
            enabled: true,
            rate_limit_per_minute: None,
            daily_budget_usd: None,
            weekly_budget_usd: None,
            monthly_budget_usd: None,
            lifetime_budget_usd: None,
            daily_token_limit: Some(1),
            weekly_token_limit: None,
            monthly_token_limit: None,
            lifetime_token_limit: None,
            budget_mode: Some("block".to_string()),
            plan_id: None,
            allowed_models: None,
            expires_at: None,
        })
        .await
        .expect("key");
    // The recorded row counts 1 prompt + 1 completion token.
    record_usage(&db, &created.key.id, 0.0).await;

    let (status, body) = json_request_with_auth(
        &app,
        "POST",
        "/v1/chat/completions",
        serde_json::json!({ "model": "nope/nothing", "messages": [] }),
        Some(&created.secret),
    )
    .await;

    assert_eq!(status, StatusCode::PAYMENT_REQUIRED);
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("daily token limit"),
        "the exhausted token window should be named: {body}"
    );
}

#[tokio::test]
async fn key_spend_endpoint_reports_window_totals() {
    let (app, db) = app(false).await;

    let created = ApiKeyRepository::new(db.pool.clone())
        .create(CreateApiKey {
            name: "monitored".to_string(),
            enabled: true,
            rate_limit_per_minute: None,
            daily_budget_usd: None,
            weekly_budget_usd: None,
            monthly_budget_usd: None,
            lifetime_budget_usd: None,
            daily_token_limit: None,
            weekly_token_limit: None,
            monthly_token_limit: None,
            lifetime_token_limit: None,
            budget_mode: None,
            plan_id: None,
            allowed_models: None,
            expires_at: None,
        })
        .await
        .expect("key");
    record_usage(&db, &created.key.id, 1.25).await;

    let (status, body) = get(&app, "/api/usage/keys").await;
    assert_eq!(status, StatusCode::OK, "unexpected body: {body}");

    let rows = body.as_array().expect("array");
    assert_eq!(
        rows.len(),
        1,
        "only keys with usage rows are reported: {body}"
    );
    assert_eq!(rows[0]["api_key_id"], created.key.id);
    assert_eq!(rows[0]["daily_usd"], 1.25);
    assert_eq!(rows[0]["weekly_usd"], 1.25);
    assert_eq!(rows[0]["monthly_usd"], 1.25);
    assert_eq!(rows[0]["lifetime_usd"], 1.25);
    assert_eq!(rows[0]["daily_tokens"], 2);
    assert_eq!(rows[0]["lifetime_tokens"], 2);
}

#[tokio::test]
async fn expired_key_is_rejected_with_401() {
    let mut config = RouterConfig::default();
    config.server.require_api_key = true;
    let (app, db) = app_with_config(config).await;

    let created = ApiKeyRepository::new(db.pool.clone())
        .create(CreateApiKey {
            name: "stale".to_string(),
            enabled: true,
            rate_limit_per_minute: None,
            daily_budget_usd: None,
            weekly_budget_usd: None,
            monthly_budget_usd: None,
            lifetime_budget_usd: None,
            daily_token_limit: None,
            weekly_token_limit: None,
            monthly_token_limit: None,
            lifetime_token_limit: None,
            budget_mode: None,
            plan_id: None,
            allowed_models: None,
            expires_at: Some(chrono::Utc::now() - chrono::Duration::minutes(1)),
        })
        .await
        .expect("key");

    let (status, body) = json_request_with_auth(
        &app,
        "POST",
        "/v1/chat/completions",
        serde_json::json!({ "model": "nope/nothing", "messages": [] }),
        Some(&created.secret),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["type"], "authentication_error");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("expired"),
        "the rejection should name the expiry: {body}"
    );
}

#[tokio::test]
async fn expired_plan_fails_closed_for_attached_keys() {
    let mut config = RouterConfig::default();
    config.server.require_api_key = true;
    let (app, _db) = app_with_config(config).await;

    let (status, plan) = json_request(
        &app,
        "POST",
        "/api/plans",
        serde_json::json!({
            "name": "trial",
            "expires_at": "2020-01-01T00:00:00Z"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "unexpected body: {plan}");

    let (status, created) = json_request(
        &app,
        "POST",
        "/api/keys",
        serde_json::json!({ "name": "trial-key", "plan_id": plan["id"] }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "unexpected body: {created}");
    let secret = created["secret"].as_str().expect("secret").to_string();

    let (status, body) = json_request_with_auth(
        &app,
        "POST",
        "/v1/chat/completions",
        serde_json::json!({ "model": "nope/nothing", "messages": [] }),
        Some(&secret),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["type"], "permission_error");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("plan"),
        "the rejection should name the plan: {body}"
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
            daily_budget_usd: None,
            weekly_budget_usd: None,
            monthly_budget_usd: None,
            lifetime_budget_usd: None,
            daily_token_limit: None,
            weekly_token_limit: None,
            monthly_token_limit: None,
            lifetime_token_limit: None,
            budget_mode: None,
            plan_id: None,
            allowed_models: None,
            expires_at: None,
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
            "daily_budget_usd": 1.0,
            "weekly_budget_usd": 5.0,
            "monthly_budget_usd": 2.5,
            "lifetime_budget_usd": 50.0,
            "daily_token_limit": 100000,
            "lifetime_token_limit": 10000000,
            "budget_mode": "block",
            "expires_at": "2030-01-01T00:00:00Z"
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["enabled"], 0);
    assert_eq!(body["rate_limit_per_minute"], 10);
    assert_eq!(body["daily_budget_usd"], 1.0);
    assert_eq!(body["weekly_budget_usd"], 5.0);
    assert_eq!(body["monthly_budget_usd"], 2.5);
    assert_eq!(body["lifetime_budget_usd"], 50.0);
    assert_eq!(body["daily_token_limit"], 100000);
    assert_eq!(body["lifetime_token_limit"], 10000000);
    assert_eq!(body["budget_mode"], "block");
    assert_eq!(body["expires_at"], "2030-01-01T00:00:00Z");

    // `null` clears a single window and the expiry without touching the rest.
    let (status, body) = json_request(
        &app,
        "PATCH",
        &format!("/api/keys/{}", created.key.id),
        serde_json::json!({
            "daily_budget_usd": null,
            "lifetime_budget_usd": null,
            "daily_token_limit": null,
            "expires_at": null,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["daily_budget_usd"].is_null());
    assert!(body["lifetime_budget_usd"].is_null());
    assert!(body["daily_token_limit"].is_null());
    assert!(body["expires_at"].is_null());
    assert_eq!(body["weekly_budget_usd"], 5.0);
    assert_eq!(body["lifetime_token_limit"], 10000000);
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

async fn raw_text(app: &axum::Router, path: &str) -> (StatusCode, String, Option<String>) {
    let response = raw_request_with_auth(app, "GET", path, None, None).await;
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

#[tokio::test]
async fn dashboard_is_served_from_the_binary() {
    let (app, _db) = app(false).await;

    let (status, body, content_type) = raw_text(&app, "/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        content_type
            .as_deref()
            .unwrap_or_default()
            .starts_with("text/html"),
        "index should be html: {content_type:?}"
    );
    assert!(
        body.contains("<html") || body.contains("<!doctype"),
        "unexpected body: {body}"
    );

    // SPA route without a file extension falls back to the shell.
    let (status, body, _) = raw_text(&app, "/connections").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("<html") || body.contains("<!doctype"));

    // A missing file is a real 404, not the shell.
    let (status, _, _) = raw_text(&app, "/missing-asset.js").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Spawns a minimal upstream that serves an OpenAI-shaped models list.
async fn spawn_models_upstream(models: Vec<&'static str>) -> String {
    let ids: Vec<String> = models.into_iter().map(str::to_string).collect();
    let router = axum::Router::new().route(
        "/v1/models",
        axum::routing::get(move || {
            let ids = ids.clone();
            async move {
                let data: Vec<serde_json::Value> = ids
                    .iter()
                    .map(|id| serde_json::json!({ "id": id, "object": "model" }))
                    .collect();
                axum::Json(serde_json::json!({ "object": "list", "data": data }))
            }
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

async fn create_probe_connection(app: &axum::Router, base_url: &str) -> String {
    let (status, connection) = json_request(
        app,
        "POST",
        "/api/connections",
        serde_json::json!({
            "name": format!("probe-{}", uuid_like()),
            "provider_type": "openai-compatible",
            "base_url": base_url,
            "api_key": "sk-probe"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    connection["id"]
        .as_str()
        .expect("connection id")
        .to_string()
}

fn uuid_like() -> String {
    format!(
        "{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    )
}

#[tokio::test]
async fn connection_models_probe_lists_upstream_models() {
    let (app, _db) = app(false).await;
    let base_url = spawn_models_upstream(vec!["gpt-4o", "gpt-4o-mini"]).await;
    let id = create_probe_connection(&app, &base_url).await;

    let (status, body) = get(&app, &format!("/api/connections/{id}/models")).await;

    assert_eq!(status, StatusCode::OK);
    let ids: Vec<&str> = body["models"]
        .as_array()
        .expect("models")
        .iter()
        .filter_map(|model| model["id"].as_str())
        .collect();
    assert_eq!(ids, vec!["gpt-4o", "gpt-4o-mini"]);
}

#[tokio::test]
async fn connection_test_reports_a_reachable_upstream() {
    let (app, _db) = app(false).await;
    let base_url = spawn_models_upstream(vec!["gpt-4o"]).await;
    let id = create_probe_connection(&app, &base_url).await;

    let (status, body) = json_request(
        &app,
        "POST",
        &format!("/api/connections/{id}/test"),
        serde_json::json!({}),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);
    assert_eq!(body["models_count"], 1);
    assert!(body["latency_ms"].is_u64());
}

#[tokio::test]
async fn connection_test_reports_an_unreachable_upstream() {
    let (app, _db) = app(false).await;
    let id = create_probe_connection(&app, "http://127.0.0.1:1/v1").await;

    let (status, body) = json_request(
        &app,
        "POST",
        &format!("/api/connections/{id}/test"),
        serde_json::json!({}),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_GATEWAY);
    let message = body["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("failed") && message.contains("127.0.0.1:1"),
        "unexpected body: {body}"
    );
}

#[tokio::test]
async fn alias_test_checks_the_override_against_upstream_models() {
    let (app, _db) = app(false).await;
    let base_url = spawn_models_upstream(vec!["gpt-4o", "gpt-4o-mini"]).await;
    let connection_id = create_probe_connection(&app, &base_url).await;

    let (status, alias) = json_request(
        &app,
        "POST",
        "/api/aliases",
        serde_json::json!({
            "prefix": "probe-ok",
            "connection_id": connection_id,
            "model_override": "gpt-4o"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let alias_id = alias["id"].as_str().expect("alias id");

    let (status, body) = json_request(
        &app,
        "POST",
        &format!("/api/aliases/{alias_id}/test"),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);
    assert_eq!(body["model_available"], true);

    let (_, missing) = json_request(
        &app,
        "POST",
        "/api/aliases",
        serde_json::json!({
            "prefix": "probe-missing",
            "connection_id": connection_id,
            "model_override": "not-a-real-model"
        }),
    )
    .await;
    let missing_id = missing["id"].as_str().expect("alias id");

    let (status, body) = json_request(
        &app,
        "POST",
        &format!("/api/aliases/{missing_id}/test"),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], false);
    assert_eq!(body["model_available"], false);
}

#[tokio::test]
async fn alias_test_reports_a_disabled_alias() {
    let (app, _db) = app(false).await;
    let base_url = spawn_models_upstream(vec!["gpt-4o"]).await;
    let connection_id = create_probe_connection(&app, &base_url).await;

    let (_, alias) = json_request(
        &app,
        "POST",
        "/api/aliases",
        serde_json::json!({
            "prefix": "probe-off",
            "connection_id": connection_id,
            "enabled": false
        }),
    )
    .await;
    let alias_id = alias["id"].as_str().expect("alias id");

    let (status, body) = json_request(
        &app,
        "POST",
        &format!("/api/aliases/{alias_id}/test"),
        serde_json::json!({}),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], false);
    assert!(
        body["message"]
            .as_str()
            .unwrap_or_default()
            .contains("disabled"),
        "unexpected body: {body}"
    );
}

/// Serves HTML at `/models` and optionally a models list at `/v1/models`.
async fn spawn_html_upstream(serves_v1: bool) -> String {
    let mut router = axum::Router::new().route(
        "/models",
        axum::routing::get(|| async {
            (
                StatusCode::NOT_FOUND,
                [(header::CONTENT_TYPE, "text/html")],
                "<!DOCTYPE html><html><body>not the api</body></html>",
            )
        }),
    );
    if serves_v1 {
        router = router.route(
            "/v1/models",
            axum::routing::get(|| async {
                axum::Json(serde_json::json!({
                    "object": "list",
                    "data": [{ "id": "gpt-4o" }]
                }))
            }),
        );
    }

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock upstream");
    let address = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    format!("http://{address}")
}

#[tokio::test]
async fn connection_test_suggests_v1_when_base_url_is_unversioned() {
    let (app, _db) = app(false).await;
    let base_url = spawn_html_upstream(true).await;
    let id = create_probe_connection(&app, &base_url).await;

    let (status, body) = json_request(
        &app,
        "POST",
        &format!("/api/connections/{id}/test"),
        serde_json::json!({}),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_GATEWAY);
    let message = body["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("base_url") && message.contains("/v1"),
        "expected an actionable hint, got: {message}"
    );
}

#[tokio::test]
async fn connection_test_summarizes_html_errors() {
    let (app, _db) = app(false).await;
    let base_url = spawn_html_upstream(false).await;
    let id = create_probe_connection(&app, &base_url).await;

    let (status, body) = json_request(
        &app,
        "POST",
        &format!("/api/connections/{id}/test"),
        serde_json::json!({}),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_GATEWAY);
    let message = body["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("HTML page") && !message.contains("<!DOCTYPE"),
        "HTML should be summarized, got: {message}"
    );
}

/// Serves a one-shot OpenAI-compatible SSE completion that replies "pong".
async fn spawn_chat_upstream() -> String {
    let router = axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(|| async {
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/event-stream")],
                concat!(
                    "data: {\"choices\":[{\"delta\":{\"content\":\"pong\"}}]}\n\n",
                    "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                    "data: [DONE]\n\n"
                ),
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

async fn create_alias_for(
    app: &axum::Router,
    connection_id: &str,
    prefix: &str,
    model: Option<&str>,
) -> String {
    let (status, alias) = json_request(
        app,
        "POST",
        "/api/aliases",
        serde_json::json!({
            "prefix": prefix,
            "connection_id": connection_id,
            "model_override": model
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    alias["id"].as_str().expect("alias id").to_string()
}

#[tokio::test]
async fn alias_chat_test_returns_a_completion() {
    let (app, _db) = app(false).await;
    let base_url = spawn_chat_upstream().await;
    let connection_id = create_probe_connection(&app, &base_url).await;
    let alias_id =
        create_alias_for(&app, &connection_id, "chatty", Some("deepseek-v4.1-flash")).await;

    let (status, body) = json_request(
        &app,
        "POST",
        &format!("/api/aliases/{alias_id}/test-chat"),
        serde_json::json!({ "prompt": "ping" }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true, "unexpected body: {body}");
    assert_eq!(body["content"], "pong");
    assert_eq!(body["model"], "deepseek-v4.1-flash");
    assert_eq!(body["attempts"], 1);
    assert_eq!(body["source"], "alias:chatty");
}

#[tokio::test]
async fn alias_chat_test_requires_a_model_without_an_override() {
    let (app, _db) = app(false).await;
    let base_url = spawn_chat_upstream().await;
    let connection_id = create_probe_connection(&app, &base_url).await;
    let alias_id = create_alias_for(&app, &connection_id, "bare-prefix", None).await;

    let (status, body) = json_request(
        &app,
        "POST",
        &format!("/api/aliases/{alias_id}/test-chat"),
        serde_json::json!({}),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], false);
    assert!(
        body["message"]
            .as_str()
            .unwrap_or_default()
            .contains("no model override"),
        "unexpected body: {body}"
    );

    // Passing a model explicitly makes the same alias testable.
    let (status, body) = json_request(
        &app,
        "POST",
        &format!("/api/aliases/{alias_id}/test-chat"),
        serde_json::json!({ "model": "deepseek-v4.1-flash" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true, "unexpected body: {body}");
}

#[tokio::test]
async fn bare_alias_name_works_through_chat_completions() {
    let (app, _db) = app(false).await;
    let base_url = spawn_chat_upstream().await;
    let connection_id = create_probe_connection(&app, &base_url).await;
    create_alias_for(
        &app,
        &connection_id,
        "bareroute",
        Some("deepseek-v4.1-flash"),
    )
    .await;

    let (status, body) = json_request(
        &app,
        "POST",
        "/v1/chat/completions",
        serde_json::json!({
            "model": "bareroute",
            "messages": [{ "role": "user", "content": "hi" }]
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "unexpected body: {body}");
    assert_eq!(body["choices"][0]["message"]["content"], "pong");
}

#[tokio::test]
async fn activity_endpoint_reports_attempts_and_connections() {
    let (app, _db) = app(false).await;
    let base_url = spawn_chat_upstream().await;
    let connection_id = create_probe_connection(&app, &base_url).await;
    create_alias_for(&app, &connection_id, "live", Some("deepseek-v4.1-flash")).await;

    let (status, _) = json_request(
        &app,
        "POST",
        "/v1/chat/completions",
        serde_json::json!({
            "model": "live",
            "messages": [{ "role": "user", "content": "hi" }]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = get(&app, "/api/activity").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["uptime_ms"].is_u64());

    let connections = body["connections"].as_array().expect("connections");
    let stats = connections
        .iter()
        .find(|stats| stats["id"] == connection_id)
        .expect("connection stats");
    assert!(stats["requests"].as_u64().unwrap_or(0) >= 1);
    assert_eq!(stats["failures"], 0);
    assert_eq!(stats["in_flight"], 0, "attempts finished, gauge drains");

    let events = body["events"].as_array().expect("events");
    assert!(
        events
            .iter()
            .any(|event| event["kind"] == "attempt.completed"),
        "expected an attempt event: {events:?}"
    );
    assert!(
        events
            .iter()
            .any(|event| event["kind"] == "request" && event["status"] == 200),
        "expected the HTTP request event: {events:?}"
    );
}

#[tokio::test]
async fn usage_filters_and_facets_are_queryable() {
    let (app, db) = app(false).await;
    let repo = UsageRepository::new(db.pool.clone());

    for (model, provider, connection) in [
        ("oa/gpt-4o", "openai-compatible", "openai-main"),
        ("kr/claude", "anthropic-native", "claude-main"),
    ] {
        repo.record(NewUsageRecord {
            api_key_id: None,
            requested_model: model.to_string(),
            resolved_provider: Some(provider.to_string()),
            resolved_model: None,
            connection_name: Some(connection.to_string()),
            attempt: 1,
            status: "ok".to_string(),
            prompt_tokens: 1,
            completion_tokens: 1,
            cached_tokens: 0,
            reasoning_tokens: 0,
            cost_usd: 0.5,
            cost_input_usd: 0.0,
            cost_output_usd: 0.0,
            cost_reasoning_usd: 0.0,
            latency_ms: 5,
        })
        .await
        .expect("record usage");
    }

    let (status, body) = get(&app, "/api/usage?provider=anthropic-native").await;
    assert_eq!(status, StatusCode::OK);
    let rows = body.as_array().expect("rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["requested_model"], "kr/claude");

    let (status, body) = get(&app, "/api/usage?connection=openai-main").await;
    assert_eq!(status, StatusCode::OK);
    let rows = body.as_array().expect("rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["connection_name"], "openai-main");

    let (status, body) = get(&app, "/api/usage/summary?model=GPT-4O").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["requests"], 1);

    let (status, body) = get(&app, "/api/usage/facets").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["models"],
        serde_json::json!(["kr/claude", "oa/gpt-4o"]),
        "models are ordered by usage count then name"
    );
    assert_eq!(
        body["providers"],
        serde_json::json!(["anthropic-native", "openai-compatible"])
    );
    assert_eq!(
        body["connections"],
        serde_json::json!(["claude-main", "openai-main"])
    );
}

/// Regression guard: flattened structs made `serde_urlencoded` reject `"100"`
/// for `i64` fields, so `/api/usage?limit=100` returned 400.
#[tokio::test]
async fn usage_query_strings_deserialize() {
    let (app, db) = app(false).await;
    UsageRepository::new(db.pool.clone())
        .record(NewUsageRecord {
            api_key_id: None,
            requested_model: "metered".to_string(),
            resolved_provider: Some("openai-compatible".to_string()),
            resolved_model: None,
            connection_name: Some("openai-main".to_string()),
            attempt: 1,
            status: "ok".to_string(),
            prompt_tokens: 1,
            completion_tokens: 1,
            cached_tokens: 0,
            reasoning_tokens: 0,
            cost_usd: 1.0,
            cost_input_usd: 0.0,
            cost_output_usd: 0.0,
            cost_reasoning_usd: 0.0,
            latency_ms: 5,
        })
        .await
        .expect("record usage");

    let (status, body) = get(&app, "/api/usage?limit=1&offset=0").await;
    assert_eq!(status, StatusCode::OK, "unexpected body: {body}");
    assert_eq!(body.as_array().map(Vec::len), Some(1));

    let (status, body) = get(
        &app,
        "/api/usage/summary?since=2026-01-01T00:00:00Z&model=metered",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "unexpected body: {body}");
    assert_eq!(body["requests"], 1);
}

// ------------------------------------------------------- key plans and rules

#[tokio::test]
async fn plan_crud_round_trip() {
    let (app, _db) = app(false).await;

    let (status, created) = json_request(
        &app,
        "POST",
        "/api/plans",
        serde_json::json!({
            "name": "team-free",
            "description": "shared rules",
            "allowed_models": ["openai/*", "openai/*", " gpt-4o-mini "],
            "rate_limit_per_minute": 60,
            "daily_budget_usd": 1.0,
            "weekly_budget_usd": 5.0,
            "monthly_budget_usd": 10.0,
            "lifetime_budget_usd": 100.0,
            "daily_token_limit": 250000,
            "monthly_token_limit": 5000000,
            "budget_mode": "warn",
            "expires_at": "2030-06-01T00:00:00Z",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "unexpected body: {created}");
    assert_eq!(
        created["allowed_models"],
        serde_json::json!(["openai/*", "gpt-4o-mini"]),
        "patterns should be trimmed and de-duplicated"
    );
    assert_eq!(created["budget_mode"], "warn");
    assert_eq!(created["daily_budget_usd"], 1.0);
    assert_eq!(created["lifetime_budget_usd"], 100.0);
    assert_eq!(created["daily_token_limit"], 250000);
    assert_eq!(created["monthly_token_limit"], 5000000);
    assert_eq!(created["expires_at"], "2030-06-01T00:00:00Z");

    let plan_id = created["id"].as_str().expect("plan id").to_string();

    let (status, list) = get(&app, "/api/plans").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().map(Vec::len), Some(1));

    let (status, updated) = json_request(
        &app,
        "PATCH",
        &format!("/api/plans/{plan_id}"),
        serde_json::json!({
            "allowed_models": [],
            "budget_mode": "off",
            "daily_budget_usd": null,
            "monthly_budget_usd": null,
            "lifetime_budget_usd": null,
            "daily_token_limit": null,
            "expires_at": null,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["allowed_models"], serde_json::json!([]));
    assert!(updated["daily_budget_usd"].is_null());
    assert_eq!(updated["monthly_budget_usd"], serde_json::Value::Null);
    assert!(updated["lifetime_budget_usd"].is_null());
    assert!(updated["daily_token_limit"].is_null());
    assert!(updated["expires_at"].is_null());
    assert_eq!(updated["weekly_budget_usd"], 5.0);
    assert_eq!(updated["monthly_token_limit"], 5000000);

    let response =
        raw_request_with_auth(&app, "DELETE", &format!("/api/plans/{plan_id}"), None, None).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let (_, list) = get(&app, "/api/plans").await;
    assert!(list.as_array().expect("array").is_empty());
}

#[tokio::test]
async fn duplicate_plan_names_are_rejected() {
    let (app, _db) = app(false).await;
    let body = serde_json::json!({ "name": "dup" });

    let (status, _) = json_request(&app, "POST", "/api/plans", body.clone()).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, error) = json_request(&app, "POST", "/api/plans", body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        error["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("already exists")
    );
}

#[tokio::test]
async fn key_allowlist_blocks_models_outside_the_list() {
    let mut config = RouterConfig::default();
    config.server.require_api_key = true;
    let (app, db) = app_with_config(config).await;

    let created = ApiKeyRepository::new(db.pool.clone())
        .create(CreateApiKey {
            name: "restricted".to_string(),
            enabled: true,
            rate_limit_per_minute: None,
            daily_budget_usd: None,
            weekly_budget_usd: None,
            monthly_budget_usd: None,
            lifetime_budget_usd: None,
            daily_token_limit: None,
            weekly_token_limit: None,
            monthly_token_limit: None,
            lifetime_token_limit: None,
            budget_mode: None,
            plan_id: None,
            allowed_models: Some(vec!["openai/gpt-4o".to_string()]),
            expires_at: None,
        })
        .await
        .expect("key");

    let (status, body) = json_request_with_auth(
        &app,
        "POST",
        "/v1/chat/completions",
        serde_json::json!({
            "model": "anthropic/claude",
            "messages": [{ "role": "user", "content": "hi" }]
        }),
        Some(&created.secret),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["type"], "permission_error");

    // Allowed models pass the allowlist and then fail on the unknown reference.
    let (status, body) = json_request_with_auth(
        &app,
        "POST",
        "/v1/chat/completions",
        serde_json::json!({
            "model": "openai/gpt-4o",
            "messages": [{ "role": "user", "content": "hi" }]
        }),
        Some(&created.secret),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "unexpected body: {body}");
}

#[tokio::test]
async fn plan_rules_apply_to_attached_keys() {
    let mut config = RouterConfig::default();
    config.server.require_api_key = true;
    let (app, _db) = app_with_config(config).await;

    let (status, plan) = json_request(
        &app,
        "POST",
        "/api/plans",
        serde_json::json!({
            "name": "template",
            "allowed_models": ["openai/*"],
            "rate_limit_per_minute": 2,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "unexpected body: {plan}");

    let (status, created) = json_request(
        &app,
        "POST",
        "/api/keys",
        serde_json::json!({ "name": "from-plan", "plan_id": plan["id"] }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "unexpected body: {created}");
    assert_eq!(created["key"]["plan_id"], plan["id"]);
    let secret = created["secret"].as_str().expect("secret").to_string();

    // The plan's allowlist applies to the key.
    let (status, _) = json_request_with_auth(
        &app,
        "POST",
        "/v1/chat/completions",
        serde_json::json!({
            "model": "anthropic/claude",
            "messages": [{ "role": "user", "content": "hi" }]
        }),
        Some(&secret),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // The plan's rate limit applies too: rate limiting runs before the
    // allowlist, so the denied request above and this one use up the two
    // tokens of the minute and the next request is throttled.
    let (status, _) = json_request_with_auth(
        &app,
        "POST",
        "/v1/chat/completions",
        serde_json::json!({
            "model": "openai/gpt-4o",
            "messages": [{ "role": "user", "content": "hi" }]
        }),
        Some(&secret),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, body) = json_request_with_auth(
        &app,
        "POST",
        "/v1/chat/completions",
        serde_json::json!({
            "model": "openai/gpt-4o",
            "messages": [{ "role": "user", "content": "hi" }]
        }),
        Some(&secret),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["error"]["type"], "rate_limit_error");
}

#[tokio::test]
async fn deleting_a_plan_detaches_it_from_keys() {
    let (app, _db) = app(false).await;

    let (_, plan) = json_request(
        &app,
        "POST",
        "/api/plans",
        serde_json::json!({ "name": "temporary" }),
    )
    .await;
    let plan_id = plan["id"].as_str().expect("plan id").to_string();

    let (_, created) = json_request(
        &app,
        "POST",
        "/api/keys",
        serde_json::json!({ "name": "attached", "plan_id": plan_id }),
    )
    .await;
    assert_eq!(created["key"]["plan_id"], plan["id"]);

    let response =
        raw_request_with_auth(&app, "DELETE", &format!("/api/plans/{plan_id}"), None, None).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let (_, keys) = get(&app, "/api/keys").await;
    assert_eq!(keys[0]["plan_id"], serde_json::Value::Null);
}

#[tokio::test]
async fn key_plan_and_allowlist_can_be_cleared() {
    let (app, _db) = app(false).await;

    let (_, plan) = json_request(
        &app,
        "POST",
        "/api/plans",
        serde_json::json!({ "name": "p" }),
    )
    .await;
    let plan_id = plan["id"].as_str().expect("plan id").to_string();

    let (_, created) = json_request(
        &app,
        "POST",
        "/api/keys",
        serde_json::json!({ "name": "k", "plan_id": plan_id, "allowed_models": ["openai/*"] }),
    )
    .await;
    let key_id = created["key"]["id"].as_str().expect("key id").to_string();

    let (status, updated) = json_request(
        &app,
        "PATCH",
        &format!("/api/keys/{key_id}"),
        serde_json::json!({ "plan_id": null, "allowed_models": null }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["plan_id"], serde_json::Value::Null);
    assert_eq!(updated["allowed_models"], serde_json::Value::Null);
}

#[tokio::test]
async fn key_with_an_unknown_plan_is_rejected() {
    let (app, _db) = app(false).await;

    let (status, body) = json_request(
        &app,
        "POST",
        "/api/keys",
        serde_json::json!({ "name": "orphan", "plan_id": "does-not-exist" }),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("plan")
    );
}

// ------------------------------------------------------------------ pricing

#[tokio::test]
async fn pricing_overrides_round_trip_through_the_api() {
    let (app, _db) = app(false).await;

    let (status, body) = json_request(
        &app,
        "PUT",
        "/api/pricing",
        serde_json::json!({
            "prices": [{
                "model": "gpt-4o",
                "input_per_million_usd": 1.0,
                "output_per_million_usd": 2.0,
                "cache_read_per_million_usd": 0.5,
                "reasoning_per_million_usd": 3.0
            }]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "unexpected body: {body}");
    assert_eq!(body["updated"], 1);

    let (status, body) = get(&app, "/api/pricing").await;
    assert_eq!(status, StatusCode::OK);
    let rows = body.as_array().expect("rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["model"], "gpt-4o");
    assert_eq!(rows[0]["source"], "override");
    assert_eq!(rows[0]["reasoning_per_million_usd"], 3.0);

    let (status, body) = json_request(
        &app,
        "PUT",
        "/api/pricing",
        serde_json::json!({
            "prices": [{ "model": "bad", "input_per_million_usd": -1.0, "output_per_million_usd": 1.0 }]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["type"], "invalid_request_error");

    let response =
        raw_request_with_auth(&app, "DELETE", "/api/pricing?model=gpt-4o", None, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await["deleted"], 1);

    let (status, body) = get(&app, "/api/pricing").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.as_array().expect("rows").is_empty());

    let (status, body) = get(&app, "/api/pricing/sync").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_null(), "no crawl has run yet");
}

#[tokio::test]
async fn pricing_match_reports_the_catalog_key() {
    let (app, _db) = app(false).await;

    let (status, _) = json_request(
        &app,
        "PUT",
        "/api/pricing",
        serde_json::json!({
            "prices": [{
                "model": "gpt-4o",
                "input_per_million_usd": 2.5,
                "output_per_million_usd": 10.0
            }]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // A prefixed id resolves to the canonical row, and the response says so.
    let (status, body) = get(&app, "/api/pricing/match?model=azure/gpt-4o").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["matched"], "gpt-4o");
    assert_eq!(body["source"], "override");
    assert_eq!(body["price"]["input_per_million_usd"], 2.5);

    let (status, body) = get(&app, "/api/pricing/match?model=unknown-model").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["matched"].is_null());

    let (status, _) = get(&app, "/api/pricing/match?model=%20").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn settings_toggle_client_auth_and_hot_apply() {
    let (app, _db) = app(false).await;

    let (status, body) = get(&app, "/api/settings").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["server"]["require_api_key"], false);
    assert_eq!(body["router"]["max_attempts"], 5);
    assert_eq!(body["deployment"]["binds_loopback"], true);
    assert!(
        body["overrides"].as_array().expect("overrides").is_empty(),
        "nothing customized yet"
    );

    let (status, body) = json_request(
        &app,
        "PATCH",
        "/api/settings",
        serde_json::json!({ "require_api_key": true }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["server"]["require_api_key"], true);
    assert_eq!(
        body["overrides"],
        serde_json::json!(["server.require_api_key"])
    );

    // /api/init reflects the new value immediately.
    let (status, body) = get(&app, "/api/init").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["require_api_key"], true);

    // /v1 now rejects unauthenticated calls.
    let (status, _) = get(&app, "/v1/models").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Resetting drops the override and reopens /v1.
    let response = raw_request_with_auth(&app, "DELETE", "/api/settings", None, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await["server"]["require_api_key"],
        false
    );

    let (status, _) = get(&app, "/v1/models").await;
    assert_ne!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn settings_admin_token_is_write_only_and_enforced() {
    let (app, _db) = app(false).await;

    let (status, body) = json_request(
        &app,
        "PATCH",
        "/api/settings",
        serde_json::json!({ "admin_token": "new-secret" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["server"]["admin_token_set"], true);
    assert!(
        body["server"].get("admin_token").is_none(),
        "the token value must never be returned: {body}"
    );

    // From now on the admin API demands the new token.
    let (status, _) = get(&app, "/api/settings").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, body) = get_with_auth(&app, "/api/settings", Some("new-secret")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["overrides"], serde_json::json!(["server.admin_token"]));

    // A blank string removes the override and reopens the loopback admin API.
    let (status, _) = json_request_with_auth(
        &app,
        "PATCH",
        "/api/settings",
        serde_json::json!({ "admin_token": "" }),
        Some("new-secret"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = get(&app, "/api/settings").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["server"]["admin_token_set"], false);
}

#[tokio::test]
async fn settings_reject_unsafe_or_malformed_values() {
    let mut config = RouterConfig::default();
    config.server.host = "0.0.0.0".to_string();
    config.server.admin_token = Some("admin-secret".to_string());
    let (app, _db) = app_with_config(config).await;

    // Clearing the only admin token would leave a public admin API open.
    let (status, body) = json_request_with_auth(
        &app,
        "PATCH",
        "/api/settings",
        serde_json::json!({ "admin_token": null }),
        Some("admin-secret"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["type"], "invalid_request_error");

    // Zero fallback tiers is nonsense.
    let (status, body) = json_request_with_auth(
        &app,
        "PATCH",
        "/api/settings",
        serde_json::json!({ "max_attempts": 0 }),
        Some("admin-secret"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("max_attempts"),
        "message should name the field: {body}"
    );
}

#[tokio::test]
async fn backup_downloads_a_snapshot_that_restore_accepts() {
    let (app, db, _directory) = file_app().await;
    let secret = mint_key(&db, "keep-me").await;

    let response = raw_request_with_auth(&app, "GET", "/api/backup", None, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("application/octet-stream")
    );
    assert!(
        response.headers().contains_key(header::CONTENT_DISPOSITION),
        "the download should carry a filename"
    );
    let backup = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    assert!(
        backup.starts_with(b"SQLite format 3\0"),
        "the download must be a SQLite database"
    );

    // Drop the key, then bring it back through a restore.
    let (status, keys) = get(&app, "/api/keys").await;
    assert_eq!(status, StatusCode::OK);
    let key_id = keys[0]["id"].as_str().expect("key id").to_string();
    let response =
        raw_request_with_auth(&app, "DELETE", &format!("/api/keys/{key_id}"), None, None).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let (status, _) = get_with_auth(&app, "/v1/models", Some(&secret)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "the key is gone");

    let (status, body) = post_bytes(&app, "/api/restore", backup.to_vec()).await;
    assert_eq!(status, StatusCode::OK, "restore failed: {body}");
    assert_eq!(body["total_rows"], 1);

    let (status, keys) = get(&app, "/api/keys").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(keys.as_array().expect("array").len(), 1);

    // The restored key authenticates again.
    let (status, _) = get_with_auth(&app, "/v1/models", Some(&secret)).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn restore_rejects_invalid_uploads() {
    let (app, _db, _directory) = file_app().await;

    let (status, body) =
        post_bytes(&app, "/api/restore", b"definitely not a database".to_vec()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["type"], "invalid_request_error");

    let (status, body) = post_bytes(&app, "/api/restore", Vec::new()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("empty"),
        "message should say the upload is empty: {body}"
    );
}

#[tokio::test]
async fn model_catalog_lists_providers_and_prices() {
    let (app, _db) = app(false).await;

    let (_, connection) = json_request(
        &app,
        "POST",
        "/api/connections",
        serde_json::json!({
            "name": "openai-main",
            "provider_type": "openai-compatible",
            "base_url": "https://example.invalid/v1"
        }),
    )
    .await;
    let connection_id = connection["id"].as_str().expect("connection id");

    // A pinned alias and an open prefix that accepts any upstream model.
    json_request(
        &app,
        "POST",
        "/api/aliases",
        serde_json::json!({
            "prefix": "oa",
            "connection_id": connection_id,
            "model_override": "gpt-4o"
        }),
    )
    .await;
    json_request(
        &app,
        "POST",
        "/api/aliases",
        serde_json::json!({ "prefix": "any", "connection_id": connection_id }),
    )
    .await;

    json_request(
        &app,
        "POST",
        "/api/combos",
        serde_json::json!({ "name": "smart", "entries": ["oa", "any/gpt-4o-mini"] }),
    )
    .await;

    json_request(
        &app,
        "PUT",
        "/api/pricing",
        serde_json::json!({
            "prices": [{ "model": "gpt-4o", "input_per_million_usd": 2.5, "output_per_million_usd": 10.0 }]
        }),
    )
    .await;

    let (status, body) = get(&app, "/api/models").await;
    assert_eq!(status, StatusCode::OK);
    let data = body["data"].as_array().expect("array");

    let alias = data
        .iter()
        .find(|row| row["id"] == "oa")
        .expect("alias row");
    assert_eq!(alias["kind"], "alias");
    assert_eq!(alias["provider"], "openai-main");
    assert_eq!(alias["provider_type"], "openai-compatible");
    assert_eq!(alias["upstream_model"], "gpt-4o");
    assert_eq!(alias["price"]["input_per_million_usd"], 2.5);
    assert_eq!(alias["price_source"], "override");

    // An alias without a pinned model has no concrete target or price.
    let open = data
        .iter()
        .find(|row| row["id"] == "any")
        .expect("open alias row");
    assert!(open["upstream_model"].is_null());
    assert!(open["price"].is_null());

    // Combos expand to one row per resolved tier.
    let tiers: Vec<&serde_json::Value> = data.iter().filter(|row| row["id"] == "smart").collect();
    assert_eq!(tiers.len(), 2);
    assert_eq!(tiers[0]["tier"], 1);
    assert_eq!(tiers[0]["provider"], "openai-main");
    assert_eq!(tiers[0]["upstream_model"], "gpt-4o");
    assert_eq!(tiers[0]["price"]["output_per_million_usd"], 10.0);
    assert_eq!(tiers[1]["tier"], 2);
    assert_eq!(tiers[1]["upstream_model"], "gpt-4o-mini");
    assert!(tiers[1]["price"].is_null());
}

#[tokio::test]
async fn public_usage_is_scoped_to_the_calling_key() {
    let (app, db) = app(false).await;

    let repository = ApiKeyRepository::new(db.pool.clone());
    let first = repository
        .create(CreateApiKey {
            name: "first".to_string(),
            enabled: true,
            rate_limit_per_minute: None,
            daily_budget_usd: None,
            weekly_budget_usd: None,
            monthly_budget_usd: None,
            lifetime_budget_usd: None,
            daily_token_limit: None,
            weekly_token_limit: None,
            monthly_token_limit: None,
            lifetime_token_limit: None,
            budget_mode: None,
            plan_id: None,
            allowed_models: None,
            expires_at: None,
        })
        .await
        .expect("first key");
    let second = repository
        .create(CreateApiKey {
            name: "second".to_string(),
            enabled: true,
            rate_limit_per_minute: None,
            daily_budget_usd: None,
            weekly_budget_usd: None,
            monthly_budget_usd: None,
            lifetime_budget_usd: None,
            daily_token_limit: None,
            weekly_token_limit: None,
            monthly_token_limit: None,
            lifetime_token_limit: None,
            budget_mode: None,
            plan_id: None,
            allowed_models: None,
            expires_at: None,
        })
        .await
        .expect("second key");

    record_usage(&db, &first.key.id, 1.5).await;
    record_usage(&db, &first.key.id, 0.5).await;
    record_usage(&db, &second.key.id, 9.0).await;

    let (status, body) = get_with_auth(&app, "/api/public/usage", Some(&first.secret)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["key"]["name"], "first");
    assert_eq!(body["summary"]["requests"], 2);
    assert_eq!(body["summary"]["cost_usd"], 2.0);
    let models = body["models"].as_array().expect("models");
    assert_eq!(models.len(), 1);
    assert_eq!(models[0]["model"], "metered");
    assert_eq!(models[0]["requests"], 2);

    // The trend defaults to day buckets and only sees this key's rows.
    assert_eq!(body["bucket"], "day");
    let series = body["timeseries"].as_array().expect("timeseries");
    assert_eq!(series.len(), 1);
    assert_eq!(series[0]["model"], "metered");
    assert_eq!(series[0]["requests"], 2);

    let (status, _) =
        get_with_auth(&app, "/api/public/usage?bucket=week", Some(&first.secret)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // A future `since` yields an empty window.
    let (status, body) = get_with_auth(
        &app,
        "/api/public/usage?since=2999-01-01T00:00:00Z",
        Some(&first.secret),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["summary"]["requests"], 0);
    assert!(body["models"].as_array().expect("models").is_empty());

    // A past `until` excludes everything too.
    let (status, body) = get_with_auth(
        &app,
        "/api/public/usage?until=2000-01-01T00:00:00Z",
        Some(&first.secret),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["summary"]["requests"], 0);
}

#[tokio::test]
async fn public_usage_requires_a_valid_key() {
    let (app, _db) = app(false).await;

    let (status, _) = get(&app, "/api/public/usage").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _) = get_with_auth(&app, "/api/public/usage", Some("sk-router-nope")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _) = get_with_auth(&app, "/api/public/models", Some("sk-router-nope")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn public_models_are_filtered_by_the_key_allowlist() {
    let (app, _db) = app(false).await;

    let (_, connection) = json_request(
        &app,
        "POST",
        "/api/connections",
        serde_json::json!({
            "name": "openai-main",
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
        serde_json::json!({
            "prefix": "oa",
            "connection_id": connection_id,
            "model_override": "gpt-4o"
        }),
    )
    .await;
    json_request(
        &app,
        "POST",
        "/api/aliases",
        serde_json::json!({ "prefix": "any", "connection_id": connection_id }),
    )
    .await;
    json_request(
        &app,
        "POST",
        "/api/combos",
        serde_json::json!({ "name": "smart", "entries": ["oa"] }),
    )
    .await;
    json_request(
        &app,
        "PUT",
        "/api/pricing",
        serde_json::json!({
            "prices": [{ "model": "gpt-4o", "input_per_million_usd": 2.5, "output_per_million_usd": 10.0 }]
        }),
    )
    .await;

    let ids = |body: &serde_json::Value| -> Vec<String> {
        body["data"]
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|row| row["id"].as_str().map(str::to_string))
            .collect()
    };

    // An exact pattern only exposes that alias, with its price.
    let (_, restricted) = json_request(
        &app,
        "POST",
        "/api/keys",
        serde_json::json!({ "name": "restricted", "allowed_models": ["oa"] }),
    )
    .await;
    let secret = restricted["secret"].as_str().expect("secret");

    let (status, body) = get_with_auth(&app, "/api/public/models", Some(secret)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["allowed_models"], serde_json::json!(["oa"]));
    assert_eq!(ids(&body), vec!["oa".to_string()]);
    assert_eq!(body["data"][0]["upstream_model"], "gpt-4o");
    assert_eq!(body["data"][0]["price"]["input_per_million_usd"], 2.5);

    // A wildcard exposes the open alias behind that prefix.
    let (_, wildcard) = json_request(
        &app,
        "POST",
        "/api/keys",
        serde_json::json!({ "name": "wildcard", "allowed_models": ["any/*"] }),
    )
    .await;
    let secret = wildcard["secret"].as_str().expect("secret");

    let (status, body) = get_with_auth(&app, "/api/public/models", Some(secret)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ids(&body), vec!["any".to_string()]);

    // Without a pattern the key sees the whole catalog.
    let (_, open) = json_request(
        &app,
        "POST",
        "/api/keys",
        serde_json::json!({ "name": "open" }),
    )
    .await;
    let secret = open["secret"].as_str().expect("secret");

    let (status, body) = get_with_auth(&app, "/api/public/models", Some(secret)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body["allowed_models"]
            .as_array()
            .expect("patterns")
            .is_empty()
    );
    let catalog = ids(&body);
    assert!(catalog.contains(&"oa".to_string()), "catalog: {catalog:?}");
    assert!(catalog.contains(&"any".to_string()), "catalog: {catalog:?}");
    assert!(
        catalog.contains(&"smart".to_string()),
        "catalog: {catalog:?}"
    );
}

#[tokio::test]
async fn public_usage_can_be_disabled() {
    let mut config = RouterConfig::default();
    config.server.public_usage = false;
    let (app, db) = app_with_config(config).await;
    let secret = mint_key(&db, "metered").await;

    let (status, body) = get_with_auth(&app, "/api/public/usage", Some(&secret)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["type"], "permission_error");

    let (status, _) = get_with_auth(&app, "/api/public/models", Some(&secret)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
