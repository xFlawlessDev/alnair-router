//! Endpoint presets for OAuth providers.
//!
//! A preset fills in the endpoints, scopes and authorize-only parameters;
//! `openai-codex` is the one preset that also carries a client id. The
//! ChatGPT/Codex backend only authorizes OpenAI's own CLI application, so a
//! "bring your own client" login cannot reach it — the router either ships that
//! identity or does not support Codex at all. Every other preset leaves the
//! client id to the operator, which is what keeps the router from borrowing a
//! first-party identity it has no right to. The Google preset is endpoints only
//! for the same reason: the CLI client id that circulates for Gemini is
//! first-party and the reference project flags that provider as risky.

use serde::Serialize;

use crate::oauth::credential::{EndpointConfig, TokenBody};

/// The ChatGPT/Codex OAuth application.
///
/// Ported from Alnair's Codex client (`alnair-models::CODEX_OAUTH_CONFIG`),
/// which follows OpenAI's official Codex CLI. Public PKCE client: no secret is
/// involved, and the loopback redirect port is fixed because OpenAI registers
/// it per client.
pub const CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";

/// Port OpenAI expects the Codex callback on.
pub const CODEX_REDIRECT_PORT: u16 = 1455;

/// Path OpenAI expects the Codex callback on.
pub const CODEX_REDIRECT_PATH: &str = "/auth/callback";

/// Parameters that make the authorize URL a Codex CLI login rather than a plain
/// OpenAI consent page.
pub const CODEX_AUTHORIZE_EXTRA: [(&str, &str); 3] = [
    ("id_token_add_organizations", "true"),
    ("codex_cli_simplified_flow", "true"),
    ("originator", "codex_cli_rs"),
];

#[derive(Debug, Clone, Serialize)]
pub struct OAuthPreset {
    pub id: &'static str,
    pub label: &'static str,
    pub authorize_url: &'static str,
    pub token_url: &'static str,
    pub device_code_url: Option<&'static str>,
    pub user_info_url: Option<&'static str>,
    pub scopes: &'static str,
    /// True when the provider issues tokens without a client secret (a public
    /// PKCE client), so the dashboard does not ask for one.
    pub public_client: bool,
    /// A client id the preset pins, when the provider only authorizes its own
    /// application. Empty for every bring-your-own-client preset.
    pub client_id: &'static str,
    /// Token-endpoint body encoding the preset requires.
    pub token_body: TokenBody,
    /// Extra authorize query parameters the preset sets.
    pub authorize_extra: Vec<(&'static str, &'static str)>,
    /// Redirect port the provider registered, when it is fixed.
    pub redirect_port: Option<u16>,
    /// The provider's upstream wire family, when the preset implies one.
    pub provider_type: Option<&'static str>,
    pub docs_url: Option<&'static str>,
    pub note: Option<&'static str>,
    /// A warning the dashboard must show before the login starts.
    pub risk_notice: Option<&'static str>,
}

/// Every preset, plus the blank generic entry the picker always offers.
pub fn presets() -> Vec<OAuthPreset> {
    vec![
        OAuthPreset {
            id: "openai-codex",
            label: "ChatGPT Plus/Pro (Codex)",
            authorize_url: "https://auth.openai.com/oauth/authorize",
            token_url: "https://auth.openai.com/oauth/token",
            device_code_url: None,
            user_info_url: None,
            scopes: "openid profile email offline_access",
            public_client: true,
            client_id: CODEX_CLIENT_ID,
            token_body: TokenBody::Form,
            authorize_extra: CODEX_AUTHORIZE_EXTRA.to_vec(),
            redirect_port: Some(CODEX_REDIRECT_PORT),
            provider_type: Some("openai-codex"),
            docs_url: Some("https://platform.openai.com/docs"),
            note: Some(
                "Signs in with your ChatGPT subscription and serves the Codex models through the Codex backend.",
            ),
            risk_notice: Some(
                "This login uses OpenAI's own Codex CLI client, because the Codex backend \
                 authorizes no other application. A ChatGPT subscription is licensed for \
                 OpenAI's clients, so use this only for your own account and review the \
                 terms you agreed to.",
            ),
        },
        OAuthPreset {
            id: "gitlab-duo",
            label: "GitLab Duo",
            authorize_url: "https://gitlab.com/oauth/authorize",
            token_url: "https://gitlab.com/oauth/token",
            device_code_url: None,
            user_info_url: Some("https://gitlab.com/api/v4/user"),
            scopes: "api read_user",
            public_client: false,
            client_id: "",
            token_body: TokenBody::Form,
            authorize_extra: Vec::new(),
            redirect_port: None,
            provider_type: None,
            docs_url: Some("https://docs.gitlab.com/ee/integration/oauth_provider.html"),
            note: Some("Self-hosted GitLab works too — replace the host in all three URLs."),
            risk_notice: None,
        },
        OAuthPreset {
            id: "google",
            label: "Google",
            authorize_url: "https://accounts.google.com/o/oauth2/v2/auth",
            token_url: "https://oauth2.googleapis.com/token",
            device_code_url: Some("https://oauth2.googleapis.com/device/code"),
            user_info_url: Some("https://www.googleapis.com/oauth2/v1/userinfo"),
            scopes: "openid email profile",
            public_client: false,
            client_id: "",
            token_body: TokenBody::Form,
            authorize_extra: Vec::new(),
            redirect_port: None,
            provider_type: None,
            docs_url: Some("https://console.cloud.google.com/apis/credentials"),
            note: Some("Endpoints only: create your own OAuth client and paste its id and secret."),
            risk_notice: None,
        },
        OAuthPreset {
            id: "generic",
            label: "Generic OAuth 2.0",
            authorize_url: "",
            token_url: "",
            device_code_url: None,
            user_info_url: None,
            scopes: "",
            public_client: false,
            client_id: "",
            token_body: TokenBody::Form,
            authorize_extra: Vec::new(),
            redirect_port: None,
            provider_type: None,
            docs_url: None,
            note: Some(
                "Any standard authorization-code or device-code provider. Fill in every URL yourself.",
            ),
            risk_notice: None,
        },
    ]
}

