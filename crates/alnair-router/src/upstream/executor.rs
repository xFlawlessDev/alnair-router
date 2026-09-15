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
            let started = std::time::Instant::now();

            // A concurrency slot is held for the life of the attempt; on
            // success it travels with the returned stream so it is released
            // only when the response finishes or is dropped. A timeout here is
            // a service-level condition, not an upstream fault, so it fails
            // the request instead of walking further tiers.
            let permit = match settings.limiter.acquire(&target.connection_id).await {
                Ok(permit) => permit,
                Err(error) => {
                    settings.metrics.record_rate_limited();
                    return Err(error);
                }
            };

            let attempt_token = settings.telemetry.begin_attempt(
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
                target.api_key.as_deref(),
                options,
                settings.retry,
                streaming,
                tools.clone(),
                target.custom_headers.clone(),
                price,
            );

            let stream = match built {
                Ok(stream) => stream,
                Err(error) => {
                    settings
                        .telemetry
                        .finish_attempt(attempt_token, false, &error.to_string());
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
                    settings
                        .telemetry
                        .finish_attempt(attempt_token, false, &error.to_string());
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
                    settings
                        .telemetry
                        .finish_attempt(attempt_token, false, &error.to_string());
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
                    settings
                        .telemetry
                        .finish_attempt(attempt_token, true, "first chunk ready");
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
