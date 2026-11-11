---
name: axum-rust
description: "Use when building or modifying Rust HTTP services with axum: Router setup, handlers, extractors, State, Json/Form/Query/Path, tower middleware, tower-http layers, error responses, tests, graceful shutdown, or axum version/API migration."
---

# Axum Rust

Use these conventions for Rust web services built with `tokio-rs/axum`.

## Source Baseline

- Prefer the released API from `docs.rs/axum` and crates.io.
- Treat the GitHub `main` branch carefully: it can contain breaking changes toward the next major/minor release. For released behavior, use the matching release branch such as `v0.8.x`.
- Axum is designed for Tokio and hyper; runtime independence is not a goal.
- Axum is a thin, safe Rust layer on hyper and uses Tower for middleware and services.

## Cargo

Start with narrow features, then add only what the app uses:

```toml
[dependencies]
axum = "0.8"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "net", "signal"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# Add when using Tower middleware, testing services, or ServiceBuilder.
tower = "0.5"

# Add specific tower-http features instead of enabling everything.
tower-http = { version = "0.6", features = ["trace", "cors", "timeout"] }
```

- Use `tokio`'s `full` feature only for prototypes or when the repository already standardizes on it.
- Enable axum features intentionally: `macros` for `debug_handler`, `ws` for WebSockets, `multipart` for `Multipart`, `http2` for HTTP/2.

## Minimal Server

```rust
use axum::{routing::get, Router};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = Router::new().route("/", get(root));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn root() -> &'static str {
    "Hello, World!"
}
```

- Use `tokio::net::TcpListener` plus `axum::serve(listener, app)` for modern axum.
- Keep app construction separate from binding/listening when the service needs integration tests.

## Routing

- Build routes with `Router::new().route(path, get(handler).post(handler))`.
- Use `merge` for peer routers and `nest` for route subtrees.
- Keep route modules small: expose `pub fn router(state...) -> Router<AppState>` or `pub fn router() -> Router<AppState>` rather than scattering route setup through `main`.
- Prefer typed path parameters such as `Path<Uuid>` or `Path<(Uuid, String)>`.
- Put catch-all or fallback behavior behind `fallback(handler)` when needed.

## Handlers and Extractors

- Handlers are `async fn`s that accept extractors and return something implementing `IntoResponse`.
- Put `State` and other request-parts extractors before body-consuming extractors like `Json`, `Form`, `Multipart`, or raw body.
- Use `Path<T>`, `Query<T>`, `State<T>`, `Json<T>`, `Form<T>`, and request extensions instead of manual request parsing.
- Derive `Deserialize` for request DTOs and `Serialize` for response DTOs.
- Avoid doing blocking CPU or filesystem work in handlers; use `tokio::task::spawn_blocking` or move work behind an async service boundary.

```rust
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    users: UserRepository,
}

#[derive(Deserialize)]
struct ListQuery {
    limit: Option<usize>,
}

#[derive(Serialize)]
struct UserDto {
    id: Uuid,
    name: String,
}

async fn get_user(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<ListQuery>,
) -> Result<Json<UserDto>, StatusCode> {
    let user = state
        .users
        .find(id, query.limit)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(UserDto { id: user.id, name: user.name }))
}
```

## State

- Prefer `State<AppState>` over `Extension<AppState>` for global application state because it is type checked at compile time.
- Keep `AppState` cheap to clone; store pools, clients, and shared services directly when they already clone cheaply, or wrap expensive mutable state in `Arc`.
- Use `FromRef<AppState>` for substates when a handler or extractor needs only one field.
- Use request `Extension` for per-request data inserted by middleware, such as authenticated user context.
- For shared mutable state, pick the synchronization primitive deliberately:
  - `std::sync::Mutex` or `RwLock` is fine for short critical sections with no `.await` while locked.
  - Use `tokio::sync::Mutex` only when a guard must be held across `.await`.
  - Do not hold a locked `std::sync::Mutex` across `.await`; that produces `!Send` futures that are incompatible with axum handlers.

## Responses and Errors

