//! Ordered fallback execution over resolved targets.
//!
//! Walks the target list produced by the resolver, dispatching to the first
//! upstream that produces a usable response. Because provider streams are
//! lazy, connection and auth failures surface on the *first* chunk rather than
//! at stream-construction time — so failover is decided by peeking the first
//! chunk, not by whether `stream()` returned `Ok`.

use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use futures::stream::BoxStream;

use crate::config::RouterConfig;
use crate::error::{Error, Result};
use crate::limits::{LimitPermit, UpstreamLimiter};
use crate::model::ResolvedTarget;
use crate::upstream::chat_backend::{
    self, ChunkStream, GenerationOptions, ProviderRegistry, RetryPolicy, RouterMessage, StreamChunk,
};

/// Router-wide upstream timeouts, in milliseconds. Zero disables one.
#[derive(Debug, Clone, Copy)]
pub struct UpstreamTimeouts {
    /// Time allowed for connect + first byte before the tier fails.
    pub connect_timeout_ms: u64,
    /// Maximum silence between stream chunks before the stream errors.
    pub idle_timeout_ms: u64,
}

impl Default for UpstreamTimeouts {
    fn default() -> Self {
        Self {
            connect_timeout_ms: 10_000,
            idle_timeout_ms: 60_000,
        }
    }
}

impl UpstreamTimeouts {
    pub fn from_config(config: &RouterConfig) -> Self {
        Self {
            connect_timeout_ms: config.router.connect_timeout_ms,
            idle_timeout_ms: config.router.idle_timeout_ms,
        }
    }
}

/// One upstream attempt and its outcome.
#[derive(Debug, Clone)]
pub struct Attempt {
    /// 1-based position in the fallback chain.
    pub index: usize,
    pub source: String,
    pub provider_type: String,
    pub connection_name: String,
    pub model: String,
    pub outcome: AttemptOutcome,
    pub latency_ms: u64,
}

#[derive(Debug, Clone)]
pub enum AttemptOutcome {
    Succeeded,
    Failed(String),
}

impl AttemptOutcome {
    pub fn is_success(&self) -> bool {
        matches!(self, AttemptOutcome::Succeeded)
    }
}

/// Round-robin cursor per connection so extra keys share traffic evenly.
///
/// The cursor lives in memory only: after a restart the first key is used
/// again, which is fine because selection is for load spreading, not state.
#[derive(Clone, Default)]
pub struct KeyRotator {
    cursors: Arc<std::sync::Mutex<std::collections::HashMap<String, usize>>>,
}

impl KeyRotator {
    /// Next start offset for a connection holding `len` keys.
    pub fn next(&self, connection_id: &str, len: usize) -> usize {
        if len <= 1 {
            return 0;
        }

        let mut cursors = self.cursors.lock().expect("key rotator poisoned");
        let cursor = cursors.entry(connection_id.to_string()).or_insert(0);
        let offset = *cursor % len;
        *cursor = (*cursor + 1) % len;
        offset
    }
}

/// Construction settings for an [`Executor`].
#[derive(Clone)]
pub struct ExecutorSettings {
    pub retry: RetryPolicy,
    pub limiter: UpstreamLimiter,
    pub timeouts: UpstreamTimeouts,
    pub metrics: Arc<crate::metrics::Metrics>,
    pub telemetry: Arc<crate::telemetry::ActivityTracker>,
    /// Price lookup for the resolved model; absent in unit tests.
    pub pricing: Option<Arc<crate::pricing::PricingCache>>,
    /// Chooses the first key to try when a connection has several.
    pub key_rotator: KeyRotator,
    /// Access tokens for OAuth connections, resolved just in time per tier;
    /// absent in unit tests.
    pub oauth_tokens: Option<Arc<crate::oauth::OAuthTokenCache>>,
}

