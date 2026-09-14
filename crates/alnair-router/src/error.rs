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

    #[error("unknown model reference: {0}")]
    UnknownModel(String),

    #[error("no route available: {0}")]
    NoRoute(String),

    #[error("unsupported provider type: {0} (supported: openai-compatible, anthropic-native)")]
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
        let body = Json(json!({
            "error": {
                "message": self.to_string(),
                "type": self.error_type(),
                "code": status.as_u16(),
            }
        }));
        (status, body).into_response()
    }
}
