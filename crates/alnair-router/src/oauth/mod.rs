//! OAuth provider authentication.
//!
//! A connection can authenticate with an OAuth account instead of a static API
//! key: PKCE loopback or device-code login, an encrypted credential blob, and
//! just-in-time token refresh in the executor.
//!
//! The connector is deliberately generic. The router implements the standard
//! flows (RFC 6749 / RFC 7636 / RFC 8628) and ships **no** first-party client
//! identity — the operator registers their own OAuth application and supplies
//! the client id, secret and endpoints. That is a legal/ToS decision recorded
//! in `docs/ROADMAP.md`, and it is why the provider presets here carry
//! endpoints only.
//!
//! An OAuth connection sends its token with `AuthStyle::Bearer`, supplied by
//! the provider layer through `upstream/chat_backend.rs`.

pub mod credential;
pub mod flows;
pub mod logins;
pub mod pkce;
pub mod presets;
pub mod token_cache;

#[cfg(test)]
mod tests;

pub use credential::{AccountInfo, EndpointConfig, OAuthCredential};
pub use logins::{DeviceChallenge, LoginRegistry, LoginState, LoginView, PendingLogin};
pub use presets::{ClientInputs, OAuthPreset};
pub use token_cache::OAuthTokenCache;

/// The path a browser is redirected back to, appended to the router's own
/// address. Operators register this as the redirect URI of their OAuth app.
pub const CALLBACK_PATH: &str = "/api/oauth/callback";

/// Builds the redirect URI for a request that arrived at `host`.
pub fn redirect_uri(host: &str) -> String {
    format!("http://{host}{CALLBACK_PATH}")
}
