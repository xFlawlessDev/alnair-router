//! Upstream dispatch: the vendored-provider seam, the fallback executor, and
//! HTTP proxying for the media endpoints.

pub mod chat_backend;
pub mod executor;
pub mod media;

pub use executor::{Attempt, AttemptOutcome, ExecutedStream, Executor};
