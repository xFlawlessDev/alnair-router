//! Model reference resolution: `<prefix>/<model>` → ordered list of upstream targets.
//!
//! A reference is resolved in one of three ways:
//! 1. It names a **combo** → expand its entries in order into a fallback chain.
//! 2. It uses a `prefix/model` form that matches an **alias** → that connection.
//! 3. It is a bare model name → the configured default connection.

pub mod catalog;
pub mod resolver;

pub use resolver::{Catalog, MAX_DEPTH, ResolvedTarget, Resolver};