- Return concrete success types for simple handlers: `Json<T>`, `StatusCode`, `(StatusCode, Json<T>)`, or `(HeaderMap, Json<T>)`.
- For fallible handlers, return `Result<impl IntoResponse, AppError>` or `Result<Json<T>, AppError>`.
- Define an application error enum and implement `IntoResponse` for it; convert internal errors into stable HTTP responses.
- Do not leak raw database, validation, or auth errors into response bodies.
- Use `debug_handler` from axum's `macros` feature temporarily when handler trait errors become opaque.

```rust
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

enum AppError {
    NotFound,
    Internal(anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            Self::NotFound => StatusCode::NOT_FOUND.into_response(),
            Self::Internal(error) => {
                tracing::error!(%error, "request failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": "internal server error" })),
                )
                    .into_response()
            }
        }
    }
}
```

## Middleware

- Axum does not have a bespoke middleware system; it composes Tower services and layers.
- Use `tower_http` layers for common production needs: tracing, CORS, compression, timeout, request IDs, and propagated request IDs.
- Use `tower::ServiceBuilder` when applying multiple layers so order is explicit and easier to reason about.
- Use `Router::layer` for middleware around already-added routes; use `route_layer` when the middleware should run only after a route matches.
- Use `middleware::from_fn` for small app-local middleware.
- Use `middleware::from_fn_with_state` when app-local middleware needs `State`.
- Use custom `tower::Layer`/`Service` only for reusable middleware crates or when lower-level control is required.
- Middleware that can fail must convert errors into responses, often with `HandleErrorLayer`; axum expects request handling errors to be handled.
- If middleware must rewrite the URI before routing, wrap the entire `Router` as a service rather than adding the layer with `Router::layer`.

```rust
use axum::{error_handling::HandleErrorLayer, http::StatusCode, Router};
use std::time::Duration;
use tower::{BoxError, ServiceBuilder};
use tower_http::trace::TraceLayer;

fn apply_http_middleware(app: Router) -> Router {
    app.layer(
        ServiceBuilder::new()
            .layer(HandleErrorLayer::new(|error: BoxError| async move {
                if error.is::<tower::timeout::error::Elapsed>() {
                    StatusCode::REQUEST_TIMEOUT
                } else {
                    tracing::error!(%error, "middleware failed");
                    StatusCode::INTERNAL_SERVER_ERROR
                }
            }))
            .timeout(Duration::from_secs(10))
            .layer(TraceLayer::new_for_http()),
    )
}
```

## Testing

- Test the `Router` as a Tower service instead of starting a socket when possible.
- Put app construction in a function so tests can create `Router<AppState>` with test state.
- Use `tower::ServiceExt::oneshot` for request/response tests.
- Assert both status and body shape; for JSON, deserialize the body instead of comparing raw strings when possible.

```rust
use axum::{body::Body, http::{Request, StatusCode}};
use tower::ServiceExt;

#[tokio::test]
async fn health_returns_ok() {
    let app = app_router(test_state());

    let response = app
        .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}
```

## Graceful Shutdown

- In binaries, wire shutdown explicitly with `axum::serve(...).with_graceful_shutdown(signal())`.
- Listen for `ctrl_c` locally and Unix `terminate` in production when supported.
- Make shutdown close database pools, workers, and telemetry exporters in the same path.

```rust
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
}
```

## Review Checklist

- Released axum API checked against `docs.rs`, not only GitHub `main`.
- `Router` construction is separated from process startup.
- Extractor order keeps body-consuming extractors last.
- `State` is used for global app state; `Extension` is only used for per-request data or dynamic extension cases.
- Shared mutable state does not hold blocking locks across `.await`.
- Errors implement `IntoResponse` and do not expose internals.
- Middleware order is intentional, preferably via `ServiceBuilder`.
- Timeout and other fallible middleware errors are converted into HTTP responses.
- Tests exercise routers through Tower service calls.
- Shutdown path is explicit for deployable binaries.

## Helper Script

Use the helper for a compact starter scaffold:

```bash
bash /mnt/skills/user/axum-rust/scripts/axum-rust-bootstrap.sh api
bash /mnt/skills/user/axum-rust/scripts/axum-rust-bootstrap.sh state
bash /mnt/skills/user/axum-rust/scripts/axum-rust-bootstrap.sh middleware
```
