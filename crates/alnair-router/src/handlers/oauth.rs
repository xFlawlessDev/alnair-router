//! OAuth login flows and account management.
//!
//! Two ways in: a PKCE authorization-code redirect the operator completes in a
//! browser, and a device code for a router with no browser of its own. Both end
//! with an encrypted credential stored against the connection, after which the
//! executor resolves an access token just in time.
//!
//! The callback route is public — a browser redirect carries no admin token —
//! so it is guarded by the PKCE `state` nonce alone.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use serde::{Deserialize, Serialize};

use crate::db::repos::oauth_accounts::{CreateOAuthAccount, OAuthAccount, UpdateOAuthAccount};
use crate::error::{Error, Result};
use crate::oauth::credential::OAuthCredential;
use crate::oauth::{self, ClientInputs, DeviceChallenge, LoginState, LoginView, PendingLogin};
use crate::state::AppState;

/// `POST /api/oauth/logins` — start a PKCE authorization.
#[derive(Debug, Deserialize)]
pub struct StartLoginRequest {
    pub connection_id: String,
    pub label: String,
    #[serde(default = "default_provider_key")]
    pub provider_key: String,
    #[serde(flatten)]
    pub client: ClientInputs,
}

#[derive(Debug, Serialize)]
pub struct StartLoginResponse {
    pub login_id: String,
    pub authorize_url: String,
    /// The URI that must be registered with the provider.
    pub redirect_uri: String,
    /// True when this login needs the operator to paste it into their OAuth app.
    pub device: bool,
}

/// `POST /api/oauth/device-logins` — start a device-code authorization.
#[derive(Debug, Deserialize)]
pub struct StartDeviceLoginRequest {
    pub connection_id: String,
    pub label: String,
    #[serde(default = "default_provider_key")]
    pub provider_key: String,
    #[serde(flatten)]
    pub client: ClientInputs,
}

#[derive(Debug, Serialize)]
pub struct StartDeviceLoginResponse {
    pub login_id: String,
    pub user_code: String,
    pub verification_uri: Option<String>,
    pub expires_in: Option<i64>,
    pub interval: Option<i64>,
}

/// `GET /api/oauth/callback` query parameters.
#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub error_description: Option<String>,
}

fn default_provider_key() -> String {
    "generic".to_string()
}

/// The redirect URI, derived from the request the operator is talking to.
///
/// A loopback router serves the callback itself, so the URI is its own address.
/// This is what the operator must register with the provider.
fn redirect_uri_for(headers: &HeaderMap) -> String {
    let host = headers
        .get(axum::http::header::HOST)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("127.0.0.1:7878");

    oauth::redirect_uri(host)
}

/// `POST /api/oauth/logins`
pub async fn start_login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<StartLoginRequest>,
) -> Result<impl IntoResponse> {
    let endpoints = input.client.into_endpoints()?;
    if endpoints.authorize_url.is_empty() {
        return Err(Error::BadRequest(
            "authorize_url is required for a browser login; use the device-code endpoint for a provider without one"
                .to_string(),
        ));
    }

    ensure_connection(&state, &input.connection_id).await?;
    let label = normalized_label(&input.label)?;

    let redirect_uri = redirect_uri_for(&headers);
    let (verifier, challenge) = oauth::flows::pkce_pair();
    let login_id = oauth::pkce::random_id();
    let state_nonce = oauth::pkce::random_state();

    let authorize_url =
        oauth::flows::authorize_url(&endpoints, &redirect_uri, &state_nonce, &challenge)?;

    state
        .oauth_logins
        .insert(PendingLogin::pkce(
            login_id.clone(),
            input.connection_id,
            label,
            input.provider_key,
            endpoints,
            state_nonce,
            verifier,
            redirect_uri.clone(),
        ))
        .await;

    Ok((
        StatusCode::CREATED,
        Json(StartLoginResponse {
            login_id,
            authorize_url,
            redirect_uri,
            device: false,
        }),
    ))
}

/// `POST /api/oauth/device-logins`
pub async fn start_device_login(
    State(state): State<AppState>,
    Json(input): Json<StartDeviceLoginRequest>,
) -> Result<impl IntoResponse> {
    let endpoints = input.client.into_endpoints()?;

    ensure_connection(&state, &input.connection_id).await?;
    let label = normalized_label(&input.label)?;

    if endpoints.device_code_url.is_none() {
        return Err(Error::BadRequest(
            "this provider has no device-code endpoint configured; use the browser login instead"
                .to_string(),
        ));
    }

    let challenge = oauth::flows::start_device_code(&endpoints).await?;

    let login_id = oauth::pkce::random_id();
    state
        .oauth_logins
        .insert(PendingLogin::device(
            login_id.clone(),
            input.connection_id,
            label,
            input.provider_key,
            endpoints.clone(),
            DeviceChallenge {
                user_code: challenge.user_code.clone(),
                verification_uri: challenge.verification_uri.clone(),
                expires_in: challenge.expires_in,
            },
        ))
        .await;

    // Polling lives server-side so closing the dialog does not lose the login.
    spawn_device_poll(
        state.clone(),
        login_id.clone(),
        endpoints,
        challenge.device_code,
        challenge.interval,
        challenge.expires_in,
    );

    Ok((
        StatusCode::CREATED,
        Json(StartDeviceLoginResponse {
            login_id,
            user_code: challenge.user_code,
            verification_uri: challenge.verification_uri,
            expires_in: challenge.expires_in,
            interval: challenge.interval,
        }),
    ))
}

