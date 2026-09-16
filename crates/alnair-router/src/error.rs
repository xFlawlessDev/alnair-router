//! Router error type.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("unauthorized: {0}")]
    Unauthorized(String),

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("rate limited: {message}")]
    RateLimited {
        message: String,
        retry_after_secs: u64,
    },

    #[error("budget exceeded: {message}")]
    BudgetExceeded { message: String },

    #[error("unknown model reference: {0}")]
    UnknownModel(String),

    #[error("no route available: {0}")]
    NoRoute(String),

    #[error(
        "unsupported provider type: {0} (supported: openai-compatible, anthropic-native, command-code)"
    )]
    UnsupportedProviderType(String),

    #[error("all upstream attempts failed; last error: {0}")]
    AllAttemptsFailed(String),

    #[error("upstream error: {0}")]
    Upstream(String),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("internal error: {0}")]
    Internal(String),
}

impl Error {
    /// HTTP status this error maps to.
    pub fn status(&self) -> StatusCode {
        match self {
            Error::BadRequest(_) | Error::UnsupportedProviderType(_) => StatusCode::BAD_REQUEST,
            Error::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Error::Forbidden(_) => StatusCode::FORBIDDEN,
            Error::RateLimited { .. } => StatusCode::TOO_MANY_REQUESTS,
            Error::BudgetExceeded { .. } => StatusCode::PAYMENT_REQUIRED,
            // An unresolvable model reference is reported as 404 so callers can
            // distinguish "you asked for something that does not exist" from a
            // malformed request.
            Error::UnknownModel(_) | Error::NotFound(_) => StatusCode::NOT_FOUND,
            Error::NoRoute(_) => StatusCode::BAD_GATEWAY,
            Error::AllAttemptsFailed(_) | Error::Upstream(_) => StatusCode::BAD_GATEWAY,
            Error::Config(_) | Error::Database(_) | Error::Internal(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        }
    }

    /// OpenAI-compatible error `type` string.
    pub fn error_type(&self) -> &'static str {
        match self {
            Error::BadRequest(_) | Error::UnsupportedProviderType(_) => "invalid_request_error",
            Error::Unauthorized(_) => "authentication_error",
            Error::Forbidden(_) => "permission_error",
            Error::RateLimited { .. } => "rate_limit_error",
            Error::BudgetExceeded { .. } => "insufficient_quota",
            Error::UnknownModel(_) | Error::NotFound(_) => "not_found_error",
            Error::NoRoute(_) => "service_unavailable_error",
            Error::AllAttemptsFailed(_) | Error::Upstream(_) => "upstream_error",
            Error::Config(_) | Error::Database(_) | Error::Internal(_) => "internal_error",
        }
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = self.status();
        let retry_after = match &self {
            Error::RateLimited {
                retry_after_secs, ..
            } => Some(*retry_after_secs),
            _ => None,
        };

        let body = Json(json!({
            "error": {
                "message": self.to_string(),
                "type": self.error_type(),
                "code": status.as_u16(),
            }
        }));

        let mut response = (status, body).into_response();
        if let Some(seconds) = retry_after
            && let Ok(value) = axum::http::HeaderValue::from_str(&seconds.to_string())
        {
            response
                .headers_mut()
                .insert(axum::http::header::RETRY_AFTER, value);
        }
        response
    }
}
