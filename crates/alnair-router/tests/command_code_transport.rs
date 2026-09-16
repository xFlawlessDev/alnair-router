//! Command Code transport selection, exercised end to end against a mock
//! upstream.
//!
//! A plan without Provider API access (Go) answers `403` on the documented
//! endpoints; the connection must fall back to the CLI envelope and stream
//! anyway. The mock stands in for both transports so this needs no network.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use alnair_router::config::RouterConfig;
use alnair_router::db::Db;
use alnair_router::{AppState, build_router};
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use http_body_util::BodyExt;
use tower::ServiceExt;

const TEST_SECRET: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

/// Where the mock counts what the router asked for.
#[derive(Clone, Default)]
struct Hits {
    models: Arc<AtomicUsize>,
    official: Arc<AtomicUsize>,
    cli: Arc<AtomicUsize>,
}

impl Hits {
    fn count(counter: &Arc<AtomicUsize>) -> usize {
        counter.load(Ordering::SeqCst)
    }
}

/// The rejection a plan without Provider API access gets.
fn upgrade_required() -> axum::response::Response {
    (
        StatusCode::FORBIDDEN,
        axum::Json(serde_json::json!({
            "error": {
                "code": "upgrade_required",
                "message": "Your Go plan doesn't include API access.",
            }
        })),
    )
        .into_response()
}

/// A Command Code stand-in. `models_ok` gates the models probe and `chat_ok`
/// the chat endpoint — real plans can disagree between the two, which is why
/// the transport decision cannot rest on the probe alone.
fn upstream(models_ok: bool, chat_ok: bool) -> (Router, Hits) {
    let hits = Hits::default();

    let models = hits.clone();
    let chat = hits.clone();
    let cli = hits.clone();

    let router = Router::new()
        .route(
            "/provider/v1/models",
            get(move || {
                let hits = models.clone();
                async move {
                    hits.models.fetch_add(1, Ordering::SeqCst);
                    if models_ok {
                        return (
                            StatusCode::OK,
                            axum::Json(serde_json::json!({ "data": [{ "id": "m" }] })),
                        )
                            .into_response();
                    }
                    upgrade_required()
                }
            }),
        )
        .route(
            "/provider/v1/chat/completions",
            post(move |_body: axum::Json<serde_json::Value>| {
                let hits = chat.clone();
                async move {
                    hits.official.fetch_add(1, Ordering::SeqCst);
                    if !chat_ok {
                        return upgrade_required();
                    }
                    (
                        StatusCode::OK,
                        [(header::CONTENT_TYPE, "text/event-stream")],
                        concat!(
                            "data: {\"choices\":[{\"delta\":{\"content\":\"from the api\"}}]}\n\n",
                            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                            "data: [DONE]\n\n"
                        ),
                    )
                        .into_response()
                }
            }),
        )
        .route(
            "/alpha/generate",
            post(move |_body: axum::Json<serde_json::Value>| {
                let hits = cli.clone();
                async move {
                    hits.cli.fetch_add(1, Ordering::SeqCst);
                    // Newline-delimited events, as the CLI transport sends them.
                    (
                        StatusCode::OK,
                        [(header::CONTENT_TYPE, "text/event-stream")],
                        concat!(
                            "{\"type\":\"reasoning-delta\",\"text\":\"weighing it\"}\n",
                            "{\"type\":\"text-delta\",\"text\":\"from the cli\"}\n",
                            "{\"type\":\"finish-step\",\"usage\":{\"inputTokens\":12,\"outputTokens\":4,",
                            "\"inputTokenDetails\":{\"cachedTokens\":2}}}\n",
                            "{\"type\":\"finish\",\"finishReason\":\"stop\"}\n"
                        ),
                    )
                        .into_response()
                }
            }),
        );

    (router, hits)
}

async fn spawn_upstream(router: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    format!("http://{address}")
}