/// Looks up one preset by id, falling back to the generic entry.
pub fn find(id: &str) -> OAuthPreset {
    let id = id.trim().to_ascii_lowercase();
    presets()
        .into_iter()
        .find(|preset| preset.id == id)
        .unwrap_or_else(|| {
            presets()
                .into_iter()
                .find(|preset| preset.id == "generic")
                .expect("generic preset exists")
        })
}

/// True when this preset pins its own client identity, so the dashboard must
/// not ask the operator for one.
pub fn pins_client(preset: &OAuthPreset) -> bool {
    !preset.client_id.is_empty()
}

/// The operator-supplied half of a login: their own client registration.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ClientInputs {
    pub client_id: String,
    #[serde(default)]
    pub client_secret: Option<String>,
    pub authorize_url: String,
    pub token_url: String,
    #[serde(default)]
    pub device_code_url: Option<String>,
    #[serde(default)]
    pub user_info_url: Option<String>,
    #[serde(default)]
    pub scopes: String,
}

impl ClientInputs {
    /// Validates and converts into the stored endpoint document.
    ///
    /// `token_url` is always required. `authorize_url` is only needed for the
    /// authorization-code flow, so a device-only provider does not have to
    /// invent one — the login handler checks it when it actually starts a PKCE
    /// session.
    pub fn into_endpoints(self) -> crate::error::Result<EndpointConfig> {
        use crate::error::Error;

        let client_id = self.client_id.trim();
        if client_id.is_empty() {
            return Err(Error::BadRequest(
                "client_id is required — register an OAuth application with the provider and paste its id"
                    .to_string(),
            ));
        }

        let token_url = self.token_url.trim();
        if token_url.is_empty() {
            return Err(Error::BadRequest(
                "token_url is required — it is where the code is exchanged and where refreshes go"
                    .to_string(),
            ));
        }

        let trim = |value: Option<String>| {
            value
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        };

        Ok(EndpointConfig {
            client_id: client_id.to_string(),
            client_secret: trim(self.client_secret),
            authorize_url: self.authorize_url.trim().to_string(),
            token_url: token_url.to_string(),
            device_code_url: trim(self.device_code_url),
            user_info_url: trim(self.user_info_url),
            scopes: self.scopes.trim().to_string(),
            authorize_extra: Vec::new(),
            token_body: TokenBody::Form,
        })
    }

    /// Fills in the provider's fixed parts, then validates.
    ///
    /// A preset that pins its own client id (ChatGPT/Codex) overrides whatever
    /// the caller sent, because the provider authorizes no other application.
    /// Its authorize parameters and token encoding come from the preset too, so
    /// the dashboard never has to reproduce them.
    pub fn into_endpoints_for(mut self, preset_id: &str) -> crate::error::Result<EndpointConfig> {
        let preset = find(preset_id);
        if pins_client(&preset) {
            self.client_id = preset.client_id.to_string();
            self.client_secret = None;
        }
        // A named preset owns its endpoints, so the dashboard never has to
        // reproduce them. Only the generic entry takes operator-supplied URLs.
        if preset.id != "generic" {
            self.authorize_url = preset.authorize_url.to_string();
            self.token_url = preset.token_url.to_string();
            self.device_code_url = preset.device_code_url.map(str::to_string);
            self.user_info_url = preset.user_info_url.map(str::to_string);
            self.scopes = preset.scopes.to_string();
        }

        let mut endpoints = self.into_endpoints()?;
        if preset.id != "generic" {
            endpoints.authorize_extra = preset
                .authorize_extra
                .iter()
                .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
                .collect();
            endpoints.token_body = preset.token_body;
        }
        Ok(endpoints)
    }
}
