//! ChatGPT/Codex provider: the Codex backend's Responses dialect.
//!
//! The backend at `https://chatgpt.com/backend-api/codex` is not the public
//! Responses API. Three contracts set it apart, all ported from Alnair's own
//! Codex client (`alnair-sampling-types::adapt_body_for_codex_backend` and
//! `alnair-sampler::client`), which in turn follows the official code CLI:
//!
//! 1. `role: system` input items are rejected. The system channel is the
//!    top-level `instructions` string, so system messages are hoisted there and
//!    removed from `input`.
//! 2. `store` is always `false`, so reasoning continuity is stateless: the
//!    request asks for `include: ["reasoning.encrypted_content"]`.
//! 3. The request carries the Codex client identity (`originator`,
//!    `openai-beta`, `User-Agent`) and the `chatgpt-account-id` claim of the
//!    bearer JWT. Without the account id the backend answers 401.
//!
//! The access token itself travels as a plain `Authorization: Bearer`, which is
//! why a Codex connection is an ordinary OAuth connection in the router.

mod request;
mod stream;

#[cfg(test)]
mod tests;

use futures::StreamExt;
use futures::stream::BoxStream;

use crate::model_config::{LlmStreamOptions, ModelConfig};
use crate::provider::LlmProvider;
use crate::providers::common::normalize_base_url;
use crate::types::{ChatError, LlmStreamChunk, Message};

/// `originator` header identifying the Codex client.
pub const CODEX_ORIGINATOR: &str = "codex_cli_rs";

/// `OpenAI-Beta` opt-in the Codex Responses endpoint requires.
pub const CODEX_OPENAI_BETA: &str = "responses=experimental";

/// `User-Agent` presented on the Codex path. The backend does not strictly
/// validate it, so this is a stable identity rather than a pinned build.
pub const CODEX_USER_AGENT: &str = "codex_cli_rs/0.159.0";

/// Last path segment the provider appends to the configured base URL.
///
/// The base URL already carries the `/codex` tail
/// (`.../backend-api/codex`), so the endpoint is `{base}/responses`.
const RESPONSES_PATH: &str = "/responses";

pub struct CodexProvider {
    client: reqwest::Client,
}

impl CodexProvider {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

impl Default for CodexProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl LlmProvider for CodexProvider {
    fn stream<'a>(
        &'a self,
        config: &'a ModelConfig,
        messages: Vec<Message>,
        options: &'a LlmStreamOptions,
        tools: Option<Vec<serde_json::Value>>,
    ) -> BoxStream<'a, Result<LlmStreamChunk, ChatError>> {
        let endpoint = format!("{}{RESPONSES_PATH}", normalize_base_url(&config.base_url));
        let bearer = config.api_key.clone();
        let account_id = config.chatgpt_account_id.clone();
        let custom_headers = config.custom_headers.clone();
        let rates = config.effective_cost_rates();
        let body = request::build_request_body(config, &messages, options, tools.as_deref(), true);
        let client = self.client.clone();

        Box::pin(async_stream::stream! {
            let Some(bearer) = bearer.as_deref().filter(|key| !key.trim().is_empty()) else {
                yield Err(ChatError::BadRequest(
                    "a ChatGPT/Codex connection needs a signed-in OAuth account".to_string(),
                ));
                return;
            };

            let response = match request::send(&client, &endpoint, bearer, account_id.as_deref(), &custom_headers, &body, options).await {
                Ok(response) => response,
                Err(error) => {
                    yield Err(error);
                    return;
                }
            };

            if !response.status().is_success() {
                yield Err(request::failure(response).await);
                return;
            }

            let chunks = stream::read_sse(response, rates);
            futures::pin_mut!(chunks);
            while let Some(chunk) = chunks.next().await {
                yield chunk;
            }
        })
    }

    /// One-shot call with `stream: false`; the backend answers one JSON body.
    fn complete<'a>(
        &'a self,
        config: &'a ModelConfig,
        messages: Vec<Message>,
        options: &'a LlmStreamOptions,
        tools: Option<Vec<serde_json::Value>>,
    ) -> BoxStream<'a, Result<LlmStreamChunk, ChatError>> {
        let endpoint = format!("{}{RESPONSES_PATH}", normalize_base_url(&config.base_url));
        let bearer = config.api_key.clone();
        let account_id = config.chatgpt_account_id.clone();
        let custom_headers = config.custom_headers.clone();
        let rates = config.effective_cost_rates();
        let body = request::build_request_body(config, &messages, options, tools.as_deref(), false);
        let client = self.client.clone();

        Box::pin(async_stream::stream! {
            let Some(bearer) = bearer.as_deref().filter(|key| !key.trim().is_empty()) else {
                yield Err(ChatError::BadRequest(
                    "a ChatGPT/Codex connection needs a signed-in OAuth account".to_string(),
                ));
                return;
            };

            let response = match request::send(&client, &endpoint, bearer, account_id.as_deref(), &custom_headers, &body, options).await {
                Ok(response) => response,
                Err(error) => {
                    yield Err(error);
                    return;
                }
            };

            if !response.status().is_success() {
                yield Err(request::failure(response).await);
                return;
            }

            let text = response.text().await.unwrap_or_default();
            let json: serde_json::Value = match serde_json::from_str(&text) {
                Ok(json) => json,
                Err(_) => {
                    yield Err(ChatError::Provider(format!(
                        "Codex returned a non-JSON response: {}",
                        crate::providers::common::describe_non_json_body(&text)
                    )));
                    return;
                }
            };

            for chunk in stream::completion_chunks(&json, rates.as_ref()) {
                yield Ok(chunk);
            }
        })
    }
}
