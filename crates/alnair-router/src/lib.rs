//! alnair-router: standalone OpenAI-compatible AI router.
//!
//! Resolves prefixed model references to upstream providers, expands combos into
//! ordered fallback chains, and streams responses through the [`alnair_llm`]
//! provider crate.
//!
//! The provider stack lives in the sibling `alnair-llm` crate so it can be
//! versioned and tested on its own. All access to it is funnelled through
//! [`upstream::chat_backend`].

pub mod auth;
pub mod backup;
pub mod cli;
pub mod config;
pub mod crypto;
pub mod db;
#[cfg(any(target_os = "windows", target_os = "macos"))]
pub mod desktop;
pub mod error;
pub mod handlers;
pub mod limits;
pub mod metrics;
pub mod middleware;
pub mod model;
pub mod policy;
pub mod pricing;
pub mod protocol;
pub mod providers;
pub mod server;
pub mod settings;
pub mod state;
pub mod telemetry;
pub mod token_saver;
pub mod update;
pub mod upstream;

pub use config::RouterConfig;
pub use db::Db;
pub use error::{Error, Result};
pub use model::{Catalog, ResolvedTarget, Resolver};
pub use server::build_router;
pub use state::AppState;
pub use upstream::{ExecutedStream, Executor};
