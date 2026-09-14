//! Bearer-key authentication for `/v1/*` routes.

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
}
