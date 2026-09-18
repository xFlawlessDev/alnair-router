//! `POST /api/admin/control/shutdown` — stop the router from the CLI.
//!
//! `stop` needs a graceful shutdown that works no matter how the admin API is
//! configured: `server.admin_token` may be unset and the dashboard password may
//! not exist yet, so the admin guard would accept a request from any local
//! process. This route therefore has its own credential — the 32-byte token in
//! `$ALNAIR_ROUTER_HOME/control.token`, which only the account owning the
//! router home can read. A browser page cannot read that file, so a hostile
//! site cannot forge the request even though it is reachable on loopback.

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use serde_json::{Value, json};

use crate::cli::daemon;
use crate::error::{Error, Result};
use crate::middleware;
use crate::state::AppState;

/// `POST /api/admin/control/shutdown` — stop the process gracefully.
pub async fn shutdown(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<Value>)> {
    authorize(&headers, daemon::read_control_token().as_deref())?;

    if !state.request_shutdown() {
        return Err(Error::Internal(
            "the server is not accepting shutdown requests yet".to_string(),
        ));
    }

    // This response still reaches the caller: `with_graceful_shutdown` stops
    // accepting new work and drains what is in flight rather than dropping it.
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "status": "shutting down" })),
    ))
}

/// Rejects anything that does not present the local control token.
fn authorize(headers: &HeaderMap, expected: Option<&str>) -> Result<()> {
    let expected = expected
        .filter(|token| !token.is_empty())
        .ok_or_else(|| Error::Forbidden("process control is unavailable".to_string()))?;

    let provided = headers
        .get(daemon::CONTROL_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty());

    match provided {
        Some(provided) if middleware::tokens_match(provided, expected) => Ok(()),
        Some(_) => Err(Error::Unauthorized("invalid control token".to_string())),
        None => Err(Error::Unauthorized(format!(
            "missing {} header",
            daemon::CONTROL_HEADER
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(value: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(value) = value {
            headers.insert(
                axum::http::HeaderName::from_static(daemon::CONTROL_HEADER),
                axum::http::HeaderValue::from_str(value).expect("header"),
            );
        }
        headers
    }

    #[test]
    fn the_control_header_is_stable() {
        assert_eq!(daemon::CONTROL_HEADER, "x-alnair-control");
    }

    #[test]
    fn the_matching_token_is_accepted() {
        assert!(authorize(&headers(Some("s3cret")), Some("s3cret")).is_ok());
    }

    #[test]
    fn a_missing_token_is_unauthorized() {
        let error = authorize(&headers(None), Some("s3cret")).expect_err("must refuse");

        assert!(matches!(error, Error::Unauthorized(_)));
        assert_eq!(error.status(), StatusCode::UNAUTHORIZED);
        assert!(error.to_string().contains(daemon::CONTROL_HEADER));
    }

    #[test]
    fn a_wrong_or_blank_token_is_unauthorized() {
        for provided in ["nope", "   "] {
            let error =
                authorize(&headers(Some(provided)), Some("s3cret")).expect_err("must refuse");
            assert!(matches!(error, Error::Unauthorized(_)), "{provided}");
        }
    }

    #[test]
    fn the_route_is_forbidden_while_no_token_file_exists() {
        let error = authorize(&headers(Some("s3cret")), None).expect_err("must refuse");

        assert!(matches!(error, Error::Forbidden(_)));
        assert_eq!(error.status(), StatusCode::FORBIDDEN);
    }
}