impl Default for ExecutorSettings {
    fn default() -> Self {
        Self {
            retry: RetryPolicy::default(),
            limiter: UpstreamLimiter::new(&crate::config::LimitsConfig::default()),
            timeouts: UpstreamTimeouts::default(),
            metrics: Arc::new(crate::metrics::Metrics::default()),
            telemetry: Arc::new(crate::telemetry::ActivityTracker::new()),
            pricing: None,
            key_rotator: KeyRotator::default(),
            oauth_tokens: None,
        }
    }
}

/// Walks resolved targets until one succeeds.
#[derive(Clone)]
pub struct Executor {
    registry: Arc<ProviderRegistry>,
    settings: Arc<std::sync::RwLock<ExecutorSettings>>,
}

impl Executor {
    pub fn new(registry: Arc<ProviderRegistry>) -> Self {
        Self::with_settings(registry, ExecutorSettings::default())
    }

    /// Builds an executor with explicit retry, concurrency, timeout and metrics.
    pub fn with_settings(registry: Arc<ProviderRegistry>, settings: ExecutorSettings) -> Self {
        Self {
            registry,
            settings: Arc::new(std::sync::RwLock::new(settings)),
        }
    }

    /// Re-reads the retry policy and timeouts from the live configuration.
    /// Concurrency and metrics are shared components and update themselves.
    pub fn apply(&self, config: &RouterConfig) {
        let mut settings = self.settings.write().expect("executor settings poisoned");
        settings.retry = RetryPolicy {
            max_retries_per_tier: config.router.max_retries_per_tier,
            max_retry_delay_ms: config.router.max_retry_delay_ms,
            transport_retries: 1,
        };
        settings.timeouts = UpstreamTimeouts::from_config(config);
    }

    fn settings(&self) -> ExecutorSettings {
        self.settings
            .read()
            .expect("executor settings poisoned")
            .clone()
    }

