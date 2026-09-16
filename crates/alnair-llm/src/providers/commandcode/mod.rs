//! Command Code upstream, over two transports.
//!
//! - The documented Provider API (`/provider/v1/chat/completions`), used when
//!   the credential is entitled to it, and
//! - the CLI envelope (`/alpha/generate`), used when the Provider API answers
//!   `403` (the Go plan has no API access) or `404`.
//!
//! Which transport applies is probed once per base URL and credential against
//! `/provider/v1/models`, then remembered for the process, so a Go-plan
//! connection does not pay a rejected request on every call. Mirrors 9Router's
//! `commandcode` registry entry and OmniRoute's `commandCode` executor.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;
use std::time::Duration;

use bytes::Bytes;
use futures::StreamExt;
use futures::stream::BoxStream;
use tracing::{debug, warn};

use crate::model_config::{LlmStreamOptions, ModelConfig, ModelCostRates, apply_custom_headers};
use crate::provider::LlmProvider;
use crate::providers::common::{
    calculate_token_costs, is_retryable_status, normalize_base_url, retry_after_delay, retry_delay,
};
use crate::providers::openai::OpenAiProvider;
use crate::providers::sse::SseLineBuffer;
use crate::types::{ChatError, LlmStreamChunk, Message};

mod request;
mod stream;

#[cfg(test)]
mod tests;

use request::{build_cli_body, cli_headers, official_options, wire_model};
use stream::{CliEvent, UsageTotals, decode_line, event_from};

/// Upper bound for the one-off entitlement probe.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Path the Provider API lives under, relative to the connection's base URL.
const PROVIDER_PATH: &str = "/provider/v1";

/// Path of the CLI transport.
const CLI_PATH: &str = "/alpha/generate";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Transport {
    Official,
    Cli,
}

pub struct CommandCodeProvider {
    client: reqwest::Client,
    /// Reused for the Provider API, which speaks plain OpenAI.
    openai: OpenAiProvider,
    /// `base URL | credential` → the transport that credential may use.
    transports: Mutex<HashMap<String, Transport>>,
}