/// The real router over an in-memory database, with one `command-code`
/// connection as the default route.
async fn app_with(base_url: &str) -> Router {
    let db = Db::connect_in_memory().await.expect("db");
    let mut config = RouterConfig::default();
    config.secrets.key = Some(TEST_SECRET.to_string());
    config.router.default_connection = Some("cmd".to_string());
    let state = AppState::new(config, db).expect("state");
    let app = build_router(state);

    let response = request(
        &app,
        "POST",
        "/api/connections",
        Some(serde_json::json!({
            "name": "cmd",
            "provider_type": "command-code",
            "base_url": base_url,
            "api_key": "user_test",
        })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED, "connection");

    app
}

async fn request(
    app: &Router,
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

async fn completion(app: &Router) -> (StatusCode, String) {
    let response = request(
        app,
        "POST",
        "/v1/chat/completions",
        Some(serde_json::json!({
            "model": "kimi-k3",
            "messages": [{ "role": "user", "content": "hi" }],
        })),
    )
    .await;

    body_text(response).await
}

#[tokio::test]
async fn a_plan_without_api_access_falls_back_to_the_cli_transport() {
    let (upstream, hits) = upstream(false, false);
    let base_url = spawn_upstream(upstream).await;
    let app = app_with(&base_url).await;

    let (status, body) = completion(&app).await;

    assert_eq!(status, StatusCode::OK, "unexpected error body: {body}");
    let payload: serde_json::Value = serde_json::from_str(&body).expect("json body");
    let content = payload["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or_default();
    assert_eq!(content, "from the cli");
    assert_eq!(
        Hits::count(&hits.cli),
        1,
        "the CLI endpoint serves the request"
    );
    assert_eq!(
        Hits::count(&hits.official),
        0,
        "the rejected Provider API path is not used once the plan is known"
    );
    assert_eq!(
        payload["usage"]["prompt_tokens"].as_u64(),
        Some(12),
        "CLI usage is reported"
    );
}

#[tokio::test]
async fn an_entitled_credential_uses_the_provider_api() {
    let (upstream, hits) = upstream(true, true);
    let base_url = spawn_upstream(upstream).await;
    let app = app_with(&base_url).await;

    let (status, body) = completion(&app).await;

    assert_eq!(status, StatusCode::OK, "unexpected error body: {body}");
    let payload: serde_json::Value = serde_json::from_str(&body).expect("json body");
    assert_eq!(payload["choices"][0]["message"]["content"], "from the api");
    assert_eq!(Hits::count(&hits.official), 1);
    assert_eq!(Hits::count(&hits.cli), 0, "the CLI path stays unused");
}

#[tokio::test]
async fn the_transport_probe_runs_once_per_credential() {
    let (upstream, hits) = upstream(false, false);
    let base_url = spawn_upstream(upstream).await;
    let app = app_with(&base_url).await;

    let (first, _) = completion(&app).await;
    let (second, _) = completion(&app).await;

    assert_eq!(first, StatusCode::OK);
    assert_eq!(second, StatusCode::OK);
    assert_eq!(
        Hits::count(&hits.models),
        1,
        "the entitlement probe is memoized, so a Go plan pays it once"
    );
    assert_eq!(Hits::count(&hits.cli), 2);
}

/// The models probe is not gated the way the chat endpoint is on every plan, so
/// a rejection on the real request has to switch transports too — and stick.
#[tokio::test]
async fn a_chat_rejection_switches_to_the_cli_transport() {
    let (upstream, hits) = upstream(true, false);
    let base_url = spawn_upstream(upstream).await;
    let app = app_with(&base_url).await;

    let (status, body) = completion(&app).await;
    assert_eq!(status, StatusCode::OK, "unexpected error body: {body}");
    let payload: serde_json::Value = serde_json::from_str(&body).expect("json body");
    assert_eq!(
        payload["choices"][0]["message"]["content"], "from the cli",
        "the rejected Provider API attempt must be retried over the CLI"
    );
    assert_eq!(Hits::count(&hits.official), 1);
    assert_eq!(Hits::count(&hits.cli), 1);

    let (status, _) = completion(&app).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        Hits::count(&hits.official),
        1,
        "the rejected transport is remembered, so the next request skips it"
    );
    assert_eq!(Hits::count(&hits.cli), 2);
}