    /// Opens a stream from the first target that yields a first chunk without
    /// erroring.
    ///
    /// Returns the stream, the winning target, and the record of every attempt
    /// made (including failures) so usage can be logged per tier.
    pub async fn stream(
        &self,
        targets: &[ResolvedTarget],
        messages: Vec<RouterMessage>,
        tools: Option<Vec<serde_json::Value>>,
        options: Option<&GenerationOptions>,
        streaming: bool,
    ) -> Result<ExecutedStream> {
        if targets.is_empty() {
            return Err(Error::NoRoute("no resolved targets".to_string()));
        }

        // One snapshot per request: a settings save mid-request must not mix
        // retry policies between tiers.
        let settings = self.settings();

        let mut attempts = Vec::with_capacity(targets.len());
        let mut last_error: Option<String> = None;

        for (index, target) in targets.iter().enumerate() {
            // A connection authenticates either with static keys or with OAuth
            // accounts; both rotate round-robin and both fall over to the next
            // credential when the current one fails before the first byte.
            let uses_oauth = !target.oauth_account_ids.is_empty();
            let credentials = if uses_oauth {
                target.oauth_account_ids.len()
            } else {
                target.api_keys.len()
            };
            let offset = settings
                .key_rotator
                .next(&target.connection_id, credentials);
            let tries = credentials.max(1);

            for step in 0..tries {
                let started = std::time::Instant::now();
                let selected = (offset + step) % tries;

                // The credential for this attempt. An OAuth connection resolves
                // its access token here, just in time, so a rotation made by
                // another request is picked up rather than served from the
                // cached routing snapshot.
                let credential = if uses_oauth {
                    match &settings.oauth_tokens {
                        Some(tokens) => {
                            match tokens
                                .access_token(&target.oauth_account_ids[selected])
                                .await
                            {
                                Ok(token) => Some(token),
                                Err(error) => {
                                    // A refresh that fails is a fault of this
                                    // tier alone: record it and walk on rather
                                    // than failing the whole request.
                                    tracing::warn!(
                                        attempt = index + 1,
                                        source = %target.source,
                                        account = %target.oauth_account_ids[selected],
                                        error = %error,
                                        "oauth token unavailable; falling through"
                                    );
                                    last_error = Some(error.to_string());
                                    attempts.push(failed_attempt(index, target, &error, started));
                                    continue;
                                }
                            }
                        }
                        None => {
                            let error = Error::Internal(
                                "this connection authenticates with OAuth but no token service is configured"
                                    .to_string(),
                            );
                            last_error = Some(error.to_string());
                            attempts.push(failed_attempt(index, target, &error, started));
                            continue;
                        }
                    }
                } else if target.api_keys.is_empty() {
                    None
                } else {
                    Some(target.api_keys[selected].clone())
                };

                // A concurrency slot is held for the life of the attempt; on
                // success it travels with the returned stream so it is released
                // only when the response finishes or is dropped. A timeout here
                // is a service-level condition, not an upstream fault, so it
                // fails the request instead of walking further tiers.
                let permit = match settings.limiter.acquire(&target.connection_id).await {
                    Ok(permit) => permit,
                    Err(error) => {
                        settings.metrics.record_rate_limited();
                        return Err(error);
                    }
                };

                let attempt_guard = settings.telemetry.begin_attempt(
                    &target.connection_id,
                    &target.connection_name,
                    &target.model,
                    &target.source,
                    index + 1,
                );

                // Connections may pin a catalog id for relays whose upstream model
                // path does not match any priced key.
                let pricing_key = target.pricing_model.as_deref().unwrap_or(&target.model);
                let price = match &settings.pricing {
                    Some(pricing) => pricing.price_for(pricing_key).await,
                    None => None,
                };

                let built = chat_backend::stream(
                    self.registry.clone(),
                    &target.provider_type,
                    &target.base_url,
                    &target.model,
                    messages.clone(),
                    credential.as_deref(),
                    options,
                    settings.retry,
                    streaming,
                    tools.clone(),
                    target.custom_headers.clone(),
                    price,
                    chat_backend::cache_retention_from_str(&target.cache_retention),
                    chat_backend::auth_style_from_str(&target.auth_style),
                    chat_backend::wire_api_from_str(&target.wire_api),
                );

                let stream = match built {
                    Ok(stream) => stream,
                    Err(error) => {
                        attempt_guard.finish(false, &error.to_string());
                        // A malformed provider type is a configuration fault, not a
                        // transient upstream failure: fail loudly instead of
                        // silently walking the rest of the chain.
                        if matches!(error, Error::UnsupportedProviderType(_)) {
                            return Err(error);
                        }
                        last_error = Some(error.to_string());
                        attempts.push(failed_attempt(index, target, &error, started));
                        continue;
                    }
                };

                // Peek the first chunk: this is where connection/auth failures land.
                // `ChunkStream` is `Pin<Box<dyn Stream>>`, so it is already `Unpin`
                // and can be advanced without pinning the local binding. The wait is
                // bounded by the connection's connect timeout.
                let mut stream = stream;
                let connect_timeout_ms = target
                    .connect_timeout_ms
                    .unwrap_or(settings.timeouts.connect_timeout_ms);
                let first = match wait_for_chunk(&mut stream, connect_timeout_ms).await {
                    ChunkWait::Ready(item) => item,
                    ChunkWait::TimedOut => {
                        let error = Error::Upstream(format!(
                            "upstream did not respond within {connect_timeout_ms} ms"
                        ));
                        attempt_guard.finish(false, &error.to_string());
                        tracing::warn!(
                            attempt = index + 1,
                            source = %target.source,
                            model = %target.model,
                            timeout_ms = connect_timeout_ms,
                            "upstream connect timeout; falling through"
                        );
                        last_error = Some(error.to_string());
                        attempts.push(failed_attempt(index, target, &error, started));
                        continue;
                    }
                };

                match first {
                    Some(Err(error)) => {
                        attempt_guard.finish(false, &error.to_string());
                        tracing::warn!(
                            attempt = index + 1,
                            source = %target.source,
                            model = %target.model,
                            error = %error,
                            "upstream attempt failed on first chunk; falling through"
                        );
                        last_error = Some(error.to_string());
                        attempts.push(failed_attempt(index, target, &error, started));
                    }
                    first => {
                        attempt_guard.finish(true, "first chunk ready");
                        let latency_ms = started.elapsed().as_millis() as u64;
                        attempts.push(Attempt {
                            index: index + 1,
                            source: target.source.clone(),
                            provider_type: target.provider_type.clone(),
                            connection_name: target.connection_name.clone(),
                            model: target.model.clone(),
                            outcome: AttemptOutcome::Succeeded,
                            latency_ms,
                        });

                        // Re-attach the peeked chunk to the front of the stream.
                        let rest: BoxStream<'static, Result<StreamChunk>> = match first {
                            Some(Ok(chunk)) => futures::stream::once(async move { Ok(chunk) })
                                .chain(stream)
                                .boxed(),
                            _ => stream.boxed(),
                        };
                        let idle_timeout_ms = target
                            .idle_timeout_ms
                            .unwrap_or(settings.timeouts.idle_timeout_ms);
                        let rest = with_idle_timeout(rest, idle_timeout_ms);
                        let rest = hold_permit(rest, permit);

                        settings.metrics.record_attempts(&attempts);

                        return Ok(ExecutedStream {
                            stream: rest,
                            target: target.clone(),
                            attempts,
                            latency_ms,
                        });
                    }
                }
            }
        }

