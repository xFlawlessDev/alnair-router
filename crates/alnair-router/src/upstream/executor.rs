//! Ordered fallback execution over resolved targets.
//!
//! Walks the target list produced by the resolver, dispatching to the first
//! upstream that produces a usable response. Because provider streams are
//! lazy, connection and auth failures surface on the *first* chunk rather than
//! at stream-construction time — so failover is decided by peeking the first
//! chunk, not by whether `stream()` returned `Ok`.

use std::sync::Arc;

use futures::StreamExt;
use futures::stream::BoxStream;

use crate::error::{Error, Result};
use crate::model::ResolvedTarget;
use crate::upstream::chat_backend::{
    self, ChunkStream, GenerationOptions, ProviderRegistry, RouterMessage, StreamChunk,
};

/// One upstream attempt and its outcome.
#[derive(Debug, Clone)]
pub struct Attempt {
    /// 1-based position in the fallback chain.
    pub index: usize,
    pub source: String,
    pub provider_type: String,
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

/// Walks resolved targets until one succeeds.
#[derive(Clone)]
pub struct Executor {
    registry: Arc<ProviderRegistry>,
}

impl Executor {
    pub fn new(registry: Arc<ProviderRegistry>) -> Self {
        Self { registry }
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
    ) -> Result<ExecutedStream> {
        if targets.is_empty() {
            return Err(Error::NoRoute("no resolved targets".to_string()));
        }

        let mut attempts = Vec::with_capacity(targets.len());
        let mut last_error: Option<String> = None;

        for (index, target) in targets.iter().enumerate() {
            let started = std::time::Instant::now();

            let built = chat_backend::stream(
                self.registry.clone(),
                &target.provider_type,
                &target.base_url,
                &target.model,
                messages.clone(),
                target.api_key.as_deref(),
                options,
                tools.clone(),
                target.custom_headers.clone(),
            );

            let stream = match built {
                Ok(stream) => stream,
                Err(error) => {
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
            // and can be advanced without pinning the local binding.
            let mut stream = stream;
            match stream.next().await {
                Some(Err(error)) => {
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
                    let latency_ms = started.elapsed().as_millis() as u64;
                    attempts.push(Attempt {
                        index: index + 1,
                        source: target.source.clone(),
                        provider_type: target.provider_type.clone(),
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

                    return Ok(ExecutedStream {
                        stream: rest,
                        target: target.clone(),
                        attempts,
                        latency_ms,
                    });
                }
            }
        }

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
        model: target.model.clone(),
        outcome: AttemptOutcome::Failed(error.to_string()),
        latency_ms: started.elapsed().as_millis() as u64,
    }
}
