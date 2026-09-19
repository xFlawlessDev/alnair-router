//! OAuth 2.0 wire calls: authorize URL, code exchange, refresh, device code.
//!
//! Deliberately generic. The router implements the standard flows and lets the
//! operator supply the client identity, so no provider-specific impersonation
//! is compiled in.

use std::time::Duration;

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::oauth::credential::{AccountInfo, EndpointConfig};
use crate::oauth::pkce;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// A token endpoint response in the fields the standard defines.
#[derive(Debug, Clone, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_in: Option<i64>,
    #[serde(default)]
    pub token_type: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
}

/// A device authorization response (RFC 8628).
#[derive(Debug, Clone, Deserialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    #[serde(default, alias = "verification_url")]
    pub verification_uri: Option<String>,
    #[serde(default)]
    pub expires_in: Option<i64>,
    #[serde(default)]
    pub interval: Option<i64>,
}

/// Outcome of one device-code poll.
pub enum DevicePoll {
    /// The user has not finished authorizing yet.
    Pending,
    /// Authorization completed; the token was issued.
    Authorized(Box<TokenResponse>),
    /// The user refused, or the code expired.
    Failed(String),
}

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

/// Builds the URL the browser is sent to, carrying the PKCE challenge.
pub fn authorize_url(
    endpoints: &EndpointConfig,
    redirect_uri: &str,
    state: &str,
    challenge: &str,
) -> Result<String> {
    let mut url = url::Url::parse(&endpoints.authorize_url).map_err(|error| {
        Error::BadRequest(format!(
            "authorize_url '{}' is not a valid URL: {error}",
            endpoints.authorize_url
        ))
    })?;

    {
        let mut query = url.query_pairs_mut();
        query.append_pair("client_id", &endpoints.client_id);
        query.append_pair("redirect_uri", redirect_uri);
        query.append_pair("response_type", "code");
        query.append_pair("state", state);
        query.append_pair("code_challenge", challenge);
        query.append_pair("code_challenge_method", "S256");
        if !endpoints.scopes.trim().is_empty() {
            query.append_pair("scope", endpoints.scopes.trim());
        }
    }

    Ok(url.into())
}

/// Reads an error body into a short, loggable message.
async fn error_body(response: reqwest::Response) -> String {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let trimmed = body.trim();
    if trimmed.is_empty() {
        format!("HTTP {status}")
    } else {
        // Providers explain themselves in the body; keep it but bound it.
        let clipped: String = trimmed.chars().take(400).collect();
        format!("HTTP {status}: {clipped}")
    }
}

async fn post_form(url: &str, form: Vec<(&str, String)>) -> Result<TokenResponse> {
    let response = client()
        .post(url)
        .header("Accept", "application/json")
        .form(&form)
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|error| Error::Upstream(format!("cannot reach '{url}': {error}")))?;

    if !response.status().is_success() {
        return Err(Error::Upstream(format!(
            "token endpoint rejected the request — {}",
            error_body(response).await
        )));
    }

    response.json::<TokenResponse>().await.map_err(|error| {
        Error::Upstream(format!(
            "token endpoint returned a non-standard body: {error}"
        ))
    })
}

/// Exchanges an authorization code for tokens.
pub async fn exchange_code(
    endpoints: &EndpointConfig,
    code: &str,
    redirect_uri: &str,
    verifier: &str,
) -> Result<TokenResponse> {
    let mut form = vec![
        ("grant_type", "authorization_code".to_string()),
        ("code", code.to_string()),
        ("redirect_uri", redirect_uri.to_string()),
        ("client_id", endpoints.client_id.clone()),
        ("code_verifier", verifier.to_string()),
    ];
    if let Some(secret) = endpoints.client_secret.as_deref() {
        form.push(("client_secret", secret.to_string()));
    }

    post_form(&endpoints.token_url, form).await
}

/// Renews an access token with the refresh grant.
pub async fn refresh(endpoints: &EndpointConfig, refresh_token: &str) -> Result<TokenResponse> {
    let mut form = vec![
        ("grant_type", "refresh_token".to_string()),
        ("refresh_token", refresh_token.to_string()),
        ("client_id", endpoints.client_id.clone()),
    ];
    if let Some(secret) = endpoints.client_secret.as_deref() {
        form.push(("client_secret", secret.to_string()));
    }

    post_form(&endpoints.token_url, form).await
}