        settings.metrics.record_attempts(&attempts);

        Err(Error::AllAttemptsFailed(
            last_error.unwrap_or_else(|| "unknown failure".to_string()),
        ))
    }
}

/// A live stream plus the routing decision that produced it.
pub struct ExecutedStream {
    pub stream: ChunkStream,
    pub target: ResolvedTarget,
    pub attempts: Vec<Attempt>,
    /// Time to first chunk, in milliseconds.
    pub latency_ms: u64,
}

/// Result of waiting for a first chunk with a deadline.
enum ChunkWait {
    Ready(Option<Result<StreamChunk>>),
    TimedOut,
}

/// Waits for the next chunk, bounded by `timeout_ms` (0 waits forever).
async fn wait_for_chunk(stream: &mut ChunkStream, timeout_ms: u64) -> ChunkWait {
    if timeout_ms == 0 {
        return ChunkWait::Ready(stream.next().await);
    }

    match tokio::time::timeout(Duration::from_millis(timeout_ms), stream.next()).await {
        Ok(item) => ChunkWait::Ready(item),
        Err(_) => ChunkWait::TimedOut,
    }
}

/// Errs when no chunk arrives for `idle_ms` (0 disables the timeout).
fn with_idle_timeout(
    stream: BoxStream<'static, Result<StreamChunk>>,
    idle_ms: u64,
) -> BoxStream<'static, Result<StreamChunk>> {
    if idle_ms == 0 {
        return stream;
    }

    futures::stream::unfold((stream, false), move |(mut stream, finished)| async move {
        if finished {
            return None;
        }

        match tokio::time::timeout(Duration::from_millis(idle_ms), stream.next()).await {
            Ok(Some(item)) => Some((item, (stream, false))),
            Ok(None) => None,
            Err(_) => Some((
                Err(Error::Upstream(format!(
                    "upstream stream was idle for more than {idle_ms} ms"
                ))),
                (stream, true),
            )),
        }
    })
    .boxed()
}

/// Keeps limiter permits alive until the stream is exhausted or dropped.
fn hold_permit(
    stream: BoxStream<'static, Result<StreamChunk>>,
    permit: LimitPermit,
) -> BoxStream<'static, Result<StreamChunk>> {
    futures::stream::unfold((stream, permit), |(mut stream, permit)| async move {
        stream.next().await.map(|item| (item, (stream, permit)))
    })
    .boxed()
}

