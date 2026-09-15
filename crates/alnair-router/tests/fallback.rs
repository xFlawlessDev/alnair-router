//! Fallback ordering, exercised end to end against a mock axum upstream.
//!
//! The mock stands in for an OpenAI-compatible provider so the executor's
//! tier-walking can be verified without any external network calls.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use alnair_router::config::LimitsConfig;
use alnair_router::db::Db;
use alnair_router::db::repos::aliases::CreateAlias;
use alnair_router::db::repos::combos::CreateCombo;
use alnair_router::db::repos::connections::CreateConnection;
use alnair_router::limits::UpstreamLimiter;
use alnair_router::model::Catalog;
use alnair_router::upstream::chat_backend::RetryPolicy;
use alnair_router::upstream::chat_backend::{self, ProviderRegistry};
use alnair_router::upstream::{Executor, ExecutorSettings, UpstreamTimeouts};
use axum::Router;
use axum::routing::post;
use futures::StreamExt;

/// A fresh per-process key for the credential cipher under test.
fn test_cipher() -> Arc<alnair_router::crypto::CredentialCipher> {
    Arc::new(alnair_router::crypto::CredentialCipher::ephemeral())
}

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
    spawn_router(upstream_router(fail, hits)).await
}

/// Spawns an arbitrary router and returns its base URL.
async fn spawn_router(router: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = listener.local_addr().expect("addr");

    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    format!("http://{address}/v1")
}

/// Answers only after `delay`, simulating a hanging upstream.
fn slow_upstream(delay: Duration) -> Router {
    Router::new().route(
        "/v1/chat/completions",
        post(move |_body: axum::Json<serde_json::Value>| async move {
            tokio::time::sleep(delay).await;
            (
                axum::http::StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                "data: [DONE]\n\n",
            )
                .into_response()
        }),
    )
}

/// Sends one chunk immediately, then goes quiet for `delay` before finishing.
fn trickling_upstream(delay: Duration) -> Router {
    Router::new().route(
        "/v1/chat/completions",
        post(move |_body: axum::Json<serde_json::Value>| async move {
            let first = futures::stream::once(async {
                Ok::<_, std::convert::Infallible>(bytes::Bytes::from(
                    "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n",
                ))
            });
            let second = futures::stream::once(async move {
                tokio::time::sleep(delay).await;
                Ok(bytes::Bytes::from("data: [DONE]\n\n"))
            });

            let body = axum::body::Body::from_stream(first.chain(second));
            (
                axum::http::StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                body,
            )
                .into_response()
        }),
    )
}

