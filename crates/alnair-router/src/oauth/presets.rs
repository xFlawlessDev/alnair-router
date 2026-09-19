//! Endpoint presets for OAuth providers.
//!
//! A preset fills in the endpoints and scopes; it deliberately carries **no
//! client id or secret**. The operator registers their own OAuth application
//! and supplies those, which is what keeps this router from shipping a borrowed
//! identity. `gitlab-duo` works this way upstream too, where the client
//! credentials come from the user rather than the provider's own CLI.
//!
//! The Google preset is endpoints only and the client id is left empty on
//! purpose: the CLI client id that circulates for Gemini is first-party and the
//! reference project flags that provider as risky. Nothing borrowed is
//! compiled in here.

use serde::Serialize;

use crate::oauth::credential::EndpointConfig;

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
    pub docs_url: Option<&'static str>,
    pub note: Option<&'static str>,
}

/// Every preset, plus the blank generic entry the picker always offers.
pub fn presets() -> Vec<OAuthPreset> {
    vec![
        OAuthPreset {
            id: "gitlab-duo",
            label: "GitLab Duo",
            authorize_url: "https://gitlab.com/oauth/authorize",
            token_url: "https://gitlab.com/oauth/token",
            device_code_url: None,
            user_info_url: Some("https://gitlab.com/api/v4/user"),
            scopes: "api read_user",
            public_client: false,
            docs_url: Some("https://docs.gitlab.com/ee/integration/oauth_provider.html"),
            note: Some("Self-hosted GitLab works too — replace the host in all three URLs."),
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
            docs_url: Some("https://console.cloud.google.com/apis/credentials"),
            note: Some("Endpoints only: create your own OAuth client and paste its id and secret."),
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
            docs_url: None,
            note: Some(
                "Any standard authorization-code or device-code provider. Fill in every URL yourself.",
            ),
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
        })
    }
}