impl CommandCodeProvider {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            openai: OpenAiProvider::new(),
            transports: Mutex::new(HashMap::new()),
        }
    }

    /// Resolves the transport for this credential, probing at most once.
    async fn transport_for(&self, base_url: &str, api_key: Option<&str>) -> Transport {
        let memo_key = memo_key(base_url, api_key);

        if let Some(known) = self.cached_transport(&memo_key) {
            return known;
        }

        let resolved = self.probe(base_url, api_key).await;
        self.remember(&memo_key, resolved);
        resolved
    }

    fn cached_transport(&self, memo_key: &str) -> Option<Transport> {
        self.transports
            .lock()
            .ok()
            .and_then(|transports| transports.get(memo_key).copied())
    }

    fn remember(&self, memo_key: &str, transport: Transport) {
        if let Ok(mut transports) = self.transports.lock() {
            transports.insert(memo_key.to_string(), transport);
        }
    }

    /// Remembers that this credential has to use the CLI transport. Called when
    /// the Provider API rejects a real request, which is the only authoritative
    /// signal: the models endpoint is not gated the same way on every plan.
    fn remember_cli(&self, base_url: &str, api_key: Option<&str>) {
        self.remember(&memo_key(base_url, api_key), Transport::Cli);
    }

    /// Asks the documented models endpoint whether this key may use the API.
    async fn probe(&self, base_url: &str, api_key: Option<&str>) -> Transport {
        let endpoint = format!("{base_url}{PROVIDER_PATH}/models");
        let mut request = self.client.get(&endpoint).timeout(PROBE_TIMEOUT);
        if let Some(key) = api_key {
            request = request.bearer_auth(key);
        }

        match request.send().await {
            Ok(response) if matches!(response.status().as_u16(), 403 | 404) => {
                debug!(
                    endpoint = %endpoint,
                    status = %response.status(),
                    "Command Code Provider API unavailable for this credential; using the CLI transport"
                );
                Transport::Cli
            }
            Ok(response) => {
                debug!(
                    endpoint = %endpoint,
                    status = %response.status(),
                    "Command Code Provider API transport selected"
                );
                Transport::Official
            }
            Err(error) => {
                warn!(
                    error = %error,
                    endpoint = %endpoint,
                    "Command Code entitlement probe failed; trying the Provider API"
                );
                Transport::Official
            }
        }
    }

    /// CLI transport: one NDJSON stream inside the CLI's session envelope.
    fn cli_stream<'a>(
        &'a self,
        base_url: &str,
        config: &'a ModelConfig,
        messages: Vec<Message>,
        options: &'a LlmStreamOptions,
        tools: Option<Vec<serde_json::Value>>,
    ) -> BoxStream<'a, Result<LlmStreamChunk, ChatError>> {
        let endpoint = format!("{base_url}{CLI_PATH}");
        let api_key = config.api_key.clone();
        let custom_headers = config.custom_headers.clone();
        let rates = config.effective_cost_rates();
        let headers = cli_headers();
        let body = build_cli_body(config, &messages, options, tools.as_deref());

        Box::pin(async_stream::stream! {
            let (request_body, names) = body;

            let response = match send_cli_request_with_retry(
                &self.client,
                &endpoint,
                api_key.as_deref(),
                &headers,
                &custom_headers,
                &request_body,
                options,
            )
            .await
            {
                Ok(response) => response,
                Err(error) => {
                    warn!(error = %error, endpoint = %endpoint, "Command Code CLI request failed after retries");
                    yield Err(error);
                    return;
                }
            };

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                yield Err(ChatError::Provider(format!(
                    "Command Code CLI error {status}: {}",
                    truncate_for_log(&body, 500)
                )));
                return;
            }

            let mut lines = SseLineBuffer::default();
            let mut usage = UsageTotals::default();
            let mut stream = response.bytes_stream();

            loop {
                let (bytes, is_last): (Option<Bytes>, bool) = match stream.next().await {
                    Some(Ok(bytes)) => (Some(bytes), false),
                    Some(Err(error)) => {
                        yield Err(ChatError::Provider(error.to_string()));
                        return;
                    }
                    None => (None, true),
                };

                let mut batch = match &bytes {
                    Some(bytes) => lines.push(bytes),
                    None => Vec::new(),
                };
                if is_last && let Some(last) = lines.finish() {
                    batch.push(last);
                }

                for line in batch {
                    let Some(value) = decode_line(&line) else {
                        continue;
                    };

                    match event_from(&value, &mut usage) {
                        CliEvent::Text(text) => {
                            if !text.is_empty() {
                                yield Ok(LlmStreamChunk::Text(text));
                            }
                        }
                        CliEvent::Reasoning(text) => {
                            if !text.is_empty() {
                                yield Ok(LlmStreamChunk::Thinking(text));
                            }
                        }
                        CliEvent::ToolCall { id, name, input } => {
                            let name = names.client_name(&name);
                            if name.is_empty() {
                                warn!(id = %id, "dropping Command Code tool call without a name");
                                continue;
                            }
                            yield Ok(LlmStreamChunk::ToolCall {
                                id,
                                name,
                                arguments: input,
                            });
                        }
                        CliEvent::Finish(reason) => {
                            if let Some(chunk) = usage_chunk(&usage, rates.as_ref()) {
                                yield Ok(chunk);
                            }
                            yield Ok(LlmStreamChunk::Done(Some(reason)));
                            return;
                        }
                        CliEvent::Error(message) => {
                            yield Err(ChatError::Provider(message));
                            return;
                        }
                        CliEvent::Ignored => {}
                    }
                }

                if is_last {
                    if let Some(chunk) = usage_chunk(&usage, rates.as_ref()) {
                        yield Ok(chunk);
                    }
                    yield Ok(LlmStreamChunk::Done(Some("stop".to_string())));
                    return;
                }
            }
        })
    }
}

impl Default for CommandCodeProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl LlmProvider for CommandCodeProvider {
    fn stream<'a>(
        &'a self,
        config: &'a ModelConfig,
        messages: Vec<Message>,
        options: &'a LlmStreamOptions,
        tools: Option<Vec<serde_json::Value>>,
    ) -> BoxStream<'a, Result<LlmStreamChunk, ChatError>> {
        let base_url = host_base(&config.base_url);

        if config
            .api_key
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .is_empty()
        {
            return Box::pin(futures::stream::once(async {
                Err(ChatError::BadRequest(
                    "Command Code requires an API key".to_string(),
                ))
            }));
        }

        Box::pin(async_stream::stream! {
            match self.transport_for(&base_url, config.api_key.as_deref()).await {
                Transport::Official => {
                    // The OpenAI provider appends `/chat/completions` to the base.
                    let mut official = config.clone();
                    official.base_url = format!("{base_url}{PROVIDER_PATH}");
                    official.model_id = wire_model(&config.model_id);
                    let official_options = official_options(options);

                    // Held back for the fallback below: a plan without API access
                    // only announces itself on the chat request, not on the
                    // models probe.
                    let retry_messages = messages.clone();
                    let retry_tools = tools.clone();

                    let inner =
                        self.openai
                            .stream_with_mode(&official, messages, &official_options, tools, true);
                    futures::pin_mut!(inner);

                    let mut rejected = false;
                    while let Some(chunk) = inner.next().await {
                        if is_entitlement_rejection(&chunk) {
                            rejected = true;
                            break;
                        }
                        yield chunk;
                    }

                    if rejected {
                        warn!(
                            base_url = %base_url,
                            "Command Code rejected this plan on the Provider API; using the CLI transport"
                        );
                        self.remember_cli(&base_url, config.api_key.as_deref());

                        let inner = self.cli_stream(&base_url, config, retry_messages, options, retry_tools);
                        futures::pin_mut!(inner);
                        while let Some(chunk) = inner.next().await {
                            yield chunk;
                        }
                    }
                }
                Transport::Cli => {
                    let inner = self.cli_stream(&base_url, config, messages, options, tools);
                    futures::pin_mut!(inner);
                    while let Some(chunk) = inner.next().await {
                        yield chunk;
                    }
                }
            }
        })
    }
}

