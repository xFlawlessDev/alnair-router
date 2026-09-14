//! Router configuration: file-based with `ALNAIR_ROUTER__SECTION__KEY` env overrides.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Home directory for router state: `$ALNAIR_ROUTER_HOME` or `~/.alnair-router`.
pub fn router_home() -> PathBuf {
    if let Ok(value) = std::env::var("ALNAIR_ROUTER_HOME")
        && !value.trim().is_empty()
    {
        return PathBuf::from(value);
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    Path::new(&home).join(".alnair-router")
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct RouterConfig {
    pub server: ServerConfig,
    pub storage: StorageConfig,
    pub router: RoutingConfig,
    pub queue: QueueSection,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ServerConfig {
    pub port: u16,
    pub host: String,
    /// When false, `/v1/*` accepts requests without an `Authorization` header.
    pub require_api_key: bool,
    /// Bearer token guarding the `/api/*` admin routes. Required whenever the
    /// server binds a non-loopback host, because those routes can read upstream
    /// credentials and mint client keys.
    pub admin_token: Option<String>,
    /// Extra origins permitted to call the API cross-origin. The embedded UI is
    /// served same-origin and needs none of this, so the default is empty —
    /// meaning no CORS layer is installed at all.
    pub cors_origins: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct StorageConfig {
    pub url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct RoutingConfig {
    pub default_connection: Option<String>,
    pub max_attempts: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct QueueSection {
    pub enabled: bool,
    pub max_concurrent: usize,
    pub max_queue_size: usize,
    pub timeout_secs: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: 7878,
            host: "127.0.0.1".to_string(),
            require_api_key: false,
            admin_token: None,
            cors_origins: Vec::new(),
        }
    }
}

impl Default for RoutingConfig {
    fn default() -> Self {
        Self {
            default_connection: None,
            max_attempts: 5,
        }
    }
}

impl Default for QueueSection {
    fn default() -> Self {
        Self {
            enabled: true,
            max_concurrent: 4,
            max_queue_size: 16,
            timeout_secs: 300,
        }
    }
}

impl ServerConfig {
    /// True when `host` addresses the local machine only.
    pub fn binds_loopback(&self) -> bool {
        let host = self.host.trim().trim_matches(['[', ']']);
        matches!(host, "127.0.0.1" | "localhost" | "::1")
            || host.parse::<std::net::Ipv4Addr>().is_ok_and(|ip| ip.is_loopback())
    }

    /// Effective admin token, treating blank values as unset.
    pub fn admin_token(&self) -> Option<&str> {
        self.admin_token
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    /// True when `/api/*` must demand a bearer token.
    pub fn requires_admin_token(&self) -> bool {
        // A non-loopback bind always requires the token; on loopback it is only
        // required when the operator explicitly configured one.
        !self.binds_loopback() || self.admin_token().is_some()
    }
}

impl RouterConfig {
    /// Default SQLite URL derived from the router home directory.
    pub fn default_database_url() -> String {
        let path = router_home().join("db").join("router.sqlite");
        format!("sqlite://{}?mode=rwc", path.display())
    }

    /// Resolves the effective database URL, falling back to the router home default.
    pub fn database_url(&self) -> String {
        if self.storage.url.trim().is_empty() {
            Self::default_database_url()
        } else {
            expand_home(self.storage.url.trim())
        }
    }

    /// Rejects configurations that would expose unauthenticated admin routes.
    ///
    /// Binding a non-loopback host without an `admin_token` would let anyone who
    /// can reach the port read every upstream credential and mint API keys, so
    /// the server refuses to start instead.
    pub fn validate(&self) -> Result<()> {
        if !self.server.binds_loopback() && self.server.admin_token().is_none() {
            return Err(Error::Config(format!(
                "refusing to bind non-loopback host '{}' without server.admin_token: \
                 the /api routes are unauthenticated and expose upstream credentials. \
                 Set ALNAIR_ROUTER__SERVER__ADMIN_TOKEN (or bind 127.0.0.1).",
                self.server.host
            )));
        }
        Ok(())
    }
}

/// Expands a leading `~` into the user's home directory.
fn expand_home(value: &str) -> String {
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME"));
    match home {
        Ok(home) if value.starts_with("~/") || value.starts_with("~\\") => {
            format!("{}{}", home, &value[1..])
        }
        _ => value.to_string(),
    }
}

/// Loads config from `$ALNAIR_ROUTER_HOME/config.toml` plus `ALNAIR_ROUTER__*` env vars.
pub fn load() -> Result<RouterConfig> {
    let config_path = router_home().join("config.toml");

    let builder = config::Config::builder()
        .add_source(config::File::from(config_path).required(false))
        .add_source(
            config::Environment::with_prefix("ALNAIR_ROUTER")
                .prefix_separator("__")
                .separator("__"),
        );

    let config = builder
        .build()
        .map_err(|error| Error::Config(format!("failed to load config: {error}")))?
        .try_deserialize::<RouterConfig>()
        .map_err(|error| Error::Config(format!("invalid config: {error}")))?;

    config.validate()?;

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(host: &str, token: Option<&str>) -> ServerConfig {
        ServerConfig {
            host: host.to_string(),
            admin_token: token.map(str::to_string),
            ..ServerConfig::default()
        }
    }

    #[test]
    fn loopback_hosts_are_recognized() {
        for host in ["127.0.0.1", "localhost", "::1", "[::1]"] {
            assert!(server(host, None).binds_loopback(), "{host} should be loopback");
        }
        for host in ["0.0.0.0", "192.168.1.10", "example.com"] {
            assert!(!server(host, None).binds_loopback(), "{host} should not be loopback");
        }
    }

    #[test]
    fn blank_admin_token_counts_as_unset() {
        assert_eq!(server("127.0.0.1", Some("   ")).admin_token(), None);
        assert_eq!(server("127.0.0.1", Some("secret")).admin_token(), Some("secret"));
    }

    #[test]
    fn non_loopback_bind_without_token_is_rejected() {
        let config = RouterConfig {
            server: server("0.0.0.0", None),
            ..RouterConfig::default()
        };

        let error = config.validate().expect_err("must refuse");
        assert!(matches!(error, Error::Config(_)));
    }

    #[test]
    fn non_loopback_bind_with_token_is_allowed() {
        let config = RouterConfig {
            server: server("0.0.0.0", Some("secret")),
            ..RouterConfig::default()
        };

        assert!(config.validate().is_ok());
    }

    #[test]
    fn loopback_bind_without_token_is_allowed() {
        let config = RouterConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn admin_token_is_required_when_explicitly_configured() {
        assert!(server("127.0.0.1", Some("secret")).requires_admin_token());
        assert!(!server("127.0.0.1", None).requires_admin_token());
        assert!(server("0.0.0.0", Some("secret")).requires_admin_token());
    }
}
