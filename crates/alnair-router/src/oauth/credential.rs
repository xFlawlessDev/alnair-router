//! The credential document stored (encrypted) for one OAuth account.
//!
//! Access token, refresh token and the endpoints needed to refresh it travel
//! together as a single JSON blob so a rotation updates them as a unit. The
//! client id/secret are the operator's own registration — the router ships no
//! first-party identity, with one documented exception: the ChatGPT/Codex
//! preset, whose OAuth application is OpenAI's own CLI client (see
//! `presets::presets`).

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// How the token endpoint expects its body.
///
/// The authorization-code providers registered in the wild disagree: GitLab and
/// Google take form encoding, while the ChatGPT/Codex endpoint takes form and
/// refuses an extra `state` field. The default keeps every existing credential
/// on the form wire it was minted against.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenBody {
    /// `application/x-www-form-urlencoded` (the OAuth norm).
    #[default]
    Form,
    /// `application/json`, which some providers accept instead.
    Json,
}

/// The OAuth endpoints and client identity a login was performed against.
///
/// Kept alongside the tokens so a refresh works after a restart without
/// re-asking the operator for anything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointConfig {
    pub client_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    pub authorize_url: String,
    pub token_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_code_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_info_url: Option<String>,
    #[serde(default)]
    pub scopes: String,
    /// Extra query parameters appended only to the browser authorize URL.
    ///
    /// The ChatGPT/Codex login needs three of them
    /// (`id_token_add_organizations`, `codex_cli_simplified_flow`,
    /// `originator`); every other provider leaves this empty. Never contains a
    /// secret — they are wire constants the dashboard may show.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authorize_extra: Vec<(String, String)>,
    /// Body encoding of the token endpoint.
    #[serde(default)]
    pub token_body: TokenBody,
}

impl EndpointConfig {
    /// True when the token endpoint is form-encoded.
    pub fn uses_form_body(&self) -> bool {
        matches!(self.token_body, TokenBody::Form)
    }
}

/// Display-only account details read from the provider's userinfo endpoint.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Everything needed to authenticate a request and to renew the credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthCredential {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// When the access token stops working; `None` when the provider omits it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    pub endpoints: EndpointConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<AccountInfo>,
    /// The `chatgpt_account_id` claim of a ChatGPT/Codex access token.
    ///
    /// Recorded at login and after every refresh so a credential whose token
    /// lost the claim is rejected while it is being stored, rather than
    /// surfacing as an opaque 401 mid-conversation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chatgpt_account_id: Option<String>,
}

impl OAuthCredential {
    /// True when the access token is missing, expired, or expires within `lead`.
    ///
    /// A credential with no known expiry is never refreshed on a timer; a token
    /// that turns out to be dead is caught by the request falling through.
    pub fn needs_refresh(&self, now: DateTime<Utc>, lead: Duration) -> bool {
        match self.expires_at {
            Some(expiry) => expiry - lead <= now,
            None => false,
        }
    }

    /// Folds a fresh token response in, preserving the endpoints and the
    /// display details gathered at login.
    ///
    /// A provider that omits `refresh_token` on renewal means "keep the one you
    /// have", per RFC 6749 §6.
    pub fn apply(&mut self, renewed: crate::oauth::flows::TokenResponse, now: DateTime<Utc>) {
        self.access_token = renewed.access_token;
        if renewed.refresh_token.is_some() {
            self.refresh_token = renewed.refresh_token;
        }
        if let Some(token_type) = renewed.token_type {
            self.token_type = Some(token_type);
        }
        if let Some(scope) = renewed.scope {
            self.scope = Some(scope);
        }
        self.expires_at = renewed
            .expires_in
            .map(|seconds| now + Duration::seconds(seconds));
    }
}