fn failed_attempt(
    index: usize,
    target: &ResolvedTarget,
    error: &Error,
    started: std::time::Instant,
) -> Attempt {
    Attempt {
        index: index + 1,
        source: target.source.clone(),
        provider_type: target.provider_type.clone(),
        connection_name: target.connection_name.clone(),
        model: target.model.clone(),
        outcome: AttemptOutcome::Failed(error.to_string()),
        latency_ms: started.elapsed().as_millis() as u64,
    }
}

#[cfg(test)]
mod tests {
    use super::{Executor, KeyRotator, StreamChunk};
    use crate::model::resolver::ResolvedTarget;
    use crate::upstream::chat_backend;
    use std::sync::Arc;

    #[test]
    fn key_rotator_cycles_offsets_per_connection() {
        let rotator = KeyRotator::default();

        assert_eq!(rotator.next("c1", 1), 0);
        assert_eq!(rotator.next("c1", 1), 0);

        assert_eq!(rotator.next("c2", 3), 0);
        assert_eq!(rotator.next("c2", 3), 1);
        assert_eq!(rotator.next("c2", 3), 2);
        assert_eq!(rotator.next("c2", 3), 0);

        // Cursors are independent per connection.
        assert_eq!(rotator.next("c3", 2), 0);
        assert_eq!(rotator.next("c2", 3), 1);
    }

    #[test]
    fn key_rotator_without_keys_is_a_no_op() {
        let rotator = KeyRotator::default();
        assert_eq!(rotator.next("c1", 0), 0);
        assert_eq!(rotator.next("c1", 0), 0);
    }

    fn response(bytes: &[u8]) -> Vec<u8> {
        bytes.to_vec()
    }

    /// What the fake upstream does after writing the body.
    #[derive(Clone, Copy)]
    enum Ending {
        /// The full event sequence, closed cleanly.
        Complete,
        /// The socket dies after the text delta, before `response.completed`.
        Truncated,
        /// The event sequence, then EOF without `response.completed`.
        NoTerminal,
    }

    /// Answers `POST /v1/responses` with a fixed event sequence. Any other path
    /// gets a 404, so a request that still went to `/chat/completions` shows up
    /// as a failed attempt rather than passing.
    async fn responses_upstream(ending: Ending) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("addr");

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut buffer = vec![0u8; 8192];
            let read = socket.read(&mut buffer).await.expect("read");
            let request = String::from_utf8_lossy(&buffer[..read]).to_string();

