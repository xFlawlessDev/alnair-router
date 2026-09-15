//! alnair-router: standalone OpenAI-compatible AI router.
//!
//! Resolves prefixed model references to upstream providers, expands combos into
//! ordered fallback chains, and streams responses through the vendored provider
//! layer in [`llm`].
//!
//! The package builds standalone: the provider stack is vendored under `src/llm`
//! rather than pulled in as a path dependency on the main workspace. All access
//! to it is funnelled through [`upstream::chat_backend`].

pub mod config;
pub mod crypto;
pub mod db;
pub mod error;
pub mod handlers;
pub mod limits;
pub mod llm;
pub mod metrics;
pub mod middleware;
pub mod model;
pub mod protocol;
pub mod server;
pub mod state;
pub mod upstream;

pub use config::RouterConfig;
pub use db::Db;
pub use error::{Error, Result};
pub use model::{Catalog, ResolvedTarget, Resolver};
pub use server::build_router;
pub use state::AppState;
pub use upstream::{ExecutedStream, Executor};
