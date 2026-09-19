//! Endpoint shape smoke tests: routing, auth, and error bodies.

use alnair_router::config::{RateLimitConfig, RouterConfig};
use alnair_router::db::Db;
use alnair_router::db::repos::api_keys::{ApiKeyRepository, CreateApiKey};
use alnair_router::db::repos::usage::{NewUsageRecord, UsageRepository};
use alnair_router::settings::SettingsOverrides;
use alnair_router::{AppState, build_router};
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use tower::ServiceExt;

const TEST_SECRET: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

/// The same cipher the app under test builds from `TEST_SECRET`, so secrets
/// written by helpers decrypt under the running app.
fn test_cipher() -> std::sync::Arc<alnair_router::crypto::CredentialCipher> {
    use alnair_router::config::SecretsConfig;
    std::sync::Arc::new(
        alnair_router::crypto::CredentialCipher::from_config(&SecretsConfig {
            key: Some(TEST_SECRET.to_string()),
        })
        .expect("cipher"),
    )
}

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
    ApiKeyRepository::new(db.pool.clone(), test_cipher())
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
            ..Default::default()
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

/// Reads a response body as text, for SSE endpoints whose frames are not JSON.
async fn body_text(response: axum::response::Response) -> String {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    String::from_utf8_lossy(&bytes).into_owned()
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
async fn update_is_admin_guarded_and_reports_the_running_version() {
    // A local port with nothing listening: the lookup fails fast instead of
    // reaching out to GitHub from the test suite.
    let mut config = RouterConfig::default();
    config.server.admin_token = Some("admin-secret".to_string());
    config.update.api_url = "http://127.0.0.1:1".to_string();
    let (app, _db) = app_with_config(config).await;

    // The update endpoint is an admin route, so it follows the same posture.
    let (status, _) = get(&app, "/api/update").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, body) = get_with_auth(&app, "/api/update", Some("admin-secret")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(body["check_enabled"], true);
    // An unreachable host reports the failure but still names the running
    // version, which is what the dashboard falls back to.
    assert!(body["latest_version"].is_null());
    assert!(body["error"].is_string(), "expected an error: {body}");
}

/// Serves a canned `/repos/{repo}/releases/latest` payload.
async fn spawn_releases_upstream(tag: &'static str) -> String {
    let router = axum::Router::new().route(
        "/repos/{owner}/{repo}/releases/latest",
        axum::routing::get(move || async move {
            axum::Json(serde_json::json!({
                "tag_name": tag,
                "html_url": format!("https://example.test/tag/{tag}"),
                "body": "notes",
                "published_at": "2026-01-02T03:04:05Z",
                "prerelease": false,
            }))
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock releases");
    let address = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    format!("http://{address}")
}

#[tokio::test]
async fn update_detects_a_newer_release() {
    let base = spawn_releases_upstream("v99.0.0").await;
    let mut config = RouterConfig::default();
    config.update.repo = "owner/repo".to_string();
    config.update.api_url = base;
    let (app, _db) = app_with_config(config).await;

    let (status, body) = get(&app, "/api/update?refresh=true").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["latest_version"], "99.0.0");
    assert_eq!(body["update_available"], true);
    assert_eq!(body["release_url"], "https://example.test/tag/v99.0.0");
    assert_eq!(body["release_notes"], "notes");
    assert!(body["error"].is_null());
}

#[tokio::test]
async fn update_does_not_flag_the_running_version_as_newer() {
    let base = spawn_releases_upstream("v0.1.0").await;
    let mut config = RouterConfig::default();
    config.update.repo = "owner/repo".to_string();
    config.update.api_url = base;
    let (app, _db) = app_with_config(config).await;

    let (status, body) = get(&app, "/api/update?refresh=true").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["latest_version"], "0.1.0");
    assert_eq!(body["update_available"], false);
}

#[tokio::test]
async fn update_check_can_be_disabled_by_configuration() {
    let mut config = RouterConfig::default();
    config.update.check_enabled = false;
    let (app, _db) = app_with_config(config).await;

    let (status, body) = get(&app, "/api/update").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["check_enabled"], false);
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    assert!(body["latest_version"].is_null());
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
async fn chat_completion_accepts_a_multi_megabyte_body() {
    let (app, _db) = app(false).await;
    // Comfortably past the axum default 2 MiB limit: large-context requests
    // must reach the router instead of failing with 413.
    let prompt = "x".repeat(3 * 1024 * 1024);
    let (status, body) = json_request(
        &app,
        "POST",
        "/v1/chat/completions",
        serde_json::json!({
            "model": "nope/nothing",
            "messages": [{ "role": "user", "content": prompt }]
        }),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["type"], "not_found_error");
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
    // A connection name is not a routable reference, so it must not be offered
    // as a model id.
    assert!(
        !ids.contains(&"glm-main".to_string()),
        "connection must not be listed: {ids:?}"
    );
}

#[tokio::test]
async fn a_connection_name_is_not_a_model_reference() {
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

    // Neither the bare name nor `name/model` resolves: a connection needs an
    // alias before callers can address it.
    for reference in ["glm-main", "glm-main/glm-4.6"] {
        let (status, body) = json_request(
            &app,
            "POST",
            "/v1/chat/completions",
            serde_json::json!({
                "model": reference,
                "messages": [{ "role": "user", "content": "hi" }]
            }),
        )
        .await;

        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "reference {reference}: {body}"
        );
    }

    // Adding an alias makes the same connection reachable, proving the two
    // rejections above came from resolution rather than a broken upstream.
    json_request(
        &app,
        "POST",
        "/api/aliases",
        serde_json::json!({ "prefix": "glm", "connection_id": connection_id }),
    )
    .await;

    let (_, body) = get(&app, "/v1/models").await;
    let ids: Vec<String> = body["data"]
        .as_array()
        .expect("array")
        .iter()
        .filter_map(|entry| entry["id"].as_str().map(str::to_string))
        .collect();
    assert!(ids.contains(&"glm".to_string()), "alias listed: {ids:?}");
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
    let created =
        alnair_router::db::repos::api_keys::ApiKeyRepository::new(db.pool.clone(), test_cipher())
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

    let created = ApiKeyRepository::new(db.pool.clone(), test_cipher())
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

    let created = ApiKeyRepository::new(db.pool.clone(), test_cipher())
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

    let created = ApiKeyRepository::new(db.pool.clone(), test_cipher())
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

    let created = ApiKeyRepository::new(db.pool.clone(), test_cipher())
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

    let created = ApiKeyRepository::new(db.pool.clone(), test_cipher())
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

    let created = ApiKeyRepository::new(db.pool.clone(), test_cipher())
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

    let created = ApiKeyRepository::new(db.pool.clone(), test_cipher())
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
    let created = ApiKeyRepository::new(db.pool.clone(), test_cipher())
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

#[tokio::test]
async fn headroom_test_reports_an_unreachable_proxy_without_failing() {
    let (app, _db) = app(false).await;

    // Nothing is listening on this port; the probe must still answer 200 with a
    // readable failure rather than surfacing a 5xx.
    let response = raw_request_with_auth(
        &app,
        "POST",
        "/api/token-saver/headroom/test",
        Some(serde_json::json!({ "url": "http://127.0.0.1:9" })),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(body["ok"], serde_json::json!(false));
    assert!(
        body["message"]
            .as_str()
            .unwrap_or_default()
            .contains("127.0.0.1:9"),
        "failure should name the URL: {body}"
    );
}

/// A tool result the slimmer reliably compresses: a long unified diff.
fn bulky_diff() -> String {
    let mut diff = String::from("diff --git a/x b/x\n@@ -1 +1 @@\n");
    for index in 0..200 {
        diff.push_str(&format!("+line {index}\n"));
    }
    diff
}

#[tokio::test]
async fn playground_shrinks_a_bulky_tool_result_and_reports_each_step() {
    let (app, _db) = app(false).await;

    let response = raw_request_with_auth(
        &app,
        "POST",
        "/api/token-saver/playground",
        Some(serde_json::json!({
            "messages": [
                { "role": "system", "content": "You are helpful." },
                { "role": "user", "content": "review this" },
                { "role": "tool", "content": bulky_diff(), "tool_call_id": "call_1" }
            ]
        })),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).expect("json");

    // The default config enables the slimmer, so the pipeline is active.
    assert_eq!(body["active"], serde_json::json!(true));

    let before = body["tokens_before"].as_u64().expect("before");
    let after = body["tokens_after"].as_u64().expect("after");
    assert!(after < before, "prompt should shrink: {before} -> {after}");
    assert_eq!(
        body["prompt_tokens_saved"],
        serde_json::json!(before as i64 - after as i64)
    );

    // The response must show the rewritten message, not merely claim a saving.
    let rewritten = body["after"][2]["content"].as_str().expect("tool content");
    assert!(
        rewritten.len() < bulky_diff().len(),
        "the tool result sent upstream should be shorter"
    );
    assert_eq!(
        body["before"][2]["content"].as_str(),
        Some(&bulky_diff()[..])
    );

    let steps = body["steps"].as_array().expect("steps");
    let rtk = steps
        .iter()
        .find(|step| step["saver"] == serde_json::json!("rtk"))
        .expect("rtk step");
    assert_eq!(rtk["applied"], serde_json::json!(true));
    assert_eq!(rtk["side"], serde_json::json!("input"));
    assert_eq!(rtk["label"], serde_json::json!("RTK / Slimmer"));
    assert!(
        rtk["delta"].as_i64().expect("delta") < 0,
        "an input saver must report a negative delta"
    );
}

#[tokio::test]
async fn playground_reports_an_idle_pipeline_without_inventing_savings() {
    let (app, _db) = app(false).await;

    let response = raw_request_with_auth(
        &app,
        "POST",
        "/api/token-saver/playground",
        Some(serde_json::json!({
            "messages": [{ "role": "user", "content": "hi" }],
            "overrides": {
                "slimmer_enabled": false,
                "headroom_enabled": false,
                "terse_enabled": false,
                "caveman_enabled": false,
                "ponytail_enabled": false
            }
        })),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).expect("json");

    assert_eq!(body["active"], serde_json::json!(false));
    assert_eq!(body["steps"].as_array().expect("steps").len(), 0);
    assert_eq!(body["prompt_tokens_saved"], serde_json::json!(0));
    assert_eq!(body["before"], body["after"], "nothing may be rewritten");
}

#[tokio::test]
async fn playground_shows_a_directive_costing_prompt_tokens() {
    let (app, _db) = app(false).await;

    let response = raw_request_with_auth(
        &app,
        "POST",
        "/api/token-saver/playground",
        Some(serde_json::json!({
            "messages": [{ "role": "user", "content": "hi" }],
            "assumed_completion_tokens": 1000,
            "overrides": {
                "slimmer_enabled": false,
                "terse_enabled": true
            }
        })),
        None,
    )
    .await;
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).expect("json");

    // The directive is injected, which costs prompt tokens — the playground must
    // say so rather than presenting the injection as a free win.
    let terse = body["steps"]
        .as_array()
        .expect("steps")
        .iter()
        .find(|step| step["saver"] == serde_json::json!("terse"))
        .expect("terse step")
        .clone();
    assert_eq!(terse["side"], serde_json::json!("output"));
    assert!(
        terse["delta"].as_i64().expect("delta") > 0,
        "a directive adds the instruction text it injects"
    );

    // An assumed completion lets the output side be priced.
    assert!(
        body["totals"]["saved_terse_tokens"]
            .as_u64()
            .expect("terse tokens")
            > 0
    );
}

#[tokio::test]
async fn playground_without_a_completion_length_reports_no_output_estimate() {
    let (app, _db) = app(false).await;

    let response = raw_request_with_auth(
        &app,
        "POST",
        "/api/token-saver/playground",
        Some(serde_json::json!({
            "messages": [{ "role": "user", "content": "hi" }],
            "overrides": { "slimmer_enabled": false, "terse_enabled": true }
        })),
        None,
    )
    .await;
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).expect("json");

    // Without a completion length the output savers cannot be priced, so the
    // playground reports nothing rather than guessing a number.
    assert_eq!(body["totals"], serde_json::Value::Null);
}

#[tokio::test]
async fn playground_rejects_a_configuration_the_settings_page_would_reject() {
    let (app, _db) = app(false).await;

    // Terse and caveman both write a system directive, so validation forbids
    // them together. The playground must not demonstrate a config that cannot
    // actually be saved.
    let response = raw_request_with_auth(
        &app,
        "POST",
        "/api/token-saver/playground",
        Some(serde_json::json!({
            "messages": [{ "role": "user", "content": "hi" }],
            "overrides": { "terse_enabled": true, "caveman_enabled": true }
        })),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("mutually exclusive"),
        "error should explain the conflict: {body}"
    );
}

#[tokio::test]
async fn playground_fails_open_when_headroom_is_unreachable() {
    let (app, _db) = app(false).await;

    let response = raw_request_with_auth(
        &app,
        "POST",
        "/api/token-saver/playground",
        Some(serde_json::json!({
            "messages": [{ "role": "user", "content": "hi" }],
            "overrides": {
                "slimmer_enabled": false,
                "headroom_enabled": true,
                "headroom_url": "http://127.0.0.1:9"
            }
        })),
        None,
    )
    .await;

    // Fail-open is the contract: an unreachable proxy must not fail the run.
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).expect("json");

    let headroom = body["steps"]
        .as_array()
        .expect("steps")
        .iter()
        .find(|step| step["saver"] == serde_json::json!("headroom"))
        .expect("headroom step")
        .clone();
    assert_eq!(headroom["applied"], serde_json::json!(false));
    assert_eq!(body["before"], body["after"], "nothing may be rewritten");
    assert!(
        body["notes"]
            .as_array()
            .expect("notes")
            .iter()
            .any(|note| note.as_str().unwrap_or_default().contains("headroom")),
        "the decline should be explained: {body}"
    );
}