            if request.starts_with("POST /v1/responses ") {
                socket
                    .write_all(&response(
                        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
                    ))
                    .await
                    .expect("headers");

                let events: &[&str] = match ending {
                    Ending::Complete | Ending::NoTerminal => &[
                        r#"{"type":"response.created","response":{"id":"resp_1"}}"#,
                        r#"{"type":"response.output_text.delta","delta":"hola"}"#,
                        r#"{"type":"response.output_item.done","item":{"type":"function_call","id":"fc_1","call_id":"call-1","name":"read_file","arguments":"{\"path\":\"a\"}"}}"#,
                        r#"{"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":10,"output_tokens":4}}}"#,
                    ],
                    Ending::Truncated => &[
                        r#"{"type":"response.created","response":{"id":"resp_1"}}"#,
                        r#"{"type":"response.output_text.delta","delta":"hola"}"#,
                    ],
                };
                let last = match ending {
                    Ending::NoTerminal => events.len() - 1,
                    _ => events.len(),
                };
                for event in &events[..last] {
                    socket
                        .write_all(format!("data: {event}\n\n").as_bytes())
                        .await
                        .expect("event");
                }
                if matches!(ending, Ending::Complete) {
                    socket.write_all(b"data: [DONE]\n\n").await.expect("done");
                }
            } else {
                socket
                    .write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
                    )
                    .await
                    .expect("404");
            }
            let _ = socket.shutdown().await;
        });

        format!("http://{address}/v1")
    }

    fn target(base_url: String, wire_api: &str) -> ResolvedTarget {
        ResolvedTarget {
            connection_id: "c1".to_string(),
            connection_name: "responses-upstream".to_string(),
            provider_type: "openai-compatible".to_string(),
            base_url,
            search_path: None,
            search_tier: None,
            model: "gpt-5.1".to_string(),
            api_keys: vec!["sk-test".to_string()],
            custom_headers: Default::default(),
            connect_timeout_ms: Some(5_000),
            idle_timeout_ms: Some(5_000),
            pricing_model: None,
            cache_retention: "none".to_string(),
            auth_style: "api_key".to_string(),
            wire_api: wire_api.to_string(),
            oauth_account_ids: Vec::new(),
            source: "alias:test".to_string(),
        }
    }

    /// Drains the stream and describes what came back, in order.
    async fn drain(
        executor: &Executor,
        target: &ResolvedTarget,
    ) -> Result<Vec<StreamChunk>, crate::error::Error> {
        use futures::StreamExt;

        let executed = executor
            .stream(
                std::slice::from_ref(target),
                vec![chat_backend::message_text("user", "hola")],
                None,
                None,
                true,
            )
            .await?;

        let mut chunks = Vec::new();
        let mut stream = executed.stream;
        while let Some(chunk) = stream.next().await {
            chunks.push(chunk?);
        }
        Ok(chunks)
    }

    #[tokio::test]
    async fn the_responses_wire_api_reaches_the_responses_endpoint() {
        let executor = Executor::new(Arc::new(chat_backend::default_registry()));
        let target = target(responses_upstream(Ending::Complete).await, "responses");

        let chunks = drain(&executor, &target).await.expect("a completed turn");

        assert!(matches!(&chunks[0], StreamChunk::Text(text) if text == "hola"));
        assert!(matches!(
            &chunks[1],
            StreamChunk::ToolCall { id, name, .. } if id == "call-1" && name == "read_file"
        ));
        assert!(matches!(
            &chunks[2],
            StreamChunk::Usage(usage) if usage.prompt_tokens == 10 && usage.completion_tokens == 4
        ));
        assert!(
            matches!(&chunks[3], StreamChunk::Done(reason) if reason.as_deref() == Some("stop"))
        );
    }

    #[tokio::test]
    async fn a_responses_socket_that_dies_mid_turn_fails_the_attempt() {
        let executor = Executor::new(Arc::new(chat_backend::default_registry()));
        let target = target(responses_upstream(Ending::Truncated).await, "responses");

        let result = drain(&executor, &target).await;

        // Only the text delta arrived, so the executor's first-chunk peek saw a
        // failure and there is no second tier to fall through to.
        assert!(result.is_err(), "a truncated turn is not a served answer");
    }

    #[tokio::test]
    async fn a_responses_stream_without_a_terminal_event_is_not_served_as_complete() {
        let executor = Executor::new(Arc::new(chat_backend::default_registry()));
        let target = target(responses_upstream(Ending::NoTerminal).await, "responses");

        let result = drain(&executor, &target).await;

        // The attempt succeeded (the first chunk arrived), so the failure
        // surfaces on the live stream rather than as a fall-through.
        let error = result.expect_err("a socket that dies mid-turn is not a complete answer");
        assert!(error.to_string().contains("terminal event"), "got: {error}");
    }

    #[tokio::test]
    async fn the_chat_wire_api_does_not_use_the_responses_endpoint() {
        let executor = Executor::new(Arc::new(chat_backend::default_registry()));
        let target = target(responses_upstream(Ending::Complete).await, "chat");

        let result = drain(&executor, &target).await;

        assert!(
            result.is_err(),
            "a chat-wired connection must not call /responses"
        );
    }
}