/// Builds and sends one CLI request, retrying transient failures.
#[allow(clippy::too_many_arguments)]
async fn send_cli_request_with_retry(
    client: &reqwest::Client,
    endpoint: &str,
    api_key: Option<&str>,
    headers: &[(&'static str, String)],
    custom_headers: &std::collections::BTreeMap<String, String>,
    body: &serde_json::Value,
    options: &LlmStreamOptions,
) -> Result<reqwest::Response, ChatError> {
    let mut last_error = None;

    for attempt in 0..=options.max_retries {
        let mut request = client.post(endpoint).json(body);
        if let Some(key) = api_key {
            request = request.bearer_auth(key);
        }
        for (name, value) in headers {
            request = request.header(*name, value);
        }

        let request = match apply_custom_headers(request, custom_headers) {
            Ok(request) => request,
            Err(error) => return Err(error),
        };
        let outcome = request
            .send()
            .await
            .map_err(|error| ChatError::Provider(error.to_string()));

        match outcome {
            Ok(response)
                if is_retryable_status(response.status()) && attempt < options.max_retries =>
            {
                let delay = retry_after_delay(response.headers())
                    .unwrap_or_else(|| retry_delay(attempt, options.max_retry_delay_ms));
                warn!(
                    status = %response.status(),
                    retry = attempt + 1,
                    "Command Code CLI returned a retryable status; retrying"
                );
                tokio::time::sleep(delay).await;
            }
            Ok(response) => return Ok(response),
            Err(error) if attempt < options.max_retries => {
                let delay = retry_delay(attempt, options.max_retry_delay_ms);
                warn!(error = %error, retry = attempt + 1, "Command Code CLI request failed; retrying");
                last_error = Some(error);
                tokio::time::sleep(delay).await;
            }
            Err(error) => return Err(error),
        }
    }

    Err(last_error
        .unwrap_or_else(|| ChatError::Provider("Command Code request failed".to_string())))
}

/// Converts the accumulated token counts into a usage chunk.
fn usage_chunk(usage: &UsageTotals, rates: Option<&ModelCostRates>) -> Option<LlmStreamChunk> {
    if usage.is_empty() {
        return None;
    }

    let (prompt, cached, completion, reasoning) = usage.counts();
    let costs = calculate_token_costs(rates, prompt, cached, None, completion, reasoning);

    Some(LlmStreamChunk::Usage {
        total_duration: None,
        prompt_eval_count: prompt,
        cached_prompt_eval_count: cached,
        eval_count: completion,
        reasoning_eval_count: reasoning,
        cost_input_usd: costs.input_usd,
        cost_output_usd: costs.output_usd,
        cost_reasoning_usd: costs.reasoning_usd,
    })
}

/// The connection's base URL without the Provider API path.
///
/// Both transports build their own path from the host, and the preset shipped
/// `https://…/provider/v1` as the base before this family existed — such a
/// connection keeps working once its provider type is switched.
fn host_base(base_url: &str) -> String {
    let base = normalize_base_url(base_url);

    if let Some(host) = base.strip_suffix(PROVIDER_PATH) {
        return host.to_string();
    }

    base
}

/// Cache key for one credential: base URL plus a fingerprint, so the cache
/// never holds a second copy of the API key.
fn memo_key(base_url: &str, api_key: Option<&str>) -> String {
    format!("{base_url}|{:x}", fingerprint(api_key))
}

/// True when the OpenAI provider surfaced a rejection that means "this plan may
/// not use the Provider API" (`403`, or a `404` from a base URL without the
/// path). The message shape is pinned by a test, so a change in the OpenAI
/// provider's error text cannot silently disable the fallback.
fn is_entitlement_rejection(chunk: &Result<LlmStreamChunk, ChatError>) -> bool {
    let Err(ChatError::Provider(message)) = chunk else {
        return false;
    };

    message
        .strip_prefix("OpenAI API error ")
        .is_some_and(|rest| rest.starts_with("403") || rest.starts_with("404"))
}

/// Short, allocation-light fingerprint so the transport cache never holds a
/// second copy of the API key.
fn fingerprint(api_key: Option<&str>) -> u64 {
    let mut hasher = DefaultHasher::new();
    api_key.unwrap_or_default().hash(&mut hasher);
    hasher.finish()
}

fn truncate_for_log(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    text.chars().take(limit).collect()
}