/// Polls the token endpoint until the operator approves, or the code expires.
fn spawn_device_poll(
    state: AppState,
    login_id: String,
    endpoints: crate::oauth::EndpointConfig,
    device_code: String,
    interval: Option<i64>,
    expires_in: Option<i64>,
) {
    let seconds = interval.unwrap_or(5).clamp(1, 60) as u64;
    let deadline = std::time::Instant::now()
        + std::time::Duration::from_secs(expires_in.unwrap_or(300).clamp(30, 3600) as u64);

    tokio::spawn(async move {
        loop {
            if std::time::Instant::now() >= deadline {
                state
                    .oauth_logins
                    .settle(
                        &login_id,
                        LoginState::Failed {
                            error: "the device code expired before it was approved".to_string(),
                        },
                    )
                    .await;
                return;
            }

            tokio::time::sleep(std::time::Duration::from_secs(seconds)).await;

            // The operator may have cancelled while this poll was sleeping.
            let Some(login) = state.oauth_logins.get(&login_id).await else {
                return;
            };
            if !matches!(login.state_value, LoginState::Pending) {
                return;
            }

            match oauth::flows::poll_device_code(&endpoints, &device_code).await {
                Ok(oauth::flows::DevicePoll::Pending) => continue,
                Ok(oauth::flows::DevicePoll::Failed(error)) => {
                    state
                        .oauth_logins
                        .settle(&login_id, LoginState::Failed { error })
                        .await;
                    return;
                }
                Ok(oauth::flows::DevicePoll::Authorized(token)) => {
                    let outcome = store_account(
                        &state,
                        &login.connection_id,
                        &login.label,
                        &login.provider_key,
                        endpoints,
                        *token,
                    )
                    .await;

                    let settled = match outcome {
                        Ok(account_id) => LoginState::Completed { account_id },
                        Err(error) => LoginState::Failed {
                            error: error.to_string(),
                        },
                    };
                    state.oauth_logins.settle(&login_id, settled).await;
                    return;
                }
                Err(error) => {
                    // A transient network fault should not kill a valid login;
                    // the deadline above bounds the retries.
                    tracing::warn!(login = %login_id, error = %error, "device poll failed; retrying");
                    continue;
                }
            }
        }
    });
}

/// Turns a token response into a stored account.
async fn store_account(
    state: &AppState,
    connection_id: &str,
    label: &str,
    provider_key: &str,
    endpoints: crate::oauth::EndpointConfig,
    token: oauth::flows::TokenResponse,
) -> Result<String> {
    let now = chrono::Utc::now();
    let account_info = oauth::flows::fetch_account_info(&endpoints, &token.access_token).await;

    let credential = OAuthCredential {
        access_token: token.access_token,
        refresh_token: token.refresh_token,
        expires_at: token
            .expires_in
            .map(|seconds| now + chrono::Duration::seconds(seconds)),
        token_type: token.token_type.or_else(|| Some("Bearer".to_string())),
        scope: token.scope,
        endpoints,
        account: account_info,
    };

    let account = state
        .oauth_accounts()
        .create(
            connection_id,
            CreateOAuthAccount {
                label: label.to_string(),
                provider_key: provider_key.to_string(),
                credential,
                enabled: true,
            },
        )
        .await?;

    // The catalog mirrors account ids, so a new account must be visible to the
    // resolver on the next request.
    state.invalidate_catalog().await;
    Ok(account.id)
}

