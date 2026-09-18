//! Bearer-key authentication for `/v1/*` and the `/api/*` admin surface.

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::Response;

use crate::db::repos::api_keys::ApiKey;
use crate::error::{Error, Result};
use crate::limits::BudgetMode;
use crate::policy::KeyPolicy;
use crate::state::AppState;

/// Response header set on requests that exceed a soft budget.
const BUDGET_WARNING_HEADER: &str = "x-router-budget-warning";

/// Methods and headers offered to cross-origin browser callers.
const CORS_METHODS: &str = "GET, POST, PUT, PATCH, DELETE, OPTIONS";
const CORS_HEADERS: &str = "Authorization, Content-Type, Accept";

/// Applies `server.cors_origins` to every response.
///
/// Reading the live config keeps the policy editable from the dashboard: an
/// empty list emits no CORS headers, `"*"` allows any origin, and anything
/// else is an explicit allowlist. Preflight `OPTIONS` requests never reach a
/// route, so they are answered here.
pub async fn cors(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let allowed = state
        .config
        .read()
        .expect("config lock poisoned")
        .server
        .cors_origins
        .clone();
    let origin = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let requested_headers = request
        .headers()
        .get(header::ACCESS_CONTROL_REQUEST_HEADERS)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);

    if request.method() == Method::OPTIONS && origin.is_some() {
        let mut response = Response::new(Body::empty());
        *response.status_mut() = StatusCode::NO_CONTENT;
        apply_cors(
            &mut response,
            &allowed,
            origin.as_deref(),
            requested_headers.as_deref(),
        );
        return response;
    }

    let mut response = next.run(request).await;
    apply_cors(
        &mut response,
        &allowed,
        origin.as_deref(),
        requested_headers.as_deref(),
    );
    response
}

/// Adds the CORS headers a matching origin asked for, if any.
fn apply_cors(
    response: &mut Response,
    allowed: &[String],
    origin: Option<&str>,
    requested_headers: Option<&str>,
) {
    let Some(origin) = origin else {
        return;
    };

    let allow_any = allowed.iter().any(|entry| entry.trim() == "*");
    let matched = allow_any
        || allowed
            .iter()
            .any(|entry| entry.trim().eq_ignore_ascii_case(origin));
    if !matched {
        return;
    }

    let headers = response.headers_mut();
    if allow_any {
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_ORIGIN,
            HeaderValue::from_static("*"),
        );
    } else if let Ok(value) = HeaderValue::from_str(origin) {
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, value);
        headers.append(header::VARY, HeaderValue::from_static("Origin"));
    }
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static(CORS_METHODS),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        requested_headers
            .and_then(|value| HeaderValue::from_str(value).ok())
            .unwrap_or_else(|| HeaderValue::from_static(CORS_HEADERS)),
    );
}

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

/// Guards the `/api/*` admin routes.
///
/// Three credentials are accepted, in order:
/// 1. `server.allow_unauthenticated_admin` disables the check entirely;
/// 2. a bearer matching `server.admin_token` (scripts and CI);
/// 3. a live dashboard session token from `/api/auth/login`.
///
/// With no password and no admin token the documented localhost posture
/// applies; once the listener exposes the network, requests are rejected until
/// a password is set up.
pub async fn require_admin_token(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, Error> {
    let config = state.config_snapshot();
    if config.server.allow_unauthenticated_admin {
        return Ok(next.run(request).await);
    }

    let admin_token = config.server.admin_token();
    let provided = extract_bearer(request.headers());

    if let (Some(token), Some(expected)) = (provided.as_deref(), admin_token)
        && tokens_match(token, expected)
    {
        return Ok(next.run(request).await);
    }

    if state.auth().password_set().await? {
        if let Some(token) = provided.as_deref()
            && state.auth().authenticate(token).await?
        {
            return Ok(next.run(request).await);
        }

        return Err(Error::Unauthorized(if provided.is_some() {
            "invalid or expired session".to_string()
        } else {
            "sign in required".to_string()
        }));
    }

    if admin_token.is_some() {
        return Err(Error::Unauthorized(if provided.is_some() {
            "invalid admin token".to_string()
        } else {
            "missing Authorization: Bearer <admin token> header".to_string()
        }));
    }

    if config.server.exposes_network() {
        return Err(Error::Unauthorized(
            "no dashboard password is set up yet".to_string(),
        ));
    }

    Ok(next.run(request).await)
}

/// Compares two secrets by hashing both sides and folding over every byte, so
/// mismatches are not observable through response timing. `sha2` is already a
/// dependency; no dedicated constant-time-compare crate is needed.
pub(crate) fn tokens_match(provided: &str, expected: &str) -> bool {
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
