use crate::{
    error::{Error, Result},
    middleware::AuthenticatedKey,
    model::ResolvedTarget,
    state::AppState,
    upstream::chat_backend::{self, WebRequest},
};
use axum::response::Response;
use std::time::Duration;

pub(super) async fn dispatch(
    state: &AppState,
    key: &Option<AuthenticatedKey>,
    model: &Option<String>,
    target: &ResolvedTarget,
    operation: WebRequest,
) -> Result<Response> {
    let context = super::ProxyContext::new(state.clone(), key, model);
    let _permit = state.limiter.acquire(&target.connection_id).await?;
    let started = std::time::Instant::now();
    let result = tokio::time::timeout(Duration::from_secs(90), async {
        let token = match target.oauth_account_ids.first() {
            Some(account) => Some(state.oauth_tokens.access_token(account).await?),
            None => target.primary_key().map(str::to_string),
        };
        chat_backend::hosted_web(target, token.as_deref(), &operation).await
    })
    .await
    .unwrap_or_else(|_| Err(Error::Upstream("hosted web request timed out".to_string())));
    context
        .record(
            target,
            if result.is_ok() { "ok" } else { "error" },
            started.elapsed().as_millis() as u64,
        )
        .await;
    let result = result?;
    let result = if matches!(operation, WebRequest::Search { .. }) {
        super::search::normalize_search_response(&result, Some(&target.provider_type))
            .unwrap_or(result)
    } else {
        result
    };
    let body = serde_json::to_vec(&result).map_err(|error| Error::Internal(error.to_string()))?;
    super::raw_json_response(body)
}
