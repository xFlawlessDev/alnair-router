//! Bearer-key authentication for `/v1/*` and the `/api/*` admin surface.

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;

use crate::error::Error;
use crate::state::AppState;

/// The API key that authenticated the current request.
#[derive(Debug, Clone)]
pub struct AuthenticatedKey(pub crate::db::repos::api_keys::ApiKey);

/// Authenticates router-issued API keys.
///
/// When `server.require_api_key` is false the check is skipped, which keeps
/// localhost development frictionless. Either way an
/// `Option<AuthenticatedKey>` extension is always inserted, so handlers can
/// extract it unconditionally.
pub async fn require_api_key(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, Error> {
    if !state.config.server.require_api_key {
        request.extensions_mut().insert(None::<AuthenticatedKey>);
        return Ok(next.run(request).await);
    }

    let secret = extract_bearer(request.headers()).ok_or_else(|| {
        Error::Unauthorized("missing Authorization: Bearer <key> header".to_string())
    })?;

    let key = state
        .api_keys()
        .find_by_secret(&secret)
        .await?
        .ok_or_else(|| Error::Unauthorized("invalid or disabled API key".to_string()))?;

    state.api_keys().touch(&key.id).await?;

    request.extensions_mut().insert(Some(AuthenticatedKey(key)));

    Ok(next.run(request).await)
}

/// Guards the `/api/*` admin routes with `server.admin_token`.
///
/// On loopback without a token the check is skipped, which is the documented
/// localhost posture. Whenever a token is configured it is enforced — including
/// on loopback — so exposing a non-loopback bind cannot accidentally leave the
/// admin API open.
pub async fn require_admin_token(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, Error> {
    if !state.config.server.requires_admin_token() {
        return Ok(next.run(request).await);
    }

    // `requires_admin_token` is true only when a non-blank token is configured.
    let expected = state
        .config
        .server
        .admin_token()
        .ok_or_else(|| Error::Unauthorized("admin token is required".to_string()))?;

    let provided = extract_bearer(request.headers()).ok_or_else(|| {
        Error::Unauthorized("missing Authorization: Bearer <admin token> header".to_string())
    })?;

    if !tokens_match(&provided, expected) {
        return Err(Error::Unauthorized("invalid admin token".to_string()));
    }

    Ok(next.run(request).await)
}

/// Compares two secrets by hashing both sides and folding over every byte, so
/// mismatches are not observable through response timing. `sha2` is already a
/// dependency; no dedicated constant-time-compare crate is needed.
fn tokens_match(provided: &str, expected: &str) -> bool {
    use sha2::{Digest, Sha256};

    let provided = Sha256::digest(provided.as_bytes());
    let expected = Sha256::digest(expected.as_bytes());

    provided
        .iter()
        .zip(expected.iter())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

/// Extracts the bearer token from an `Authorization` header.
fn extract_bearer(headers: &http::HeaderMap) -> Option<String> {
    let value = headers.get(http::header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = token.trim();
    if token.is_empty() {
        return None;
    }
    Some(token.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::HeaderMap;

    fn headers(value: &str) -> HeaderMap {
        let mut map = HeaderMap::new();
        map.insert(
            http::header::AUTHORIZATION,
            value.parse().expect("valid header value"),
        );
        map
    }

    #[test]
    fn bearer_token_is_extracted() {
        assert_eq!(
            extract_bearer(&headers("Bearer sk-router-abc")),
            Some("sk-router-abc".to_string())
        );
    }

    #[test]
    fn scheme_is_case_insensitive() {
        assert_eq!(
            extract_bearer(&headers("bearer sk-router-abc")),
            Some("sk-router-abc".to_string())
        );
    }

    #[test]
    fn non_bearer_schemes_are_rejected() {
        assert_eq!(extract_bearer(&headers("Basic abc")), None);
        assert_eq!(extract_bearer(&headers("Bearer   ")), None);
    }

    #[test]
    fn tokens_match_only_when_identical() {
        assert!(tokens_match("s3cret", "s3cret"));
        assert!(!tokens_match("s3cret", "s3cret2"));
        assert!(!tokens_match("", "s3cret"));
    }
}