#[tokio::test]
async fn usage_summary_carries_a_savings_block() {
    let (app, db) = app(false).await;
    let repo = alnair_router::db::repos::usage::UsageRepository::new(db.pool.clone());
    repo.record(
        alnair_router::db::repos::usage::NewUsageRecord {
            requested_model: "gpt-4o".to_string(),
            attempt: 1,
            status: "ok".to_string(),
            prompt_tokens: 100,
            completion_tokens: 50,
            latency_ms: 10,
            ..Default::default()
        }
        .with_savings(alnair_router::token_saver::SavingsTotals {
            saved_rtk_tokens: 25,
            saved_cost_usd: 0.001,
            ..Default::default()
        }),
    )
    .await
    .expect("record");

    let response = raw_request_with_auth(&app, "GET", "/api/usage/summary", None, None).await;
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(body["savings"]["saved_rtk_tokens"], serde_json::json!(25));
    assert_eq!(body["savings"]["requests"], serde_json::json!(1));
    // The pre-existing top-level cost fields must survive the added block.
    assert_eq!(body["requests"], serde_json::json!(1));
    assert!(body["cost_usd"].is_number());
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

/// Responds with a plain JSON 404 to everything, which is what a wrong
/// base_url looks like when the host answers but no route matches.
async fn spawn_plain_404_upstream() -> String {
    let router = axum::Router::new().fallback(|| async {
        (
            StatusCode::NOT_FOUND,
            axum::Json(serde_json::json!({ "error_msg": "404 Route Not Found" })),
        )
    });

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
async fn connection_test_rejects_a_base_url_where_every_route_404s() {
    let (app, _db) = app(false).await;
    let base_url = spawn_plain_404_upstream().await;
    let id = create_probe_connection(&app, &base_url).await;

    let (status, body) = json_request(
        &app,
        "POST",
        &format!("/api/connections/{id}/test"),
        serde_json::json!({}),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_GATEWAY, "unexpected body: {body}");
    let message = body["error"]["message"].as_str().unwrap_or_default();
    assert!(message.contains("404"), "unexpected message: {message}");
}

/// Serves an OpenAI-compatible upstream that exposes only the chat route:
/// `/models` is absent (404) but `/chat/completions` exists (401 without a
/// usable key), which is how CodeBuddy Intl behaves.
async fn spawn_chat_only_upstream() -> String {
    let router = axum::Router::new().route(
        "/chat/completions",
        axum::routing::post(|| async {
            (
                StatusCode::UNAUTHORIZED,
                axum::Json(serde_json::json!({ "error": "invalid api key" })),
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

    format!("http://{address}")
}

#[tokio::test]
async fn connection_test_accepts_a_provider_without_a_models_endpoint() {
    let (app, _db) = app(false).await;
    let base_url = spawn_chat_only_upstream().await;
    let id = create_probe_connection(&app, &base_url).await;

    let (status, body) = json_request(
        &app,
        "POST",
        &format!("/api/connections/{id}/test"),
        serde_json::json!({}),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "unexpected body: {body}");
    assert_eq!(body["ok"], true);
    assert_eq!(body["enumerable"], false);
    assert_eq!(body["models_count"], 0);
    assert!(
        body["message"]
            .as_str()
            .unwrap_or_default()
            .contains("model list not available"),
        "unexpected body: {body}"
    );
}

#[tokio::test]
async fn connection_models_reports_a_non_enumerable_provider() {
    let (app, _db) = app(false).await;
    let base_url = spawn_chat_only_upstream().await;
    let id = create_probe_connection(&app, &base_url).await;

    let (status, body) = get(&app, &format!("/api/connections/{id}/models")).await;

    assert_eq!(status, StatusCode::OK, "unexpected body: {body}");
    assert_eq!(body["enumerable"], false);
    assert_eq!(body["models"].as_array().expect("models").len(), 0);
}

#[tokio::test]
async fn alias_test_skips_the_model_check_for_a_non_enumerable_provider() {
    let (app, _db) = app(false).await;
    let base_url = spawn_chat_only_upstream().await;
    let connection_id = create_probe_connection(&app, &base_url).await;
    let alias_id = create_alias_for(&app, &connection_id, "cb", Some("claude-sonnet-4")).await;

    let (status, body) = json_request(
        &app,
        "POST",
        &format!("/api/aliases/{alias_id}/test"),
        serde_json::json!({}),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "unexpected body: {body}");
    assert_eq!(body["ok"], true);
    assert_eq!(body["enumerable"], false);
    assert_eq!(body["model_available"], serde_json::Value::Null);
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

/// Serves an SSE completion that also reports token usage, so the playground
/// chat's terminal frame has something to report.
async fn spawn_usage_chat_upstream() -> String {
    let router = axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(|| async {
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/event-stream")],
                concat!(
                    "data: {\"choices\":[{\"delta\":{\"content\":\"pong\"}}]}\n\n",
                    "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                    "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":3,\"total_tokens\":15}}\n\n",
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

/// Extracts the JSON payload of every `data:` frame, dropping `[DONE]`.
fn sse_frames(body: &str) -> Vec<serde_json::Value> {
    body.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter(|payload| *payload != "[DONE]")
        .filter_map(|payload| serde_json::from_str(payload).ok())
        .collect()
}

#[tokio::test]
async fn playground_chat_streams_router_delta_usage_frames() {
    let (app, _db) = app(false).await;
    let base_url = spawn_usage_chat_upstream().await;
    let connection_id = create_probe_connection(&app, &base_url).await;
    create_alias_for(
        &app,
        &connection_id,
        "playchat",
        Some("deepseek-v4.1-flash"),
    )
    .await;

    let response = raw_request_with_auth(
        &app,
        "POST",
        "/api/playground/chat",
        Some(serde_json::json!({
            "model": "playchat",
            "messages": [{ "role": "user", "content": "hi" }]
        })),
        None,
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("text/event-stream")
    );

    let body = body_text(response).await;
    let frames = sse_frames(&body);

    // The routing decision opens the stream, so the dashboard can label the
    // answer with the tier that produced it before any token arrives.
    assert_eq!(frames[0]["type"], "router");
    assert_eq!(frames[0]["model"], "deepseek-v4.1-flash");
    assert_eq!(frames[0]["source"], "alias:playchat");
    assert_eq!(frames[0]["attempts"], 1);

    let text: String = frames
        .iter()
        .filter(|frame| frame["type"] == "delta")
        .filter_map(|frame| frame["text"].as_str())
        .collect();
    assert_eq!(text, "pong");

    let usage = frames
        .iter()
        .find(|frame| frame["type"] == "usage")
        .expect("usage frame");
    assert!(usage["prompt_tokens"].as_u64().is_some());
    assert!(usage["savings"].is_object());

    // The stream always terminates with the OpenAI sentinel.
    assert!(body.contains("data: [DONE]"));
}

#[tokio::test]
async fn playground_chat_rejects_a_configuration_the_settings_page_would_reject() {
    let (app, _db) = app(false).await;

    // Terse and caveman both write a system directive, so the settings page
    // rejects them together. The playground must not demonstrate a config that
    // cannot be saved.
    let (status, body) = json_request(
        &app,
        "POST",
        "/api/playground/chat",
        serde_json::json!({
            "model": "anything",
            "messages": [{ "role": "user", "content": "hi" }],
            "overrides": { "terse_enabled": true, "caveman_enabled": true }
        }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "unexpected body: {body}");
    assert_eq!(body["error"]["type"], "invalid_request_error");
}

#[tokio::test]
async fn playground_chat_unknown_model_reports_an_error_frame() {
    let (app, _db) = app(false).await;

    // Resolution happens before the stream opens, so an unknown reference is a
    // plain HTTP error rather than an in-band frame.
    let (status, body) = json_request(
        &app,
        "POST",
        "/api/playground/chat",
        serde_json::json!({
            "model": "nope/nothing",
            "messages": [{ "role": "user", "content": "hi" }]
        }),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "unexpected body: {body}");
    assert_eq!(body["error"]["type"], "not_found_error");
}

#[tokio::test]
async fn playground_chat_requires_the_admin_guard() {
    let (app, _db) = app_with_admin_token("secret-token").await;

    let (status, body) = json_request(
        &app,
        "POST",
        "/api/playground/chat",
        serde_json::json!({
            "model": "anything",
            "messages": [{ "role": "user", "content": "hi" }]
        }),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED, "unexpected body: {body}");

    // The same request with the bearer passes the guard and then fails on the
    // unknown model, proving the credential was what allowed it through.
    let (status, body) = json_request_with_auth(
        &app,
        "POST",
        "/api/playground/chat",
        serde_json::json!({
            "model": "anything",
            "messages": [{ "role": "user", "content": "hi" }]
        }),
        Some("secret-token"),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "unexpected body: {body}");
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
            ..Default::default()
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

    let (status, body) = get(&app, "/api/usage/models").await;
    assert_eq!(status, StatusCode::OK, "unexpected body: {body}");
    let rows = body.as_array().expect("rows");
    assert_eq!(rows.len(), 2, "one row per requested model: {body}");
    assert_eq!(rows[0]["model"], "kr/claude");
    assert_eq!(rows[0]["requests"], 1);
    assert_eq!(rows[0]["error_requests"], 0);
    assert_eq!(rows[0]["cost_usd"], 0.5);

    let (status, body) = get(&app, "/api/usage/models?provider=anthropic-native").await;
    assert_eq!(status, StatusCode::OK);
    let rows = body.as_array().expect("rows");
    assert_eq!(
        rows.len(),
        1,
        "filters apply to the per-model rollup: {body}"
    );
    assert_eq!(rows[0]["model"], "kr/claude");
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
            ..Default::default()
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

/// The dashboard's trend chart reads `/api/usage/timeseries`, which buckets the
/// same filtered rows the table shows.
#[tokio::test]
async fn usage_timeseries_buckets_the_filtered_rows() {
    let (app, db) = app(false).await;
    let repo = UsageRepository::new(db.pool.clone());

    for (model, cost) in [("oa/gpt-4o", 1.0), ("oa/gpt-4o", 2.0), ("kr/claude", 4.0)] {
        repo.record(NewUsageRecord {
            api_key_id: None,
            requested_model: model.to_string(),
            resolved_provider: Some("openai-compatible".to_string()),
            resolved_model: None,
            connection_name: Some("openai-main".to_string()),
            attempt: 1,
            status: "ok".to_string(),
            prompt_tokens: 1,
            completion_tokens: 1,
            cached_tokens: 0,
            reasoning_tokens: 0,
            cost_usd: cost,
            cost_input_usd: 0.0,
            cost_output_usd: 0.0,
            cost_reasoning_usd: 0.0,
            latency_ms: 5,
            ..Default::default()
        })
        .await
        .expect("record usage");
    }

    let (status, body) = get(&app, "/api/usage/timeseries?bucket=hour").await;
    assert_eq!(status, StatusCode::OK, "unexpected body: {body}");
    let rows = body.as_array().expect("rows");
    assert_eq!(rows.len(), 2, "one row per (bucket, model): {body}");

    // Rows come back ordered by bucket then model, so look them up by name.
    let openai = rows
        .iter()
        .find(|row| row["model"] == "oa/gpt-4o")
        .expect("openai row");
    assert_eq!(
        openai["requests"], 2,
        "both attempts share one bucket: {body}"
    );
    assert_eq!(openai["cost_usd"], 3.0);
    assert!(
        openai["bucket"].as_str().unwrap_or("").ends_with(":00:00Z"),
        "hour buckets are RFC 3339: {body}"
    );

    // The same filters the table uses narrow the series.
    let (status, body) = get(
        &app,
        "/api/usage/timeseries?model=claude&provider=openai-compatible",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "unexpected body: {body}");
    let rows = body.as_array().expect("rows");
    assert_eq!(rows.len(), 1, "only matching rows are bucketed: {body}");
    assert_eq!(rows[0]["model"], "kr/claude");

    // An unknown bucket is a client error, not a silent fallback.
    let (status, _) = get(&app, "/api/usage/timeseries?bucket=week").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// The usage table sorts by whitelisted columns; anything else is rejected
/// before it can reach the SQL.
#[tokio::test]
async fn usage_list_sorts_and_rejects_unknown_keys() {
    let (app, db) = app(false).await;
    let repo = UsageRepository::new(db.pool.clone());

    for (model, cost) in [("cheap", 1.0), ("pricey", 9.0)] {
        repo.record(NewUsageRecord {
            api_key_id: None,
            requested_model: model.to_string(),
            resolved_provider: None,
            resolved_model: None,
            connection_name: None,
            attempt: 1,
            status: "ok".to_string(),
            prompt_tokens: 1,
            completion_tokens: 1,
            cached_tokens: 0,
            reasoning_tokens: 0,
            cost_usd: cost,
            cost_input_usd: 0.0,
            cost_output_usd: 0.0,
            cost_reasoning_usd: 0.0,
            latency_ms: 5,
            ..Default::default()
        })
        .await
        .expect("record usage");
    }

    let (status, body) = get(&app, "/api/usage?sort=cost&order=asc").await;
    assert_eq!(status, StatusCode::OK, "unexpected body: {body}");
    assert_eq!(body[0]["requested_model"], "cheap");
    assert_eq!(body[1]["requested_model"], "pricey");

    let (status, body) = get(&app, "/api/usage?sort=model&order=desc").await;
    assert_eq!(status, StatusCode::OK, "unexpected body: {body}");
    assert_eq!(body[0]["requested_model"], "pricey");

    let (status, _) = get(&app, "/api/usage?sort=cost%3B%20DROP%20TABLE").await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "unknown sort keys are refused"
    );

    let (status, _) = get(&app, "/api/usage?sort=cost&order=sideways").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// `until` bounds the window at both ends, which the month selector relies on.
#[tokio::test]
async fn usage_until_bounds_the_window() {
    let (app, db) = app(false).await;
    UsageRepository::new(db.pool.clone())
        .record(NewUsageRecord {
            api_key_id: None,
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
            cost_usd: 1.0,
            cost_input_usd: 0.0,
            cost_output_usd: 0.0,
            cost_reasoning_usd: 0.0,
            latency_ms: 5,
            ..Default::default()
        })
        .await
        .expect("record usage");

    let (status, body) = get(&app, "/api/usage?until=2000-01-01T00:00:00Z").await;
    assert_eq!(status, StatusCode::OK, "unexpected body: {body}");
    assert_eq!(body.as_array().map(Vec::len), Some(0));

    let (status, body) = get(&app, "/api/usage/summary?until=2000-01-01T00:00:00Z").await;
    assert_eq!(status, StatusCode::OK, "unexpected body: {body}");
    assert_eq!(body["requests"], 0);
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

    let created = ApiKeyRepository::new(db.pool.clone(), test_cipher())
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
async fn settings_toggle_store_key_secrets_hot() {
    let (app, db) = app(true).await;

    let (status, body) = get(&app, "/api/settings").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["server"]["store_key_secrets"], true, "defaults to on");

    // Turning it off must apply to the very next key minted, with no restart.
    let (status, _) = json_request(
        &app,
        "PATCH",
        "/api/settings",
        serde_json::json!({ "store_key_secrets": false }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, created) = json_request(
        &app,
        "POST",
        "/api/keys",
        serde_json::json!({ "name": "hash-only" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["key"]["id"].as_str().expect("id");

    let (status, _) = get(&app, &format!("/api/keys/{id}/secret")).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "nothing was stored to reveal"
    );

    // Flipping it back on only affects keys minted from then on.
    let (status, _) = json_request(
        &app,
        "PATCH",
        "/api/settings",
        serde_json::json!({ "store_key_secrets": true }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let secret = mint_key(&db, "revealable").await;
    let id = ApiKeyRepository::new(db.pool.clone(), test_cipher())
        .find_by_secret(&secret)
        .await
        .expect("lookup")
        .expect("key")
        .id;

    let (status, body) = get(&app, &format!("/api/keys/{id}/secret")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["secret"], secret);
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

    let repository = ApiKeyRepository::new(db.pool.clone(), test_cipher());
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

async fn get_with_origin(app: &axum::Router, path: &str, origin: &str) -> axum::response::Response {
    let request = Request::builder()
        .method("GET")
        .uri(path)
        .header(header::ORIGIN, origin)
        .body(Body::empty())
        .unwrap();
    app.clone().oneshot(request).await.expect("request")
}

fn allowed_origin(response: &axum::response::Response) -> Option<&str> {
    response
        .headers()
        .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        .and_then(|value| value.to_str().ok())
}

#[tokio::test]
async fn cors_origins_gate_cross_origin_access() {
    // Empty list: the same-origin dashboard needs no headers, and other
    // origins get none.
    let (app, _db) = app(false).await;
    let response = get_with_origin(&app, "/api/health", "https://app.example.com").await;
    assert_eq!(allowed_origin(&response), None);

    // An explicit allowlist answers only its own origins.
    let mut config = RouterConfig::default();
    config.server.cors_origins = vec!["https://app.example.com".to_string()];
    let (app, _db) = app_with_config(config).await;
    let response = get_with_origin(&app, "/api/health", "https://app.example.com").await;
    assert_eq!(allowed_origin(&response), Some("https://app.example.com"));
    let response = get_with_origin(&app, "/api/health", "https://other.example.com").await;
    assert_eq!(allowed_origin(&response), None);

    // `*` allows any origin.
    let mut config = RouterConfig::default();
    config.server.cors_origins = vec!["*".to_string()];
    let (app, _db) = app_with_config(config).await;
    let response = get_with_origin(&app, "/api/health", "https://any.example.com").await;
    assert_eq!(allowed_origin(&response), Some("*"));
}

#[tokio::test]
async fn cors_preflight_is_answered_for_allowed_origins() {
    let mut config = RouterConfig::default();
    config.server.cors_origins = vec!["https://app.example.com".to_string()];
    let (app, _db) = app_with_config(config).await;

    let request = Request::builder()
        .method("OPTIONS")
        .uri("/v1/chat/completions")
        .header(header::ORIGIN, "https://app.example.com")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
        .header(
            header::ACCESS_CONTROL_REQUEST_HEADERS,
            "authorization, content-type",
        )
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.expect("request");

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(allowed_origin(&response), Some("https://app.example.com"));
    assert!(
        response
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_METHODS)
    );

    // A disallowed origin is answered without CORS headers.
    let request = Request::builder()
        .method("OPTIONS")
        .uri("/v1/chat/completions")
        .header(header::ORIGIN, "https://other.example.com")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.expect("request");
    assert_eq!(allowed_origin(&response), None);
}

#[tokio::test]
async fn settings_apply_cors_origins_without_a_restart() {
    let (app, _db) = app(false).await;

    let response = get_with_origin(&app, "/api/health", "https://app.example.com").await;
    assert_eq!(allowed_origin(&response), None);

    let (status, body) = json_request(
        &app,
        "PATCH",
        "/api/settings",
        serde_json::json!({ "cors_origins": ["https://app.example.com"] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["server"]["cors_origins"],
        serde_json::json!(["https://app.example.com"])
    );

    let response = get_with_origin(&app, "/api/health", "https://app.example.com").await;
    assert_eq!(allowed_origin(&response), Some("https://app.example.com"));
    let response = get_with_origin(&app, "/api/health", "https://other.example.com").await;
    assert_eq!(allowed_origin(&response), None);

    // `*` opens it up.
    let (status, _) = json_request(
        &app,
        "PATCH",
        "/api/settings",
        serde_json::json!({ "cors_origins": ["*"] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let response = get_with_origin(&app, "/api/health", "https://any.example.com").await;
    assert_eq!(allowed_origin(&response), Some("*"));

    // Invalid entries are rejected before they are stored.
    let (status, _) = json_request(
        &app,
        "PATCH",
        "/api/settings",
        serde_json::json!({ "cors_origins": ["not a header"] }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Builds a state whose setup code is ready, plus the app over it.
async fn app_with_setup_code() -> (axum::Router, AppState, String) {
    let db = Db::connect_in_memory().await.expect("db");
    let mut config = RouterConfig::default();
    config.secrets.key = Some(TEST_SECRET.to_string());

    let state = AppState::new(config, db).expect("state");
    let code = state
        .init_setup_code()
        .await
        .expect("setup code")
        .expect("no password yet");
    (build_router(state.clone()), state, code)
}

#[tokio::test]
async fn password_auth_gates_the_admin_api() {
    let (app, _state, code) = app_with_setup_code().await;

    let (status, body) = get(&app, "/api/auth/status").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["setup_required"], true);
    assert_eq!(body["password_set"], false);

    // The wrong setup code never creates a password.
    let (status, _) = json_request(
        &app,
        "POST",
        "/api/auth/setup",
        serde_json::json!({ "setup_code": "0000-0000-0000-0000", "password": "secret123" }),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, session) = json_request(
        &app,
        "POST",
        "/api/auth/setup",
        serde_json::json!({ "setup_code": code, "password": "secret123" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "setup failed: {session}");
    let access = session["access_token"]
        .as_str()
        .expect("access")
        .to_string();
    let refresh = session["refresh_token"]
        .as_str()
        .expect("refresh")
        .to_string();

    // A password now exists, so even loopback needs a credential.
    let (status, _) = get(&app, "/api/connections").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = get_with_auth(&app, "/api/connections", Some(&access)).await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = get_with_auth(&app, "/api/auth/status", Some(&access)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["authenticated"], true);
    assert_eq!(body["setup_required"], false);

    // Refreshing rotates the pair; replaying the old token kills the family.
    let (status, rotated) = json_request(
        &app,
        "POST",
        "/api/auth/refresh",
        serde_json::json!({ "refresh_token": refresh }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "refresh failed: {rotated}");
    let rotated_access = rotated["access_token"]
        .as_str()
        .expect("access")
        .to_string();

    let (status, _) = json_request(
        &app,
        "POST",
        "/api/auth/refresh",
        serde_json::json!({ "refresh_token": refresh }),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "reuse must be rejected");
    let (status, _) = get_with_auth(&app, "/api/connections", Some(&rotated_access)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "family revoked");

    // Login and logout round-trip.
    let (status, session) = json_request(
        &app,
        "POST",
        "/api/auth/login",
        serde_json::json!({ "password": "secret123" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let access = session["access_token"]
        .as_str()
        .expect("access")
        .to_string();

    let (status, _) = json_request(&app, "POST", "/api/auth/logout", serde_json::json!({})).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "logout needs a session");

    let response =
        raw_request_with_auth(&app, "POST", "/api/auth/logout", None, Some(&access)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let (status, _) = get_with_auth(&app, "/api/connections", Some(&access)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn lan_access_requires_credentials_even_without_a_password() {
    let (app, _db) = app_with_config({
        let mut config = RouterConfig::default();
        config.server.lan_access = true;
        config
    })
    .await;

    let (status, body) = get(&app, "/api/connections").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["type"], "authentication_error");

    // Public surfaces still work, so the setup screen can load.
    let (status, _) = get(&app, "/api/auth/status").await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn settings_toggle_lan_access_hot() {
    let (app, _db) = app(false).await;

    // Loopback with no password keeps the localhost posture.
    let (status, _) = get(&app, "/api/connections").await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = json_request(
        &app,
        "PATCH",
        "/api/settings",
        serde_json::json!({ "lan_access": true }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["server"]["lan_access"], true);

    // Exposing the network closes the admin API until a password exists.
    let (status, _) = get(&app, "/api/connections").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// Startup applies the stored dashboard overrides before any loop exists. A
/// `Notify` permit handed out then is kept for the next waiter, so notifying
/// would rebind the listener (logging a second "listening" line) the moment
/// serving begins, for a change that was already in effect.
#[tokio::test]
async fn startup_overrides_do_not_wake_the_loops() {
    let db = Db::connect_in_memory().await.expect("db");
    let mut config = RouterConfig::default();
    config.secrets.key = Some(TEST_SECRET.to_string());
    let state = AppState::new(config, db).expect("state");

    let startup = SettingsOverrides {
        lan_access: Some(true),
        pricing_sync_interval_secs: Some(3600),
        ..Default::default()
    };
    state.apply_overrides(&startup).await.expect("apply");

    let rebound = tokio::time::timeout(
        std::time::Duration::from_millis(50),
        state.rebind.notified(),
    )
    .await;
    assert!(
        rebound.is_err(),
        "startup overrides must not queue a rebind"
    );
    let synced = tokio::time::timeout(
        std::time::Duration::from_millis(50),
        state.pricing_sync_trigger.notified(),
    )
    .await;
    assert!(synced.is_err(), "startup overrides must not queue a sync");

    // Once the loops run, the same settings wake them.
    state.start_loops();
    let saved = SettingsOverrides {
        lan_access: Some(false),
        pricing_sync_interval_secs: Some(7200),
        ..Default::default()
    };
    state.apply_overrides(&saved).await.expect("apply");

    let rebound = tokio::time::timeout(
        std::time::Duration::from_millis(50),
        state.rebind.notified(),
    )
    .await;
    assert!(rebound.is_ok(), "a dashboard save must rebind the listener");
    let synced = tokio::time::timeout(
        std::time::Duration::from_millis(50),
        state.pricing_sync_trigger.notified(),
    )
    .await;
    assert!(
        synced.is_ok(),
        "a dashboard save must wake the pricing sync"
    );
}

#[tokio::test]
async fn provider_presets_fill_connection_defaults() {
    let (app, _db) = app(false).await;

    let (status, body) = get(&app, "/api/providers").await;
    assert_eq!(status, StatusCode::OK);
    let presets = body["data"].as_array().expect("presets");
    assert!(presets.len() >= 50, "expected a real catalog");
    assert!(presets.iter().any(|preset| preset["id"] == "openai"));
    assert!(
        presets.iter().all(|preset| preset["configured"] == 0),
        "nothing configured yet"
    );

    // Every preset carries the tier the picker groups it under.
    assert!(
        presets.iter().all(|preset| preset["category"].is_string()),
        "presets need a category"
    );
    let nvidia = presets
        .iter()
        .find(|preset| preset["id"] == "nvidia")
        .expect("nvidia preset");
    assert_eq!(nvidia["category"], "free_tier");
    let lmstudio = presets
        .iter()
        .find(|preset| preset["id"] == "lmstudio")
        .expect("lmstudio preset");
    assert_eq!(lmstudio["category"], "local");

    // Creating from a preset fills the endpoint and wire family.
    let (status, body) = json_request(
        &app,
        "POST",
        "/api/connections",
        serde_json::json!({ "name": "openai-main", "provider_id": "openai" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "create failed: {body}");
    assert_eq!(body["provider_type"], "openai-compatible");
    assert_eq!(body["base_url"], "https://api.openai.com/v1");
    assert_eq!(body["provider_id"], "openai");

    // A newly added preset fills itself the same way.
    let (status, body) = json_request(
        &app,
        "POST",
        "/api/connections",
        serde_json::json!({ "name": "command-code", "provider_id": "commandcode" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "create failed: {body}");
    assert_eq!(body["provider_type"], "command-code");
    assert_eq!(body["base_url"], "https://api.commandcode.ai");

    // A base URL that still carries a preset placeholder is refused.
    let (status, _) = json_request(
        &app,
        "POST",
        "/api/connections",
        serde_json::json!({ "name": "azure", "provider_id": "azure-openai" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // The catalog reports the new connection.
    let (_, body) = get(&app, "/api/providers").await;
    let openai = body["data"]
        .as_array()
        .expect("presets")
        .iter()
        .find(|preset| preset["id"] == "openai")
        .expect("openai preset");
    assert_eq!(openai["configured"], 1);

    // Unknown presets are rejected.
    let (status, _) = json_request(
        &app,
        "POST",
        "/api/connections",
        serde_json::json!({ "name": "nope", "provider_id": "does-not-exist" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // An explicit wire family still has to be supported.
    let (status, _) = json_request(
        &app,
        "POST",
        "/api/connections",
        serde_json::json!({
            "name": "bad",
            "provider_id": "openai",
            "provider_type": "not-a-type"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn provider_presets_keep_user_overrides() {
    let (app, _db) = app(false).await;

    let (status, body) = json_request(
        &app,
        "POST",
        "/api/connections",
        serde_json::json!({
            "name": "custom-openai",
            "provider_id": "openai",
            "base_url": "https://proxy.example.com/v1",
            "provider_type": "anthropic-native",
            "custom_headers": { "X-Org": "acme" }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["base_url"], "https://proxy.example.com/v1");
    assert_eq!(body["provider_type"], "anthropic-native");

    let headers: serde_json::Value = serde_json::from_str(
        body["custom_headers"]
            .as_str()
            .expect("custom_headers string"),
    )
    .expect("headers json");
    assert_eq!(headers["X-Org"], "acme");
}

#[tokio::test]
async fn connection_accounts_round_trip_and_invalidate() {
    let (app, _db) = app(false).await;

    let (_, connection) = json_request(
        &app,
        "POST",
        "/api/connections",
        serde_json::json!({
            "name": "openai-main",
            "provider_id": "openai",
            "api_key": "sk-primary"
        }),
    )
    .await;
    let connection_id = connection["id"].as_str().expect("connection id");
    assert_eq!(connection["account_count"], 0);

    let (status, account) = json_request(
        &app,
        "POST",
        &format!("/api/connections/{connection_id}/accounts"),
        serde_json::json!({ "label": "backup", "api_key": "sk-backup" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "create failed: {account}");
    assert!(
        account.get("api_key").is_none(),
        "the extra key must never be returned: {account}"
    );
    let account_id = account["id"].as_str().expect("account id");

    // The connection list reports the enabled account count.
    let (status, list) = get(&app, "/api/connections").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list[0]["account_count"], 1);

    // Duplicate labels are rejected.
    let (status, _) = json_request(
        &app,
        "POST",
        &format!("/api/connections/{connection_id}/accounts"),
        serde_json::json!({ "label": "backup", "api_key": "sk-other" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Disabling an account drops it from the rotation count.
    let (status, _) = json_request(
        &app,
        "PATCH",
        &format!("/api/connections/{connection_id}/accounts/{account_id}"),
        serde_json::json!({ "enabled": false }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, list) = get(&app, "/api/connections").await;
    assert_eq!(list[0]["account_count"], 0);

    let response = raw_request_with_auth(
        &app,
        "DELETE",
        &format!("/api/connections/{connection_id}/accounts/{account_id}"),
        None,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    // Accounts need an existing connection.
    let (status, _) = json_request(
        &app,
        "POST",
        "/api/connections/does-not-exist/accounts",
        serde_json::json!({ "label": "x", "api_key": "sk-y" }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn oauth_presets_ship_endpoints_but_no_borrowed_identity() {
    let (app, _db) = app(false).await;

    let (status, body) = get(&app, "/api/oauth/presets").await;
    assert_eq!(status, StatusCode::OK);

    let presets = body["data"].as_array().expect("preset list");
    assert!(
        presets.iter().any(|preset| preset["id"] == "gitlab-duo"),
        "the GitLab preset is the reference flow: {body}"
    );
    assert!(
        presets.iter().any(|preset| preset["id"] == "generic"),
        "the generic entry must always be offered: {body}"
    );

    // A preset may carry endpoints and scopes, but never a client id or secret:
    // shipping one would mean impersonating a first-party application.
    for preset in presets {
        assert!(
            preset.get("client_id").is_none(),
            "a preset carried a client id: {preset}"
        );
        assert!(
            preset.get("client_secret").is_none(),
            "a preset carried a client secret: {preset}"
        );
    }
}

#[tokio::test]
async fn start_oauth_login_builds_a_pkce_authorize_url() {
    let (app, _db) = app(false).await;

    let (_, connection) = json_request(
        &app,
        "POST",
        "/api/connections",
        serde_json::json!({
            "name": "gitlab-duo",
            "provider_type": "anthropic-native",
            "base_url": "https://gitlab.com/api/v4",
            "auth_style": "bearer"
        }),
    )
    .await;
    let connection_id = connection["id"].as_str().expect("connection id");

    let (status, body) = json_request(
        &app,
        "POST",
        "/api/oauth/logins",
        serde_json::json!({
            "connection_id": connection_id,
            "label": "work",
            "provider_key": "gitlab-duo",
            "client_id": "client-abc",
            "client_secret": "secret-abc",
            "authorize_url": "https://gitlab.com/oauth/authorize",
            "token_url": "https://gitlab.com/oauth/token",
            "user_info_url": "https://gitlab.com/api/v4/user",
            "scopes": "api read_user"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "start failed: {body}");

    let login_id = body["login_id"].as_str().expect("login id");
    let authorize = body["authorize_url"].as_str().expect("authorize url");

    // The URL the browser is sent to must be a PKCE authorization request.
    assert!(
        authorize.starts_with("https://gitlab.com/oauth/authorize?"),
        "{authorize}"
    );
    assert!(authorize.contains("client_id=client-abc"), "{authorize}");
    assert!(authorize.contains("response_type=code"), "{authorize}");
    assert!(
        authorize.contains("code_challenge_method=S256"),
        "{authorize}"
    );
    assert!(authorize.contains("code_challenge="), "{authorize}");
    // The operator has to register exactly this redirect URI.
    assert_eq!(
        body["redirect_uri"],
        "http://127.0.0.1:7878/api/oauth/callback"
    );

    // The login starts pending and is readable by id.
    let (status, view) = get(&app, &format!("/api/oauth/logins/{login_id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["status"], "pending");
    assert_eq!(view["label"], "work");

    // Cancelling forgets it.
    let response = raw_request_with_auth(
        &app,
        "DELETE",
        &format!("/api/oauth/logins/{login_id}"),
        None,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let (status, _) = get(&app, &format!("/api/oauth/logins/{login_id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_browser_login_needs_an_authorize_url() {
    let (app, _db) = app(false).await;
    let (_, connection) = json_request(
        &app,
        "POST",
        "/api/connections",
        serde_json::json!({
            "name": "device-only",
            "provider_type": "openai-compatible",
            "base_url": "https://example.invalid/v1"
        }),
    )
    .await;
    let connection_id = connection["id"].as_str().expect("connection id");

    // A device-only provider has no authorize URL; the browser flow must say so
    // rather than mint an unusable authorize link.
    let (status, body) = json_request(
        &app,
        "POST",
        "/api/oauth/logins",
        serde_json::json!({
            "connection_id": connection_id,
            "label": "work",
            "client_id": "client-abc",
            "authorize_url": "",
            "token_url": "https://example.invalid/token",
            "device_code_url": "https://example.invalid/device"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("device-code"),
        "{body}"
    );
}

#[tokio::test]
async fn an_oauth_login_rejects_a_bad_client_and_missing_connection() {
    let (app, _db) = app(false).await;

    // No client id: the operator must bring their own registration.
    let (status, body) = json_request(
        &app,
        "POST",
        "/api/oauth/logins",
        serde_json::json!({
            "connection_id": "any",
            "label": "work",
            "client_id": "  ",
            "authorize_url": "https://example.invalid/authorize",
            "token_url": "https://example.invalid/token"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("client_id"),
        "{body}"
    );

    // A well-formed client but no such connection.
    let (status, _) = json_request(
        &app,
        "POST",
        "/api/oauth/logins",
        serde_json::json!({
            "connection_id": "does-not-exist",
            "label": "work",
            "client_id": "client-abc",
            "authorize_url": "https://example.invalid/authorize",
            "token_url": "https://example.invalid/token"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn oauth_accounts_are_listed_renamed_and_deleted() {
    let (app, db) = app(false).await;

    let (_, connection) = json_request(
        &app,
        "POST",
        "/api/connections",
        serde_json::json!({
            "name": "gitlab-duo",
            "provider_type": "anthropic-native",
            "base_url": "https://gitlab.com/api/v4",
            "auth_style": "bearer"
        }),
    )
    .await;
    let connection_id = connection["id"].as_str().expect("connection id");

    // Seed one account the way a completed login would.
    let account = alnair_router::db::repos::oauth_accounts::OAuthAccountRepository::new(
        db.pool.clone(),
        test_cipher(),
    )
    .create(
        connection_id,
        alnair_router::db::repos::oauth_accounts::CreateOAuthAccount {
            label: "work".to_string(),
            provider_key: "gitlab-duo".to_string(),
            credential: alnair_router::oauth::OAuthCredential {
                access_token: "access-1".to_string(),
                refresh_token: Some("refresh-1".to_string()),
                expires_at: None,
                token_type: Some("Bearer".to_string()),
                scope: None,
                endpoints: alnair_router::oauth::EndpointConfig {
                    client_id: "client-abc".to_string(),
                    client_secret: Some("secret-abc".to_string()),
                    authorize_url: "https://gitlab.com/oauth/authorize".to_string(),
                    token_url: "https://gitlab.com/oauth/token".to_string(),
                    device_code_url: None,
                    user_info_url: None,
                    scopes: "api".to_string(),
                },
                account: None,
            },
            enabled: true,
        },
    )
    .await
    .expect("seed account");

    let (status, list) = get(
        &app,
        &format!("/api/connections/{connection_id}/oauth-accounts"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let list = list.as_array().expect("account list");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["label"], "work");
    assert_eq!(list[0]["provider_key"], "gitlab-duo");

    // The credential is encrypted at rest and must never be serialized out.
    assert!(
        list[0].get("credential").is_none(),
        "the credential must never leave the router: {}",
        list[0]
    );
    assert!(
        !list[0].to_string().contains("access-1"),
        "the access token leaked into the response: {}",
        list[0]
    );
    assert!(
        !list[0].to_string().contains("secret-abc"),
        "the client secret leaked into the response: {}",
        list[0]
    );

    // Renaming is allowed; the credential is not replaceable by a PATCH.
    let (status, updated) = json_request(
        &app,
        "PATCH",
        &format!(
            "/api/connections/{connection_id}/oauth-accounts/{}",
            account.id
        ),
        serde_json::json!({ "label": "personal", "enabled": false }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["label"], "personal");
    assert_eq!(updated["enabled"], 0);

    let response = raw_request_with_auth(
        &app,
        "DELETE",
        &format!(
            "/api/connections/{connection_id}/oauth-accounts/{}",
            account.id
        ),
        None,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let (_, list) = get(
        &app,
        &format!("/api/connections/{connection_id}/oauth-accounts"),
    )
    .await;
    assert!(list.as_array().expect("account list").is_empty());
}

#[tokio::test]
async fn the_oauth_callback_is_public_and_guarded_by_the_state_nonce() {
    let (app, _db) = app(true).await;

    // A stray callback with no state resolves nothing. It is reachable without a
    // client key because a browser redirect cannot carry one.
    let response = raw_request_with_auth(
        &app,
        "GET",
        "/api/oauth/callback?code=abc&state=not-a-real-nonce",
        None,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_text(response).await;
    assert!(body.contains("no longer valid"), "{body}");

    // An empty state must not match a device login, which carries none.
    let response =
        raw_request_with_auth(&app, "GET", "/api/oauth/callback?state=", None, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_text(response).await;
    assert!(!body.contains("Connected"), "{body}");
}

#[tokio::test]
async fn reveal_key_returns_the_secret() {
    let (app, db) = app(true).await;
    let secret = mint_key(&db, "revealable").await;
    let id = ApiKeyRepository::new(db.pool.clone(), test_cipher())
        .find_by_secret(&secret)
        .await
        .expect("lookup")
        .expect("key")
        .id;

    let (status, body) = get(&app, &format!("/api/keys/{id}/secret")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["secret"], secret);
}

#[tokio::test]
async fn reveal_key_404s_when_no_secret_was_stored() {
    let (app, db) = app(true).await;
    let created = ApiKeyRepository::new(db.pool.clone(), test_cipher())
        .storing_secrets(false)
        .create(CreateApiKey {
            name: "hash-only".to_string(),
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

    let (status, _) = get(&app, &format!("/api/keys/{}/secret", created.key.id)).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn reveal_key_404s_for_an_unknown_key() {
    let (app, _db) = app(true).await;

    let (status, _) = get(&app, "/api/keys/does-not-exist/secret").await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn reveal_key_requires_the_admin_token() {
    let (app, db) = app_with_admin_token("s3cret").await;
    let secret = mint_key(&db, "guarded").await;
    let id = ApiKeyRepository::new(db.pool.clone(), test_cipher())
        .find_by_secret(&secret)
        .await
        .expect("lookup")
        .expect("key")
        .id;

    let (status, _) = get(&app, &format!("/api/keys/{id}/secret")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, body) =
        get_with_auth(&app, &format!("/api/keys/{id}/secret"), Some("s3cret")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["secret"], secret);
}

#[tokio::test]
async fn rotate_key_mints_a_new_secret_and_invalidates_the_old_one() {
    let (app, db) = app(true).await;
    let secret = mint_key(&db, "rotating").await;
    let id = ApiKeyRepository::new(db.pool.clone(), test_cipher())
        .find_by_secret(&secret)
        .await
        .expect("lookup")
        .expect("key")
        .id;

    let response =
        raw_request_with_auth(&app, "POST", &format!("/api/keys/{id}/rotate"), None, None).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = body_json(response).await;
    let rotated = body["secret"].as_str().expect("secret").to_string();
    assert_ne!(rotated, secret);

    // The new secret authenticates; the old one is rejected.
    let (status, _) = get_with_auth(&app, "/v1/models", Some(&rotated)).await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = get_with_auth(&app, "/v1/models", Some(&secret)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // And the dashboard can reveal the replacement.
    let (status, body) = get(&app, &format!("/api/keys/{id}/secret")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["secret"], rotated);
}

#[tokio::test]
async fn rotate_key_404s_for_an_unknown_key() {
    let (app, _db) = app(true).await;

    let response =
        raw_request_with_auth(&app, "POST", "/api/keys/does-not-exist/rotate", None, None).await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn rotate_key_requires_the_admin_token() {
    let (app, db) = app_with_admin_token("s3cret").await;
    let secret = mint_key(&db, "guarded").await;
    let id = ApiKeyRepository::new(db.pool.clone(), test_cipher())
        .find_by_secret(&secret)
        .await
        .expect("lookup")
        .expect("key")
        .id;

    let response =
        raw_request_with_auth(&app, "POST", &format!("/api/keys/{id}/rotate"), None, None).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = raw_request_with_auth(
        &app,
        "POST",
        &format!("/api/keys/{id}/rotate"),
        None,
        Some("s3cret"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn list_keys_never_exposes_stored_secrets() {
    let (app, db) = app(true).await;
    let secret = mint_key(&db, "listed").await;

    let (status, body) = get(&app, "/api/keys").await;

    assert_eq!(status, StatusCode::OK);
    let listed = &body[0];
    assert!(listed.get("secret_enc").is_none());
    assert!(listed.get("key_hash").is_none());
    assert_ne!(listed["prefix"], secret);
}

/// Serves `body` on any media path the proxy forwards to, echoing back the raw
/// request bytes so a test can assert what actually reached the upstream.
async fn spawn_media_upstream(body: &'static str) -> String {
    let router = axum::Router::new()
        .route(
            "/v1/{*rest}",
            axum::routing::post(move |request: Request<Body>| async move {
                let bytes = request
                    .into_body()
                    .collect()
                    .await
                    .map(|collected| collected.to_bytes())
                    .unwrap_or_default();
                axum::Json(serde_json::json!({
                    "body": body,
                    "echo": String::from_utf8_lossy(&bytes),
                }))
            })
            .get(move || async move { axum::Json(serde_json::json!({ "body": body })) }),
        )
        .route(
            "/v1/models",
            axum::routing::get(|| async {
                axum::Json(serde_json::json!({ "object": "list", "data": [] }))
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

/// Creates a connection plus an alias pointing at it, and returns the prefix.
async fn route_media_to(app: &axum::Router, base_url: &str, prefix: &str, model: &str) -> String {
    let (status, connection) = json_request(
        app,
        "POST",
        "/api/connections",
        serde_json::json!({
            "name": format!("media-{}", uuid_like()),
            "provider_type": "openai-compatible",
            "base_url": base_url,
            "api_key": "sk-media"
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
            "prefix": prefix,
            "connection_id": connection_id,
            "model_override": model
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "unexpected body: {alias}");

    prefix.to_string()
}

/// Creates an API key whose allowlist admits only `pattern`.
async fn mint_restricted_key(db: &Db, pattern: &str) -> String {
    ApiKeyRepository::new(db.pool.clone(), test_cipher())
        .create(CreateApiKey {
            name: "restricted-media".to_string(),
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
            allowed_models: Some(vec![pattern.to_string()]),
            expires_at: None,
        })
        .await
        .expect("mint key")
        .secret
}

/// The regression: a key restricted to one model could reach any connection
/// through the proxied endpoints, which skipped the allowlist entirely.
#[tokio::test]
async fn proxied_endpoints_enforce_the_model_allowlist() {
    let mut config = RouterConfig::default();
    config.server.require_api_key = true;
    let (app, db) = app_with_config(config).await;
    let base_url = spawn_media_upstream("{}").await;
    route_media_to(&app, &base_url, "openai", "whisper-1").await;
    let secret = mint_restricted_key(&db, "openai/*").await;
    let secret = secret.as_str();

    // Allowed: the reference matches the allowlist.
    let (status, body) = json_request_with_auth(
        &app,
        "POST",
        "/v1/embeddings",
        serde_json::json!({ "model": "openai/text-embedding-3-small", "input": "hi" }),
        Some(secret),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "unexpected body: {body}");

    // Denied: an unrelated reference must not slip through.
    let (status, body) = json_request_with_auth(
        &app,
        "POST",
        "/v1/embeddings",
        serde_json::json!({ "model": "anthropic/embed", "input": "hi" }),
        Some(secret),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "unexpected body: {body}");

    // Denied: search carries no model field, so `?model=`/body decides.
    let (status, body) = json_request_with_auth(
        &app,
        "POST",
        "/v1/search",
        serde_json::json!({ "query": "hi", "model": "anthropic/search" }),
        Some(secret),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "unexpected body: {body}");

    // Denied: the polling URL names the connection with `?model=`.
    let (status, body) =
        get_with_auth(&app, "/v1/videos/job-1?model=anthropic/video", Some(secret)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "unexpected body: {body}");
}

#[tokio::test]
async fn audio_transcription_reads_the_model_form_field() {
    let (app, _db) = app(false).await;
    let base_url = spawn_media_upstream("{}").await;
    route_media_to(&app, &base_url, "openai", "whisper-1").await;

    let boundary = "----alnair-boundary";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\nopenai/whisper-1\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.mp3\"\r\nContent-Type: audio/mpeg\r\n\r\ndata\r\n--{boundary}--\r\n"
    );

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/audio/transcriptions")
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .expect("request");

    assert_eq!(response.status(), StatusCode::OK);
    let payload = body_json(response).await;
    let echo = payload["echo"].as_str().unwrap_or_default();
    assert!(
        echo.contains("whisper-1"),
        "the multipart payload should reach the upstream: {payload}"
    );
    assert!(
        echo.contains("filename=\"a.mp3\""),
        "the file part should survive the proxy: {payload}"
    );
}

#[tokio::test]
async fn proxied_calls_record_usage_against_the_resolved_connection() {
    let (app, db) = app_with_config(RouterConfig::default()).await;
    let base_url = spawn_media_upstream("{}").await;
    route_media_to(&app, &base_url, "openai", "dall-e-3").await;

    let (status, _) = json_request(
        &app,
        "POST",
        "/v1/images/generations",
        serde_json::json!({ "model": "openai/dall-e-3", "prompt": "a cat" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let usage = UsageRepository::new(db.pool.clone())
        .list(
            10,
            0,
            &alnair_router::db::repos::usage::UsageFilter::new(None, None, None, None, None),
            alnair_router::db::repos::usage::Sort::default(),
        )
        .await
        .expect("usage rows");

    assert_eq!(usage.len(), 1, "one proxied call should record one row");
    let row = &usage[0];
    assert_eq!(row.requested_model, "openai/dall-e-3");
    assert_eq!(row.resolved_model.as_deref(), Some("dall-e-3"));
    assert_eq!(row.status, "ok");
    assert_eq!(row.attempt, 1);
}

#[tokio::test]
async fn proxied_failures_are_recorded_as_errors() {
    let (app, db) = app_with_config(RouterConfig::default()).await;
    // Nothing listens on this port, so the upstream call fails outright.
    route_media_to(&app, "http://127.0.0.1:1/v1", "dead", "embed").await;

    let (status, _) = json_request(
        &app,
        "POST",
        "/v1/embeddings",
        serde_json::json!({ "model": "dead/embed", "input": "hi" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);

    let usage = UsageRepository::new(db.pool.clone())
        .list(
            10,
            0,
            &alnair_router::db::repos::usage::UsageFilter::new(None, None, None, None, None),
            alnair_router::db::repos::usage::Sort::default(),
        )
        .await
        .expect("usage rows");

    assert_eq!(usage.len(), 1, "a failed proxy should still be attributed");
    assert_eq!(usage[0].status, "error");
}

/// The router strips its own prefix from a multipart `model` field, so the
/// upstream never sees `alias/model`.
#[tokio::test]
async fn transcription_rewrites_the_model_field_for_the_upstream() {
    use alnair_router::db::repos::usage::{Sort, UsageFilter, UsageRepository};

    let (app, db) = app_with_config(RouterConfig::default()).await;
    let base_url = spawn_media_upstream("{}").await;
    route_media_to(&app, &base_url, "openai", "whisper-1").await;

    let boundary = "----alnair-boundary";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\nopenai/whisper-1\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.mp3\"\r\nContent-Type: audio/mpeg\r\n\r\n\u{0}\u{1}\u{7f}\r\n--{boundary}--\r\n"
    );

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/audio/transcriptions")
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .expect("request");

    assert_eq!(response.status(), StatusCode::OK);
    let payload = body_json(response).await;
    let echo = payload["echo"].as_str().unwrap_or_default();

    assert!(
        echo.contains("whisper-1"),
        "the upstream should see the bare model id: {payload}"
    );
    assert!(
        !echo.contains("openai/whisper-1"),
        "the router prefix must not reach the upstream: {payload}"
    );
    // The file part's bytes survive the in-place edit.
    assert!(
        echo.contains("\u{0}\u{1}\u{7f}"),
        "the file part must be untouched: {payload}"
    );

    // The usage row keeps the reference the caller asked for.
    let usage = UsageRepository::new(db.pool.clone())
        .list(
            10,
            0,
            &UsageFilter::new(None, None, None, None, None),
            Sort::default(),
        )
        .await
        .expect("usage rows");
    assert_eq!(usage.len(), 1, "one transcription request, one row");
    assert_eq!(usage[0].requested_model, "openai/whisper-1");
}

#[tokio::test]
async fn newly_added_proxy_endpoints_are_routed() {
    let (app, _db) = app(false).await;
    let base_url = spawn_media_upstream("{}").await;
    let prefix = route_media_to(&app, &base_url, "openai", "whisper-1").await;

    // JSON endpoints accept a model reference and reach the upstream.
    for (path, body) in [
        (
            "/v1/moderations",
            serde_json::json!({ "model": format!("{prefix}/omni-moderation-latest"), "input": "hi" }),
        ),
        (
            "/v1/embeddings",
            serde_json::json!({ "model": format!("{prefix}/whisper-1"), "input": "hi" }),
        ),
    ] {
        let (status, payload) = json_request(&app, "POST", path, body).await;
        assert_eq!(status, StatusCode::OK, "{path} should be routed: {payload}");
    }

    // Multipart endpoints forward the body.
    for path in [
        "/v1/audio/translations",
        "/v1/images/edits",
        "/v1/images/variations",
    ] {
        let boundary = "----alnair-boundary";
        let body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\n{prefix}/whisper-1\r\n--{boundary}--\r\n"
        );

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
                    .header(
                        header::CONTENT_TYPE,
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .expect("request");

        assert_eq!(response.status(), StatusCode::OK, "{path} should be routed");
    }
}

#[tokio::test]
async fn retrieve_model_returns_one_model_object() {
    let (app, _db) = app(false).await;
    let base_url = spawn_media_upstream("{}").await;
    route_media_to(&app, &base_url, "openai", "whisper-1").await;

    let (status, body) = get(&app, "/v1/models/openai").await;
    assert_eq!(status, StatusCode::OK, "unexpected body: {body}");
    assert_eq!(body["id"], "openai");
    assert_eq!(body["object"], "model");
    assert_eq!(body["router_kind"], "alias");

    // An unknown reference is a 404, matching the resolve failure a completion
    // request would produce.
    let (status, body) = get(&app, "/v1/models/not-a-thing").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["type"], "not_found_error");
}