/// `GET /api/oauth/callback` — the browser comes back here.
///
/// Public by necessity: a redirect from the provider carries no admin token.
/// The `state` nonce is the guard, and it is single-use because the login is
/// removed once it resolves.
pub async fn callback(
    State(state): State<AppState>,
    Query(query): Query<CallbackQuery>,
) -> Response {
    let Some(state_nonce) = query.state.as_deref() else {
        return callback_page(false, "The provider did not return a state parameter.");
    };

    let Some(login) = state.oauth_logins.by_state(state_nonce).await else {
        return callback_page(
            false,
            "This login link is no longer valid. Start the connection again from the dashboard.",
        );
    };

    if let Some(error) = query.error.as_deref() {
        let detail = query.error_description.as_deref().unwrap_or(error);
        state
            .oauth_logins
            .settle(
                &login.id,
                LoginState::Failed {
                    error: detail.to_string(),
                },
            )
            .await;
        return callback_page(
            false,
            &format!("The provider refused the request: {detail}"),
        );
    }

    let Some(code) = query.code.as_deref() else {
        return callback_page(false, "The provider did not return an authorization code.");
    };

    let token = match oauth::flows::exchange_code(
        &login.endpoints,
        code,
        &login.redirect_uri,
        &login.verifier,
    )
    .await
    {
        Ok(token) => token,
        Err(error) => {
            state
                .oauth_logins
                .settle(
                    &login.id,
                    LoginState::Failed {
                        error: error.to_string(),
                    },
                )
                .await;
            return callback_page(false, &format!("Could not exchange the code: {error}"));
        }
    };

    match store_account(
        &state,
        &login.connection_id,
        &login.label,
        &login.provider_key,
        login.endpoints.clone(),
        token,
    )
    .await
    {
        Ok(account_id) => {
            state
                .oauth_logins
                .settle(&login.id, LoginState::Completed { account_id })
                .await;
            callback_page(true, "Account connected. You can close this tab.")
        }
        Err(error) => {
            state
                .oauth_logins
                .settle(
                    &login.id,
                    LoginState::Failed {
                        error: error.to_string(),
                    },
                )
                .await;
            callback_page(false, &format!("Could not store the credential: {error}"))
        }
    }
}

/// A minimal self-closing page; the dialog picks up the outcome by polling.
fn callback_page(ok: bool, message: &str) -> Response {
    let title = if ok { "Connected" } else { "Connection failed" };
    let color = if ok { "#16a34a" } else { "#dc2626" };
    let body = format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>{title}</title>
<style>
 body {{ font-family: ui-sans-serif, system-ui, sans-serif; margin: 0;
        display: grid; place-items: center; min-height: 100vh; background: #0a0a0a; color: #fafafa; }}
 main {{ text-align: center; padding: 2rem; max-width: 32rem; }}
 h1 {{ color: {color}; font-size: 1.25rem; margin: 0 0 .5rem; }}
 p {{ color: #a1a1aa; line-height: 1.6; margin: 0; }}
</style></head>
<body><main><h1>{title}</h1><p>{message}</p></main></body></html>"#
    );

    Html(body).into_response()
}

/// `GET /api/oauth/logins/{login_id}`
pub async fn login_status(
    State(state): State<AppState>,
    Path(login_id): Path<String>,
) -> Result<Json<LoginView>> {
    let view = state
        .oauth_logins
        .view(&login_id)
        .await
        .ok_or_else(|| Error::NotFound(format!("login '{login_id}' not found")))?;
    Ok(Json(view))
}

/// `DELETE /api/oauth/logins/{login_id}`
pub async fn cancel_login(
    State(state): State<AppState>,
    Path(login_id): Path<String>,
) -> Result<impl IntoResponse> {
    if !state.oauth_logins.remove(&login_id).await {
        return Err(Error::NotFound(format!("login '{login_id}' not found")));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /api/connections/{id}/oauth-accounts`
pub async fn list_accounts(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
) -> Result<Json<Vec<OAuthAccount>>> {
    Ok(Json(state.oauth_accounts().list(&connection_id).await?))
}

/// `PATCH /api/connections/{id}/oauth-accounts/{account_id}`
pub async fn update_account(
    State(state): State<AppState>,
    Path((connection_id, account_id)): Path<(String, String)>,
    Json(input): Json<UpdateOAuthAccount>,
) -> Result<impl IntoResponse> {
    let account = state
        .oauth_accounts()
        .update(&connection_id, &account_id, input)
        .await?;
    state.invalidate_catalog().await;
    Ok(Json(account))
}

/// `DELETE /api/connections/{id}/oauth-accounts/{account_id}`
pub async fn delete_account(
    State(state): State<AppState>,
    Path((connection_id, account_id)): Path<(String, String)>,
) -> Result<impl IntoResponse> {
    if !state
        .oauth_accounts()
        .delete(&connection_id, &account_id)
        .await?
    {
        return Err(Error::NotFound(format!(
            "oauth account '{account_id}' not found"
        )));
    }
    // Drop any cached token so a re-added account is not served a stale one.
    state.oauth_tokens.invalidate(&account_id).await;
    state.invalidate_catalog().await;
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /api/oauth/presets`
pub async fn list_presets() -> Result<impl IntoResponse> {
    Ok(Json(serde_json::json!({
        "object": "list",
        "data": oauth::presets::presets(),
    })))
}

async fn ensure_connection(state: &AppState, connection_id: &str) -> Result<()> {
    if state.connections().get(connection_id).await?.is_none() {
        return Err(Error::NotFound(format!(
            "connection '{connection_id}' not found"
        )));
    }
    Ok(())
}

fn normalized_label(value: &str) -> Result<String> {
    let label = value.trim();
    if label.is_empty() {
        return Err(Error::BadRequest("account label is required".to_string()));
    }
    Ok(label.to_string())
}
