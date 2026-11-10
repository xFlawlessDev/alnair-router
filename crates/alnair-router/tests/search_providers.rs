use alnair_router::{AppState, build_router, config::RouterConfig, db::Db};
use axum::{
    Json, Router,
    body::Body,
    http::{Request, StatusCode},
    routing::{get, post},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

async fn app() -> Router {
    let mut config = RouterConfig::default();
    config.server.require_api_key = false;
    config.secrets.key =
        Some("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f".to_string());
    build_router(AppState::new(config, Db::connect_in_memory().await.unwrap()).unwrap())
}

async fn request(app: &Router, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn serve(router: Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (base, task)
}

async fn tavily(app: &Router, base: &str) -> String {
    let (status, connection) = request(
        app,
        "POST",
        "/api/connections",
        json!({
            "provider_id":"tavily", "name":"tavily", "base_url":base, "api_key":"tvly-test"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{connection}");
    let id = connection["id"].as_str().unwrap().to_string();
    // The user's pre-existing connection predates preset routing defaults.
    let (status, result) = request(app, "PATCH", &format!("/api/connections/{id}"), json!({
        "search_tier":null, "web_fetch_path":null, "web_fetch_shape":null, "web_fetch_mode":"local"
    })).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    id
}

#[tokio::test]
async fn an_existing_search_provider_extracts_without_a_model_or_fetch_settings() {
    let (base, server) = serve(Router::new().route(
        "/extract",
        post(
            |headers: axum::http::HeaderMap, Json(body): Json<Value>| async move {
                assert_eq!(headers["authorization"], "Bearer tvly-test");
                assert_eq!(body["urls"], json!(["https://example.com"]));
                assert!(body.get("url").is_none());
                Json(json!({"results":[{"raw_content":"extracted page"}]}))
            },
        ),
    ))
    .await;
    let app = app().await;
    let id = tavily(&app, &base).await;
    for body in [
        json!({"url":"https://example.com"}),
        json!({"url":"https://example.com","model":"unknown-chat-model"}),
    ] {
        let (status, result) = request(&app, "POST", "/v1/web/fetch", body).await;
        assert_eq!(status, StatusCode::OK, "{result}");
        assert_eq!(result["content"]["text"], "extracted page");
    }
    request(
        &app,
        "PATCH",
        &format!("/api/connections/{id}"),
        json!({"enabled":false}),
    )
    .await;
    let (status, _) = request(
        &app,
        "POST",
        "/v1/web/fetch",
        json!({"url":"http://127.0.0.1/private"}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "disabled provider must leave the local SSRF guard in place"
    );
    server.abort();
}

#[tokio::test]
async fn tavily_quota_authenticates_and_returns_only_usage_fields() {
    let (base, server) = serve(Router::new().route("/usage", get(|headers: axum::http::HeaderMap| async move {
        assert_eq!(headers["authorization"], "Bearer tvly-test");
        Json(json!({"key":{"usage":12,"limit":null,"api_key":"must-not-leak"},
            "account":{"current_plan":"Free","plan_usage":80,"plan_limit":1000,"paygo_usage":0,"paygo_limit":0}}))
    }))).await;
    let app = app().await;
    let id = tavily(&app, &base).await;
    let (status, result) = request(
        &app,
        "GET",
        &format!("/api/connections/{id}/quota"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["key"]["usage"], 12);
    assert!(result["key"]["limit"].is_null());
    assert_eq!(result["account"]["plan_limit"], 1000);
    assert!(!result.to_string().contains("must-not-leak"));
    server.abort();
}

#[tokio::test]
async fn codex_search_and_fetch_use_responses_before_search_connections() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    let (base, server) = serve(Router::new().route("/responses", post(move |headers: axum::http::HeaderMap, Json(body): Json<Value>| {
        counter.fetch_add(1, Ordering::SeqCst);
        async move {
            assert_eq!(headers["authorization"], "Bearer codex-test");
            assert_eq!(headers["originator"], "codex_cli_rs");
            assert_eq!(body["stream"], true);
            assert_eq!(body["model"], "gpt-6.1-sol");
            assert_eq!(body["tools"][0]["type"], "web_search");
            let fetch = body["input"][0]["content"][0]["text"].as_str().unwrap().contains("Open this exact URL");
            let action = if fetch { json!({"type":"open_page","url":"https://example.com"}) }
                else { json!({"type":"search","sources":[{"url":"https://example.com"}]}) };
            let text = if fetch { "page content".to_string() } else {
                assert!(body["instructions"].as_str().unwrap().contains("published_at"));
                json!({"answer":"Example answer", "results":[
                    {"url":"https://example.com", "title":"Example", "snippet":"Example source summary", "published_at":"2026-10-01"},
                    {"url":"https://unobserved.example", "title":"Not a tool source", "snippet":"Discard this"}
                ]}).to_string()
            };
            let output = json!([
                {"type":"web_search_call","status":"completed","action":action},
                {"type":"message","content":[{"type":"output_text","text":text,"annotations":[
                    {"type":"url_citation","url":"https://example.com","title":"Example"}]}]}
            ]);
            let mut stream = String::new();
            for item in output.as_array().unwrap() {
                let event = json!({"type":"response.output_item.done", "item":item});
                stream.push_str(&format!("data: {event}\n\n"));
            }
            let terminal = json!({"type":"response.completed","response":{"status":"completed","output":[]}});
            stream.push_str(&format!("data: {terminal}\n\n"));
            stream
        }
    }))).await;
    let app = app().await;
    // This provider only exposes /responses; a raw /search or /extract is 404.
    tavily(&app, &base).await;
    let (status, connection) = request(
        &app,
        "POST",
        "/api/connections",
        json!({
            "name":"codex", "provider_type":"openai-codex", "base_url":base, "api_key":"codex-test"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{connection}");
    request(
        &app,
        "POST",
        "/api/aliases",
        json!({"prefix":"codex", "connection_id":connection["id"], "model_override":"gpt-6.1-sol"}),
    )
    .await;
    let (status, result) = request(
        &app,
        "POST",
        "/v1/search",
        json!({"model":"codex","query":"test"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["results"][0]["url"], "https://example.com");
    assert_eq!(result["results"].as_array().unwrap().len(), 1);
    assert_eq!(result["results"][0]["title"], "Example");
    assert_eq!(result["results"][0]["snippet"], "Example source summary");
    assert_eq!(result["results"][0]["published_at"], "2026-10-01");
    assert_eq!(result["answer"], "Example answer");
    let (status, result) = request(
        &app,
        "POST",
        "/v1/web/fetch",
        json!({"model":"codex","url":"https://example.com"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["content"]["text"], "page content");
    assert_eq!(result["mode"], "hosted_web_tool");
    assert_eq!(hits.load(Ordering::SeqCst), 2);
    server.abort();
}
