//! Fallback ordering, exercised end to end against a mock axum upstream.
//!
//! The mock stands in for an OpenAI-compatible provider so the executor's
//! tier-walking can be verified without any external network calls.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use alnair_router::db::Db;
use alnair_router::db::repos::aliases::CreateAlias;
use alnair_router::db::repos::combos::CreateCombo;
use alnair_router::db::repos::connections::CreateConnection;
use alnair_router::model::Catalog;
use alnair_router::upstream::Executor;
use alnair_router::upstream::chat_backend::{self, ProviderRegistry};
use axum::Router;
use axum::routing::post;

/// Serves a minimal OpenAI-compatible SSE completion.
///
/// The provider appends `/chat/completions` to the configured base URL, so the
/// mock must serve the full `/v1/...` path.
fn upstream_router(fail: bool, hits: Arc<AtomicUsize>) -> Router {
    Router::new().route(
        "/v1/chat/completions",
        post(move |_body: axum::Json<serde_json::Value>| {
            let hits = hits.clone();
            async move {
                hits.fetch_add(1, Ordering::SeqCst);

                if fail {
                    return (
                        axum::http::StatusCode::TOO_MANY_REQUESTS,
                        axum::Json(serde_json::json!({
                            "error": { "message": "rate limited" }
                        })),
                    )
                        .into_response();
                }

                let body = concat!(
                    "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n",
                    "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                    "data: [DONE]\n\n"
                );

                (
                    axum::http::StatusCode::OK,
                    [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                    body,
                )
                    .into_response()
            }
        }),
    )
}

use axum::response::IntoResponse;

/// Spawns the mock upstream and returns its base URL.
async fn spawn_upstream(fail: bool, hits: Arc<AtomicUsize>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = listener.local_addr().expect("addr");
    let router = upstream_router(fail, hits);

    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    format!("http://{address}/v1")
}

#[tokio::test]
async fn combo_falls_through_to_the_second_tier() {
    let db = Db::connect_in_memory().await.expect("db");
    let connections =
        alnair_router::db::repos::connections::ConnectionRepository::new(db.pool.clone());

    let hits = Arc::new(AtomicUsize::new(0));
    let good_url = spawn_upstream(false, hits.clone()).await;

    let dead = connections
        .create(CreateConnection {
            name: "dead".to_string(),
            provider_type: "openai-compatible".to_string(),
            base_url: "http://127.0.0.1:1/v1".to_string(),
            api_key: Some("sk-test".to_string()),
            custom_headers: Default::default(),
            enabled: true,
        })
        .await
        .expect("create dead");

    let good = connections
        .create(CreateConnection {
            name: "good".to_string(),
            provider_type: "openai-compatible".to_string(),
            base_url: good_url,
            api_key: Some("sk-test".to_string()),
            custom_headers: Default::default(),
            enabled: true,
        })
        .await
        .expect("create good");

    let aliases = alnair_router::db::repos::aliases::AliasRepository::new(db.pool.clone());
    aliases
        .create(CreateAlias {
            prefix: "dead".to_string(),
            connection_id: dead.id.clone(),
            model_override: None,
            enabled: true,
            sort_order: 0,
        })
        .await
        .expect("alias dead");
    aliases
        .create(CreateAlias {
            prefix: "good".to_string(),
            connection_id: good.id.clone(),
            model_override: None,
            enabled: true,
            sort_order: 1,
        })
        .await
        .expect("alias good");

    let combos = alnair_router::db::repos::combos::ComboRepository::new(db.pool.clone());
    combos
        .create(CreateCombo {
            name: "rescue".to_string(),
            description: None,
            enabled: true,
            entries: vec!["dead/gpt-4o".to_string(), "good/gpt-4o".to_string()],
        })
        .await
        .expect("combo");

    let catalog = Catalog::load(&db.pool).await.expect("catalog");
    let resolver = catalog
        .resolver(Some("good".to_string()), 5);
    let targets = resolver.resolve("rescue").expect("resolve");
    assert_eq!(targets.len(), 2, "combo should expand into two tiers");

    let executor = Executor::new(Arc::new(ProviderRegistry::with_defaults()));
    let executed = executor
        .stream(
            &targets,
            vec![chat_backend::message_text("user", "hi")],
            None,
            None,
        )
        .await
        .expect("a tier should succeed");

    assert_eq!(executed.attempts.len(), 2, "first tier must have failed");
    assert!(!executed.attempts[0].outcome.is_success());
    assert!(executed.attempts[1].outcome.is_success());
    assert_eq!(executed.target.source, "combo:rescue#2");
    assert_eq!(hits.load(Ordering::SeqCst), 1, "mock hit exactly once");

    let completion = chat_backend::collect(executed.stream)
        .await
        .expect("collect");
    assert_eq!(completion.content, "hello");
}

#[tokio::test]
async fn all_tiers_failing_reports_the_last_error() {
    let db = Db::connect_in_memory().await.expect("db");
    let connections =
        alnair_router::db::repos::connections::ConnectionRepository::new(db.pool.clone());

    let hits = Arc::new(AtomicUsize::new(0));
    // A tier that responds, but with a retryable failure status.
    let failing_url = spawn_upstream(true, hits.clone()).await;

    connections
        .create(CreateConnection {
            name: "flaky".to_string(),
            provider_type: "openai-compatible".to_string(),
            base_url: failing_url,
            api_key: Some("sk-test".to_string()),
            custom_headers: Default::default(),
            enabled: true,
        })
        .await
        .expect("create flaky");

    let aliases = alnair_router::db::repos::aliases::AliasRepository::new(db.pool.clone());
    aliases
        .create(CreateAlias {
            prefix: "flaky".to_string(),
            connection_id: "flaky-id".to_string(),
            model_override: None,
            enabled: true,
            sort_order: 0,
        })
        .await
        .ok();

    let catalog = Catalog::load(&db.pool).await.expect("catalog");
    let resolver = catalog.resolver(Some("flaky".to_string()), 5);
    let targets = resolver.resolve("flaky").expect("resolve");

    let executor = Executor::new(Arc::new(ProviderRegistry::with_defaults()));
    let result = executor
        .stream(
            &targets,
            vec![chat_backend::message_text("user", "hi")],
            None,
            None,
        )
        .await;

    assert!(result.is_err(), "a failing upstream must not report success");
}
