//! `POST /api/auth/*` — dashboard password sign-in and session rotation.

use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::auth::{AuthSession, Rotation, setup_code_matches, validate_password, verify_password};
use crate::error::{Error, Result};
use crate::middleware;
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct Status {
    pub password_set: bool,
    /// True while the router still needs its first password.
    pub setup_required: bool,
    /// Whether the presented bearer token is a live session.
    pub authenticated: bool,
    /// Whether `server.admin_token` also works for scripts.
    pub admin_token_set: bool,
    /// True when the admin API needs no credential at all (loopback posture).
    pub admin_open: bool,
}

/// `GET /api/auth/status` — public; drives the login and setup screens.
pub async fn status(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Status>> {
    let auth = state.auth();
    let password_set = auth.password_set().await?;
    let authenticated = match middleware::extract_bearer(&headers) {
        Some(token) => auth.authenticate(&token).await?,
        None => false,
    };
    let config = state.config_snapshot();
    let admin_open = config.server.allow_unauthenticated_admin
        || (config.server.admin_token().is_none()
            && !password_set
            && !config.server.exposes_network());

    Ok(Json(Status {
        password_set,
        setup_required: !password_set,
        authenticated,
        admin_token_set: config.server.requires_admin_token(),
        admin_open,
    }))
}

#[derive(Debug, Deserialize)]
pub struct SetupRequest {
    /// One-time code printed in the router log at startup.
    pub setup_code: String,
    pub password: String,
}

/// `POST /api/auth/setup` — sets the first password, requires the setup code.
pub async fn setup(
    State(state): State<AppState>,
    Json(input): Json<SetupRequest>,
) -> Result<Json<AuthSession>> {
    let auth = state.auth();
    if auth.password_set().await? {
        return Err(Error::Forbidden(
            "the dashboard password is already set".to_string(),
        ));
    }
    validate_password(&input.password)?;

    let matches = state
        .setup_code()
        .is_some_and(|code| setup_code_matches(&input.setup_code, &code));
    if !matches {
        return Err(Error::Unauthorized("invalid setup code".to_string()));
    }

    auth.set_password(&input.password).await?;
    state.clear_setup_code();
    tracing::info!("dashboard password set");

    Ok(Json(auth.login(&input.password).await?))
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub password: String,
}

/// `POST /api/auth/login` — opens a session family.
pub async fn login(
    State(state): State<AppState>,
    Json(input): Json<LoginRequest>,
) -> Result<Json<AuthSession>> {
    Ok(Json(state.auth().login(&input.password).await?))
}

#[derive(Debug, Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

/// `POST /api/auth/refresh` — rotates the pair; reuse revokes the family.
pub async fn refresh(
    State(state): State<AppState>,
    Json(input): Json<RefreshRequest>,
) -> Result<Json<AuthSession>> {
    match state.auth().refresh(&input.refresh_token).await? {
        Rotation::Rotated(session) => Ok(Json(session)),
        Rotation::Reused => {
            tracing::warn!("a rotated refresh token was replayed; session family revoked");
            Err(Error::Unauthorized(
                "this session was revoked; sign in again".to_string(),
            ))
        }
        Rotation::Missing => Err(Error::Unauthorized(
            "invalid or expired refresh token".to_string(),
        )),
    }
}

/// `POST /api/auth/logout` — revokes the caller's session family.
pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>> {
    if let Some(token) = middleware::extract_bearer(&headers) {
        state.auth().revoke_family_for_access(&token).await?;
    }
    Ok(Json(json!({ "signed_out": true })))
}

#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

/// `PATCH /api/auth/password` — rotates the password and every session.
pub async fn change_password(
    State(state): State<AppState>,
    Json(input): Json<ChangePasswordRequest>,
) -> Result<Json<AuthSession>> {
    let auth = state.auth();
    let Some(hash) = auth.password_hash().await? else {
        return Err(Error::Unauthorized(
            "no dashboard password is set yet".to_string(),
        ));
    };
    if !verify_password(&hash, &input.current_password) {
        return Err(Error::Unauthorized("invalid password".to_string()));
    }
    validate_password(&input.new_password)?;

    auth.set_password(&input.new_password).await?;
    Ok(Json(auth.login(&input.new_password).await?))
}
