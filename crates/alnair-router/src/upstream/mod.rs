//! Upstream dispatch: the vendored-provider seam, the fallback executor, and
//! HTTP proxying for the media endpoints.

pub mod chat_backend;
pub mod executor;
pub mod media;
pub mod probe;

pub use executor::{
    Attempt, AttemptOutcome, ExecutedStream, Executor, ExecutorSettings, UpstreamTimeouts,
};
pub use probe::{ProbeOutcome, UpstreamModel, model_available};
