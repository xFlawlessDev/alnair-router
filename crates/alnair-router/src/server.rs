//! Axum router construction.

use axum::routing::{delete, get, patch, post};
use axum::{Router, middleware as axum_middleware};
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

    // Read-only surfaces that identify the caller with a client key instead of
    // the admin token.
    let public = Router::new()
        .route("/api/public/usage", get(handlers::public::usage))
        .route("/api/public/models", get(handlers::public::models))
        // Sign-in must work before any credential exists.
        .route("/api/auth/status", get(handlers::auth::status))
        .route("/api/auth/setup", post(handlers::auth::setup))
        .route("/api/auth/login", post(handlers::auth::login))
        .route("/api/auth/refresh", post(handlers::auth::refresh));

    // Admin and management routes are unauthenticated on loopback; when
    // `server.admin_token` is configured the token is enforced on all of them.
    let admin = Router::new()
        .route("/api/version", get(handlers::admin::version))
        .route("/api/init", get(handlers::admin::init_state))
        .route("/api/providers", get(handlers::providers::list_providers))
        .route(
            "/api/connections",
            get(handlers::admin::list_connections).post(handlers::admin::create_connection),
        )
        .route(
            "/api/connections/{id}",
            delete(handlers::admin::delete_connection).patch(handlers::admin::update_connection),
        )
        .route(
            "/api/connections/{id}/models",
            get(handlers::admin::connection_models),
        )
        .route(
            "/api/connections/{id}/accounts",
            get(handlers::accounts::list_accounts).post(handlers::accounts::create_account),
        )
        .route(
            "/api/connections/{id}/accounts/{account_id}",
            delete(handlers::accounts::delete_account).patch(handlers::accounts::update_account),
        )
        .route(
            "/api/connections/{id}/test",
            post(handlers::admin::connection_test),
        )
        .route(
            "/api/aliases",
            get(handlers::admin::list_aliases).post(handlers::admin::create_alias),
        )
        .route(
            "/api/aliases/{id}",
            delete(handlers::admin::delete_alias).patch(handlers::admin::update_alias),
        )
        .route("/api/aliases/{id}/test", post(handlers::admin::alias_test))
        .route(
            "/api/aliases/{id}/test-chat",
            post(handlers::admin::alias_chat_test),
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
        .route(
            "/api/plans",
            get(handlers::admin::list_plans).post(handlers::admin::create_plan),
        )
        .route(
            "/api/plans/{id}",
            delete(handlers::admin::delete_plan).patch(handlers::admin::update_plan),
        )
        .route("/api/usage", get(handlers::admin::list_usage))
        .route("/api/usage/summary", get(handlers::admin::usage_summary))
        .route("/api/usage/facets", get(handlers::admin::usage_facets))
        .route("/api/usage/models", get(handlers::admin::usage_models))
        .route("/api/usage/keys", get(handlers::admin::usage_by_key))
        .route("/api/models", get(handlers::catalog::models_catalog))
        .route(
            "/api/pricing",
            get(handlers::admin::list_pricing)
                .put(handlers::admin::upsert_pricing)
                .delete(handlers::admin::delete_pricing),
        )
        .route(
            "/api/pricing/sync",
            get(handlers::admin::pricing_sync_status).post(handlers::admin::sync_pricing),
        )
        .route("/api/pricing/match", get(handlers::admin::match_pricing))
        .route("/api/activity", get(handlers::admin::activity))
        .route("/api/metrics", get(handlers::admin::metrics))
        .route(
            "/api/settings",
            get(handlers::settings::get_settings)
                .patch(handlers::settings::update_settings)
                .delete(handlers::settings::reset_settings),
        )
        .route("/api/backup", get(handlers::backup::download_backup))
        .route("/api/restore", post(handlers::backup::restore_backup))
        .route("/api/auth/logout", post(handlers::auth::logout))
        .route("/api/auth/password", patch(handlers::auth::change_password))
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

    let config = state.config_snapshot();
    let mut app = Router::new()
        .merge(probes)
        .merge(public)
        .merge(admin)
        .merge(v1);
    if config.server.serve_dashboard {
        // Unmatched paths fall through to the embedded dashboard (SPA routes
        // resolve to index.html; missing files 404).
        app = app.fallback(handlers::web::serve_asset);
    }

    app.layer(axum_middleware::from_fn_with_state(
        state.clone(),
        middleware::cors,
    ))
    .layer(TraceLayer::new_for_http())
    .with_state(state)
}
