//! Bearer-key authentication for `/v1/*` and the `/api/*` admin surface.

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;

use crate::db::repos::api_keys::ApiKey;
use crate::error::{Error, Result};
use crate::limits::BudgetMode;
use crate::policy::KeyPolicy;
use crate::state::AppState;

/// Response header set on requests that exceed a soft budget.
const BUDGET_WARNING_HEADER: &str = "x-router-budget-warning";

/// The API key that authenticated the current request, with its effective
/// rules (its own fields merged with the attached plan, if any).
#[derive(Debug, Clone)]
pub struct AuthenticatedKey {
    pub key: crate::db::repos::api_keys::ApiKey,
    pub policy: KeyPolicy,
}

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
    let method = request.method().to_string();
    let path = request.uri().path().to_string();
    let started = std::time::Instant::now();

    if !state
        .config
        .read()
        .expect("config lock poisoned")
        .server
        .require_api_key
    {
        request.extensions_mut().insert(None::<AuthenticatedKey>);
        let response = next.run(request).await;
        record_http(&state, &method, &path, response.status(), started);
        return Ok(response);
    }

    let secret = match extract_bearer(request.headers()) {
        Some(secret) => secret,
        None => {
            state.telemetry.record(
                "warn",
                "auth.denied",
                None,
                None,
                format!("{method} {path} — missing Authorization header"),
                None,
                Some(401),
            );
            return Err(Error::Unauthorized(
                "missing Authorization: Bearer <key> header".to_string(),
            ));
        }
    };

    let key = match state.api_keys().find_by_secret(&secret).await? {
        Some(key) => key,
        None => {
            state.telemetry.record(
                "warn",
                "auth.denied",
                None,
                None,
                format!("{method} {path} — invalid or disabled API key"),
                None,
                Some(401),
            );
            return Err(Error::Unauthorized(
                "invalid or disabled API key".to_string(),
            ));
        }
    };

    if key.is_expired() {
        state.telemetry.record(
            "warn",
            "auth.denied",
            None,
            None,
            format!("{method} {path} — API key '{}' has expired", key.name),
            None,
            Some(401),
        );
        return Err(Error::Unauthorized(format!(
            "API key '{}' has expired",
            key.name
        )));
    }

    state.api_keys().touch(&key.id).await?;

    let plan = match &key.plan_id {
        Some(plan_id) => state.key_plans().get(plan_id).await?,
        None => None,
    };

    // An expired plan fails closed: keys attached to it stop working instead of
    // silently falling back to their own fields or the server defaults.
    if let Some(plan) = plan.as_ref()
        && plan.is_expired()
    {
        state.telemetry.record(
            "warn",
            "auth.denied",
            None,
            None,
            format!("{method} {path} — plan '{}' has expired", plan.name),
            None,
            Some(403),
        );
        return Err(Error::Forbidden(format!(
            "the plan '{}' attached to this API key has expired",
            plan.name
        )));
    }

    let policy = KeyPolicy::resolve(&key, plan.as_ref());

    // Cheap in-memory check first, then the budget rollup.
    if let Err(error) = state
        .rate_limiter
        .check(&key.id, policy.rate_limit_per_minute)
    {
        state.metrics.record_rate_limited();
        state.telemetry.record(
            "warn",
            "rate.limited",
            None,
            None,
            format!("{method} {path} — {error}"),
            None,
            Some(429),
        );
        return Err(error);
    }
    let budget_warning = check_budget(&state, &key, &policy).await?;

    request
        .extensions_mut()
        .insert(Some(AuthenticatedKey { key, policy }));

    let mut response = next.run(request).await;
    if let Some(warning) = budget_warning
        && let Ok(value) = axum::http::HeaderValue::from_str(&warning)
    {
        response.headers_mut().insert(
            axum::http::HeaderName::from_static(BUDGET_WARNING_HEADER),
            value,
        );
    }
    record_http(&state, &method, &path, response.status(), started);

    Ok(response)
}

/// Records one finished HTTP request in the activity feed.
fn record_http(
    state: &AppState,
    method: &str,
    path: &str,
    status: axum::http::StatusCode,
    started: std::time::Instant,
) {
    let level = if status.is_server_error() {
        "error"
    } else if status.is_client_error() {
        "warn"
    } else {
        "info"
    };
    state.telemetry.record(
        level,
        "request",
        None,
        None,
        format!("{method} {path}"),
        Some(started.elapsed().as_millis() as u64),
        Some(status.as_u16()),
    );
}

/// Enforces a key's spend caps, one window at a time, in USD and tokens.
///
/// `warn` mode lets the request through and reports every exhausted window via
/// a response header; `block` mode fails it with `402 Payment Required`.
async fn check_budget(
    state: &AppState,
    key: &ApiKey,
    policy: &KeyPolicy,
) -> Result<Option<String>> {
    let mode = policy.budget_mode;
    if mode == BudgetMode::Off || (policy.budgets.is_empty() && policy.token_limits.is_empty()) {
        return Ok(None);
    }

    let now = chrono::Utc::now();
    let mut details: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    for (window, limit) in policy.budgets.iter() {
        let spent = state
            .usage()
            .spend_since(&key.id, window.start(now))
            .await?;
        if spent >= limit {
            details.push(format!(
                "{} budget of ${limit:.2} exhausted for key '{}' (spent ${spent:.2})",
                window.as_str(),
                key.name
            ));
            warnings.push(format!(
                "{} spent ${spent:.2} of ${limit:.2}",
                window.as_str()
            ));
        }
    }

    for (window, limit) in policy.token_limits.iter() {
        let spent = state
            .usage()
            .tokens_since(&key.id, window.start(now))
            .await?;
        if spent >= limit {
            details.push(format!(
                "{} token limit of {limit} tokens exhausted for key '{}' (spent {spent} tokens)",
                window.as_str(),
                key.name
            ));
            warnings.push(format!(
                "{} spent {spent} of {limit} tokens",
                window.as_str()
            ));
        }
    }

    if details.is_empty() {
        return Ok(None);
    }

    match mode {
        BudgetMode::Block => {
            let message = details.join("; ");
            state.metrics.record_budget_blocked();
            state.telemetry.record(
                "warn",
                "budget.blocked",
                None,
                None,
                message.clone(),
                None,
                Some(402),
            );
            Err(Error::BudgetExceeded { message })
        }
        BudgetMode::Warn => Ok(Some(warnings.join("; "))),
        BudgetMode::Off => Ok(None),
    }
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
    // Copy the expected token out of the lock before awaiting anything.
    let expected = {
        let config = state.config.read().expect("config lock poisoned");
        config.server.admin_token().map(str::to_string)
    };

    // A blank or absent token keeps the documented loopback posture.
    let Some(expected) = expected else {
        return Ok(next.run(request).await);
    };

    let provided = extract_bearer(request.headers()).ok_or_else(|| {
        Error::Unauthorized("missing Authorization: Bearer <admin token> header".to_string())
    })?;

    if !tokens_match(&provided, &expected) {
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
pub(crate) fn extract_bearer(headers: &http::HeaderMap) -> Option<String> {
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
