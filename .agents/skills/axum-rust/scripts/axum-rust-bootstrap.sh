#!/bin/bash
set -euo pipefail

scenario="${1:-api}"
tmp_file=""

cleanup() {
  if [ -n "$tmp_file" ] && [ -f "$tmp_file" ]; then
    rm -f "$tmp_file"
  fi
}
trap cleanup EXIT

case "$scenario" in
  api|state|middleware|test)
    ;;
  *)
    echo "Unsupported scenario: $scenario. Use api, state, middleware, or test." >&2
    exit 2
    ;;
esac

echo "Preparing axum scaffold for $scenario" >&2

case "$scenario" in
  api)
    snippet=$(cat <<RUST
use axum::{routing::get, Router};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = Router::new().route("/health", get(health));
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> String {
    "ok".to_owned()
}
RUST
)
    ;;
  state)
    snippet=$(cat <<RUST
use axum::{extract::State, routing::get, Router};

#[derive(Clone)]
struct AppState {
    service: UserService,
}

fn router(state: AppState) -> Router {
    Router::new()
        .route("/users", get(list_users))
        .with_state(state)
}

async fn list_users(State(state): State<AppState>) -> axum::Json<Vec<UserDto>> {
    axum::Json(state.service.list().await)
}
RUST
)
    ;;
  middleware)
    snippet=$(cat <<RUST
use axum::{error_handling::HandleErrorLayer, http::StatusCode, Router};
use std::time::Duration;
use tower::{BoxError, ServiceBuilder};
use tower_http::trace::TraceLayer;

fn with_middleware(app: Router) -> Router {
    app.layer(
        ServiceBuilder::new()
            .layer(HandleErrorLayer::new(|error: BoxError| async move {
                tracing::error!(%error, "middleware failed");
                StatusCode::INTERNAL_SERVER_ERROR
            }))
            .timeout(Duration::from_secs(10))
            .layer(TraceLayer::new_for_http()),
    )
}
RUST
)
    ;;
  test)
    snippet=$(cat <<RUST
use axum::{body::Body, http::{Request, StatusCode}};
use tower::ServiceExt;

#[tokio::test]
async fn health_returns_ok() {
    let response = app_router(test_state())
        .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}
RUST
)
    ;;
esac

SCENARIO="$scenario" SNIPPET="$snippet" python3 <<PY
import json
import os

scenario = os.environ["SCENARIO"]
snippet = os.environ["SNIPPET"]
print(json.dumps({
    "scenario": scenario,
    "dependencies": {
        "runtime": "tokio = { version = \"1\", features = [\"macros\", \"rt-multi-thread\", \"net\", \"signal\"] }",
        "web": "axum = \"0.8\"",
        "testing_or_middleware": ["tower = \"0.5\"", "tower-http = { version = \"0.6\", features = [\"trace\", \"timeout\"] }"]
    },
    "snippet": snippet,
    "reminders": [
        "Check docs.rs for released axum APIs; GitHub main may contain breaking changes.",
        "Keep State before body extractors like Json or Form.",
        "Use ServiceBuilder for multiple middleware layers.",
        "Do not hold blocking mutex guards across .await in handlers."
    ]
}, indent=2))
PY
