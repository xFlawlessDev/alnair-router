//! Axum router construction.

use axum::routing::{delete, get, post};
use axum::{Router, middleware as axum_middleware};
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::handlers;
use crate::middleware;
use crate::state::AppState;

/// Builds the application router with all routes wired.
pub fn build_router(state: AppState) -> Router {
    // Liveness/readiness probes stay public: container and load-balancer
    // healthchecks cannot easily carry a bearer token.
    let probes = Router::new()
        .route("/api/health", get(handlers::admin::health))
        .route("/api/ready", get(handlers::admin::ready));

    // Admin and management routes are unauthenticated on loopback; when
    // `server.admin_token` is configured the token is enforced on all of them.
    let admin = Router::new()
        .route("/api/version", get(handlers::admin::version))
        .route("/api/init", get(handlers::admin::init_state))
        .route(
            "/api/connections",
            get(handlers::admin::list_connections).post(handlers::admin::create_connection),
        )
        .route(
            "/api/connections/{id}",
            delete(handlers::admin::delete_connection).patch(handlers::admin::update_connection),
        )
        .route(
            "/api/aliases",
            get(handlers::admin::list_aliases).post(handlers::admin::create_alias),
        )
        .route(
            "/api/aliases/{id}",
            delete(handlers::admin::delete_alias).patch(handlers::admin::update_alias),
        )
        .route(
            "/api/combos",
            get(handlers::admin::list_combos).post(handlers::admin::create_combo),
        )
        .route(
            "/api/combos/{id}",
            delete(handlers::admin::delete_combo).patch(handlers::admin::update_combo),
        )
        .route(
            "/api/keys",
            get(handlers::admin::list_keys).post(handlers::admin::create_key),
        )
        .route(
            "/api/keys/{id}",
            delete(handlers::admin::delete_key).patch(handlers::admin::update_key),
        )
        .route("/api/usage", get(handlers::admin::list_usage))
        .route("/api/usage/summary", get(handlers::admin::usage_summary))
        .route("/api/metrics", get(handlers::admin::metrics))
        // Enforced only when `server.admin_token` is configured; loopback
        // without a token keeps the documented frictionless posture.
        .route_layer(axum_middleware::from_fn_with_state(
            state.clone(),
            middleware::require_admin_token,
        ));

    let v1 = Router::new()
        .route(
            "/v1/chat/completions",
            post(handlers::chat::chat_completions),
        )
        .route("/v1/messages", post(handlers::messages::messages))
        .route(
            "/v1/messages/count_tokens",
            post(handlers::messages::count_tokens),
        )
        .route("/v1/responses", post(handlers::responses::responses))
        .route("/v1/models", get(handlers::models::list_models))
        .route("/v1/models/info", get(handlers::models::models_info))
        .route("/v1/embeddings", post(handlers::media::embeddings))
        .route(
            "/v1/images/generations",
            post(handlers::media::image_generations),
        )
        .route("/v1/audio/speech", post(handlers::media::audio_speech))
        .route(
            "/v1/audio/transcriptions",
            post(handlers::media::audio_transcriptions),
        )
        .route(
            "/v1/videos/generations",
            post(handlers::media::video_generations),
        )
        .route("/v1/videos/{id}", get(handlers::media::video_status))
        .route("/v1/search", post(handlers::media::search))
        .route("/v1/web/fetch", post(handlers::media::web_fetch))
        .route_layer(axum_middleware::from_fn_with_state(
            state.clone(),
            middleware::require_api_key,
        ));

    Router::new()
        .merge(probes)
        .merge(admin)
        .merge(v1)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