/// Starts a device authorization (RFC 8628).
pub async fn start_device_code(endpoints: &EndpointConfig) -> Result<DeviceCodeResponse> {
    let url = endpoints.device_code_url.as_deref().ok_or_else(|| {
        Error::BadRequest("this provider has no device-code endpoint configured".to_string())
    })?;

    let mut form = vec![("client_id", endpoints.client_id.clone())];
    if let Some(secret) = endpoints.client_secret.as_deref() {
        form.push(("client_secret", secret.to_string()));
    }
    if !endpoints.scopes.trim().is_empty() {
        form.push(("scope", endpoints.scopes.trim().to_string()));
    }

    let response = client()
        .post(url)
        .header("Accept", "application/json")
        .form(&form)
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|error| Error::Upstream(format!("cannot reach '{url}': {error}")))?;

    if !response.status().is_success() {
        return Err(Error::Upstream(format!(
            "device authorization was rejected — {}",
            error_body(response).await
        )));
    }

    response
        .json::<DeviceCodeResponse>()
        .await
        .map_err(|error| {
            Error::Upstream(format!(
                "device endpoint returned a non-standard body: {error}"
            ))
        })
}

/// Polls once for the device-code grant.
///
/// `authorization_pending` and `slow_down` are ordinary states, not failures.
pub async fn poll_device_code(endpoints: &EndpointConfig, device_code: &str) -> Result<DevicePoll> {
    let url = endpoints.device_code_url.as_deref().ok_or_else(|| {
        Error::BadRequest("this provider has no device-code endpoint configured".to_string())
    })?;

    let mut form = vec![
        (
            "grant_type",
            "urn:ietf:params:oauth:grant-type:device_code".to_string(),
        ),
        ("device_code", device_code.to_string()),
        ("client_id", endpoints.client_id.clone()),
    ];
    if let Some(secret) = endpoints.client_secret.as_deref() {
        form.push(("client_secret", secret.to_string()));
    }

    let response = client()
        .post(url)
        .header("Accept", "application/json")
        .form(&form)
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|error| Error::Upstream(format!("cannot reach '{url}': {error}")))?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();

    // Providers report the pending states either as a 4xx with an `error` field
    // or as a 200 with the same field; handle both by reading the body first.
    if let Ok(error) = serde_json::from_str::<serde_json::Value>(&body)
        && let Some(code) = error.get("error").and_then(|value| value.as_str())
    {
        return match code {
            "authorization_pending" | "slow_down" => Ok(DevicePoll::Pending),
            "access_denied" => Ok(DevicePoll::Failed(
                "authorization was denied in the browser".to_string(),
            )),
            "expired_token" => Ok(DevicePoll::Failed(
                "the device code expired before it was approved".to_string(),
            )),
            other => Ok(DevicePoll::Failed(other.to_string())),
        };
    }

    if !status.is_success() {
        let clipped: String = body.trim().chars().take(400).collect();
        return Err(Error::Upstream(format!(
            "device poll failed — HTTP {status}: {clipped}"
        )));
    }

    let token = serde_json::from_str::<TokenResponse>(&body).map_err(|error| {
        Error::Upstream(format!("device poll returned a non-standard body: {error}"))
    })?;
    Ok(DevicePoll::Authorized(Box::new(token)))
}

/// Reads display details from a userinfo endpoint. Best-effort: a provider
/// without one, or a failed call, only costs the label on the dashboard.
pub async fn fetch_account_info(
    endpoints: &EndpointConfig,
    access_token: &str,
) -> Option<AccountInfo> {
    let url = endpoints.user_info_url.as_deref()?;

    let response = client()
        .get(url)
        .bearer_auth(access_token)
        .header("Accept", "application/json")
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .ok()?;

    if !response.status().is_success() {
        return None;
    }

    let body = response.json::<serde_json::Value>().await.ok()?;
    let text = |key: &str| {
        body.get(key)
            .and_then(|value| value.as_str())
            .map(str::to_string)
    };

    Some(AccountInfo {
        username: text("username").or_else(|| text("login")),
        email: text("email"),
        name: text("name"),
    })
}

/// A fresh PKCE verifier/challenge pair, for starting a login.
pub fn pkce_pair() -> (String, String) {
    let verifier = pkce::code_verifier();
    let challenge = pkce::code_challenge(&verifier);
    (verifier, challenge)
}