#[tokio::test]
async fn combo_falls_through_to_the_second_tier() {
    let db = Db::connect_in_memory().await.expect("db");
    let cipher = test_cipher();
    let connections = alnair_router::db::repos::connections::ConnectionRepository::new(
        db.pool.clone(),
        cipher.clone(),
    );

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

            connect_timeout_ms: None,

            idle_timeout_ms: None,
            pricing_model: None,
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

            connect_timeout_ms: None,

            idle_timeout_ms: None,
            pricing_model: None,
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

    let catalog = Catalog::load(&db.pool, &cipher).await.expect("catalog");
    let resolver = catalog.resolver(Some("good".to_string()), 5);
    let targets = resolver.resolve("rescue").expect("resolve");
    assert_eq!(targets.len(), 2, "combo should expand into two tiers");

    let executor = Executor::new(Arc::new(ProviderRegistry::with_defaults()));
    let executed = executor
        .stream(
            &targets,
            vec![chat_backend::message_text("user", "hi")],
            None,
            None,
            false,
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
    let cipher = test_cipher();
    let connections = alnair_router::db::repos::connections::ConnectionRepository::new(
        db.pool.clone(),
        cipher.clone(),
    );

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

            connect_timeout_ms: None,

            idle_timeout_ms: None,
            pricing_model: None,
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

    let catalog = Catalog::load(&db.pool, &cipher).await.expect("catalog");
    let resolver = catalog.resolver(Some("flaky".to_string()), 5);
    let targets = resolver.resolve("flaky").expect("resolve");

    let executor = Executor::new(Arc::new(ProviderRegistry::with_defaults()));
    let result = executor
        .stream(
            &targets,
            vec![chat_backend::message_text("user", "hi")],
            None,
            None,
            false,
        )
        .await;

    assert!(
        result.is_err(),
        "a failing upstream must not report success"
    );
}

#[tokio::test]
async fn connect_timeout_fails_the_tier_without_waiting() {
    let db = Db::connect_in_memory().await.expect("db");
    let cipher = test_cipher();
    let connections = alnair_router::db::repos::connections::ConnectionRepository::new(
        db.pool.clone(),
        cipher.clone(),
    );

    let slow_url = spawn_router(slow_upstream(Duration::from_secs(5))).await;
    connections
        .create(CreateConnection {
            name: "slow".to_string(),
            provider_type: "openai-compatible".to_string(),
            base_url: slow_url,
            api_key: Some("sk-test".to_string()),
            custom_headers: Default::default(),
            enabled: true,
            connect_timeout_ms: Some(50),
            idle_timeout_ms: None,
            pricing_model: None,
        })
        .await
        .expect("create slow");

    let aliases = alnair_router::db::repos::aliases::AliasRepository::new(db.pool.clone());
    aliases
        .create(CreateAlias {
            prefix: "slow".to_string(),
            connection_id: "ignored".to_string(),
            model_override: None,
            enabled: true,
            sort_order: 0,
        })
        .await
        .ok();

    let catalog = Catalog::load(&db.pool, &cipher).await.expect("catalog");
    let resolver = catalog.resolver(Some("slow".to_string()), 5);
    let targets = resolver.resolve("slow").expect("resolve");

    let executor = Executor::with_settings(
        Arc::new(ProviderRegistry::with_defaults()),
        ExecutorSettings {
            retry: RetryPolicy {
                max_retries_per_tier: 0,
                max_retry_delay_ms: 1_000,
            },
            limiter: UpstreamLimiter::new(&LimitsConfig::default()),
            timeouts: UpstreamTimeouts {
                connect_timeout_ms: 10_000,
                idle_timeout_ms: 0,
            },
            metrics: Arc::new(alnair_router::metrics::Metrics::default()),
            telemetry: Arc::new(alnair_router::telemetry::ActivityTracker::new()),
            pricing: None,
        },
    );

    let started = std::time::Instant::now();
    let result = executor
        .stream(
            &targets,
            vec![chat_backend::message_text("user", "hi")],
            None,
            None,
            true,
        )
        .await;

    assert!(result.is_err(), "a hanging tier must fail");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "the per-connection connect timeout should have fired, took {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn idle_streams_error_after_the_timeout() {
    let db = Db::connect_in_memory().await.expect("db");
    let cipher = test_cipher();
    let connections = alnair_router::db::repos::connections::ConnectionRepository::new(
        db.pool.clone(),
        cipher.clone(),
    );

    let url = spawn_router(trickling_upstream(Duration::from_secs(5))).await;
    connections
        .create(CreateConnection {
            name: "trickle".to_string(),
            provider_type: "openai-compatible".to_string(),
            base_url: url,
            api_key: Some("sk-test".to_string()),
            custom_headers: Default::default(),
            enabled: true,
            connect_timeout_ms: Some(0),
            idle_timeout_ms: Some(50),
            pricing_model: None,
        })
        .await
        .expect("create trickle");

    let catalog = Catalog::load(&db.pool, &cipher).await.expect("catalog");
    let resolver = catalog.resolver(Some("trickle".to_string()), 5);
    let targets = resolver.resolve("trickle").expect("resolve");

    let executor = Executor::with_settings(
        Arc::new(ProviderRegistry::with_defaults()),
        ExecutorSettings {
            retry: RetryPolicy {
                max_retries_per_tier: 0,
                max_retry_delay_ms: 1_000,
            },
            limiter: UpstreamLimiter::new(&LimitsConfig::default()),
            timeouts: UpstreamTimeouts {
                connect_timeout_ms: 10_000,
                idle_timeout_ms: 0,
            },
            metrics: Arc::new(alnair_router::metrics::Metrics::default()),
            telemetry: Arc::new(alnair_router::telemetry::ActivityTracker::new()),
            pricing: None,
        },
    );

    let executed = executor
        .stream(
            &targets,
            vec![chat_backend::message_text("user", "hi")],
            None,
            None,
            true,
        )
        .await
        .expect("first chunk should arrive");

    let started = std::time::Instant::now();
    let error = chat_backend::collect(executed.stream)
        .await
        .expect_err("idle timeout must surface as an error");

    assert!(
        error.to_string().contains("idle"),
        "unexpected error: {error}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "idle timeout should fire promptly, took {:?}",
        started.elapsed()
    );
}
