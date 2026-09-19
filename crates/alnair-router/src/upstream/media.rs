//! Thin HTTP proxying for endpoints the router does not translate.
//!
//! Embeddings, images, audio and video are forwarded to a resolved upstream
//! connection verbatim. They deliberately bypass the chat provider layer (it
//! has no notion of them) and pull in no shared storage/RAG stack — that would
//! drag unrelated dependencies into a standalone package.

use reqwest::header::HeaderMap;

use crate::error::{Error, Result};
use crate::model::ResolvedTarget;

/// A shared HTTP client. Proxying has no per-request configuration.
#[derive(Clone)]
pub struct MediaProxy {
    client: reqwest::Client,
}

impl Default for MediaProxy {
    fn default() -> Self {
        Self::new()
    }
}

impl MediaProxy {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(15))
            .timeout(std::time::Duration::from_secs(600))
            .build()
            .unwrap_or_default();
        Self { client }
    }

    /// Forwards a JSON body to `{base_url}{path}` and returns the raw response.
    pub async fn post_json(
        &self,
        target: &ResolvedTarget,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<reqwest::Response> {
        let url = join_url(&target.base_url, path);
        let request = self
            .client
            .post(&url)
            .json(body)
            .headers(upstream_headers(target)?);

        let response = request
            .send()
            .await
            .map_err(|error| Error::Upstream(format!("{url}: {error}")))?;

        Ok(response)
    }

    /// Forwards a request body stream to `{base_url}{path}` preserving
    /// the caller's `Content-Type` (needed for multipart audio uploads).
    pub async fn post_raw(
        &self,
        target: &ResolvedTarget,
        path: &str,
        content_type: Option<&str>,
        body: Vec<u8>,
        extra_query: Option<&str>,
    ) -> Result<reqwest::Response> {
        let mut url = join_url(&target.base_url, path);
        if let Some(query) = extra_query {
            url = format!("{url}?{query}");
        }

        let mut request = self
            .client
            .post(&url)
            .body(body)
            .headers(upstream_headers(target)?);
        if let Some(content_type) = content_type {
            request = request.header(reqwest::header::CONTENT_TYPE, content_type);
        }

        let response = request
            .send()
            .await
            .map_err(|error| Error::Upstream(format!("{url}: {error}")))?;

        Ok(response)
    }

    /// Forwards a GET (used for async video job polling).
    pub async fn get(&self, target: &ResolvedTarget, path: &str) -> Result<reqwest::Response> {
        let url = join_url(&target.base_url, path);
        let response = self
            .client
            .get(&url)
            .headers(upstream_headers(target)?)
            .send()
            .await
            .map_err(|error| Error::Upstream(format!("{url}: {error}")))?;

        Ok(response)
    }
}

/// Builds auth and custom headers for an upstream call.
fn upstream_headers(target: &ResolvedTarget) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();

    if let Some(api_key) = target.primary_key()
        && !api_key.trim().is_empty()
    {
        // An Anthropic-native upstream defaults to `x-api-key`, but a connection
        // flagged `bearer` (OAuth/subscription session token) must send that
        // token in the Authorization header instead. OpenAI-compatible ones
        // always take a bearer token. Sending the wrong one would 401.
        let (name, value) =
            if target.provider_type == "anthropic-native" && target.auth_style != "bearer" {
                ("x-api-key", api_key.to_string())
            } else {
                ("authorization", format!("Bearer {api_key}"))
            };

        headers.insert(
            reqwest::header::HeaderName::from_bytes(name.as_bytes())
                .map_err(|error| Error::Internal(error.to_string()))?,
            reqwest::header::HeaderValue::from_str(&value)
                .map_err(|error| Error::Internal(error.to_string()))?,
        );
    }

    if target.provider_type == "anthropic-native" {
        headers.insert(
            "anthropic-version",
            reqwest::header::HeaderValue::from_static("2023-06-01"),
        );
    }

    for (name, value) in &target.custom_headers {
        headers.insert(
            reqwest::header::HeaderName::from_bytes(name.as_bytes())
                .map_err(|error| Error::BadRequest(format!("invalid header name: {error}")))?,
            reqwest::header::HeaderValue::from_str(value)
                .map_err(|error| Error::BadRequest(format!("invalid header value: {error}")))?,
        );
    }

    Ok(headers)
}

/// Joins a base URL and path without doubling or dropping the separator.
fn join_url(base_url: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(provider_type: &str, auth_style: &str) -> ResolvedTarget {
        ResolvedTarget {
            connection_id: "c1".to_string(),
            connection_name: "test".to_string(),
            provider_type: provider_type.to_string(),
            base_url: "https://api.example.com/v1".to_string(),
            model: "some-model".to_string(),
            api_keys: vec!["secret-key".to_string()],
            custom_headers: Default::default(),
            connect_timeout_ms: None,
            idle_timeout_ms: None,
            pricing_model: None,
            cache_retention: "none".to_string(),
            auth_style: auth_style.to_string(),
            oauth_account_ids: Vec::new(),
            source: "alias:test".to_string(),
        }
    }

    #[test]
    fn join_url_handles_separators() {
        assert_eq!(
            join_url("https://api.example.com/v1", "/embeddings"),
            "https://api.example.com/v1/embeddings"
        );
        assert_eq!(
            join_url("https://api.example.com/v1/", "embeddings"),
            "https://api.example.com/v1/embeddings"
        );
    }

    #[test]
    fn anthropic_native_sends_x_api_key_by_default() {
        let headers = upstream_headers(&target("anthropic-native", "api_key")).expect("headers");

        assert_eq!(headers.get("x-api-key").unwrap(), "secret-key");
        assert!(headers.get("authorization").is_none());
    }

    #[test]
    fn anthropic_native_bearer_style_moves_the_credential() {
        let headers = upstream_headers(&target("anthropic-native", "bearer")).expect("headers");

        assert_eq!(headers.get("authorization").unwrap(), "Bearer secret-key");
        assert!(headers.get("x-api-key").is_none());
    }

    #[test]
    fn openai_compatible_always_uses_bearer() {
        let headers = upstream_headers(&target("openai-compatible", "api_key")).expect("headers");

        assert_eq!(headers.get("authorization").unwrap(), "Bearer secret-key");
        assert!(headers.get("x-api-key").is_none());
    }
}
