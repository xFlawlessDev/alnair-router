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
    pub secrets: SecretsConfig,
    pub limits: LimitsConfig,
    pub rate_limit: RateLimitConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ServerConfig {
    pub port: u16,
    pub host: String,
    /// When false, `/v1/*` accepts requests without an `Authorization` header.
    pub require_api_key: bool,
    /// Bearer token guarding the `/api/*` admin routes. Enforced whenever it is
    /// set, on loopback and beyond.
    pub admin_token: Option<String>,
    /// Permit a non-loopback bind without an admin token. Dangerous: anyone who
    /// can reach the port can read upstream credentials and mint client keys.
    pub allow_unauthenticated_admin: bool,
    /// Extra origins permitted to call the API cross-origin. The embedded UI is
    /// served same-origin and needs none of this, so the default is empty.
    pub cors_origins: Vec<String>,
    /// Include best-effort upstream TCP reachability in `/api/ready`.
    pub readiness_upstream_checks: bool,
    /// Serve the embedded dashboard at `/` (disable when a reverse proxy owns it).
    pub serve_dashboard: bool,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct SecretsConfig {
    /// 32-byte key, as 64 hex characters or base64, encrypting upstream
    /// credentials at rest. Required: the router refuses to start without it.
    pub key: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct StorageConfig {
    pub url: String,
}

/// Upstream concurrency caps. Zeros disable the corresponding cap.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct LimitsConfig {
    /// Maximum concurrent upstream calls across all connections.
    pub max_concurrent: usize,
    /// Maximum concurrent upstream calls per connection.
    pub max_concurrent_per_connection: usize,
    /// How long a request waits for a slot before failing with 429. 0 waits forever.
    pub acquire_timeout_ms: u64,
}

impl Default for LimitsConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 0,
            max_concurrent_per_connection: 0,
            acquire_timeout_ms: 30_000,
        }
    }
}

/// Default per-key request rate. `0` is unlimited; keys can override.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct RateLimitConfig {
    pub requests_per_minute: u32,
    /// Token-bucket capacity for bursts. 0 uses one minute's worth of tokens.
    pub burst: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct RoutingConfig {
    pub default_connection: Option<String>,
    pub max_attempts: usize,
    /// Retries inside a single tier before the executor fails over to the next.
    pub max_retries_per_tier: usize,
    /// Upper bound for the exponential provider retry delay, in milliseconds.
    pub max_retry_delay_ms: u64,
    /// Routing-catalog cache TTL in milliseconds. 0 caches until an admin write
    /// invalidates it.
    pub catalog_ttl_ms: u64,
    /// Default upstream connect/first-byte timeout in milliseconds (0 disables).
    /// Connections can override this.
    pub connect_timeout_ms: u64,
    /// Default upstream idle timeout between stream chunks in milliseconds
    /// (0 disables). Connections can override this.
    pub idle_timeout_ms: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: 7878,
            host: "127.0.0.1".to_string(),
            require_api_key: false,
            admin_token: None,
            allow_unauthenticated_admin: false,
            cors_origins: Vec::new(),
            readiness_upstream_checks: false,
            serve_dashboard: true,
        }
    }
}

impl Default for RoutingConfig {
    fn default() -> Self {
        Self {
            default_connection: None,
            max_attempts: 5,
            max_retries_per_tier: 2,
            max_retry_delay_ms: 30_000,
            catalog_ttl_ms: 1_000,
            connect_timeout_ms: 10_000,
            idle_timeout_ms: 60_000,
        }
    }
}

impl ServerConfig {
    /// True when `host` addresses the local machine only.
    pub fn binds_loopback(&self) -> bool {
        let host = self.host.trim().trim_matches(['[', ']']);
        matches!(host, "127.0.0.1" | "localhost" | "::1")
            || host
                .parse::<std::net::Ipv4Addr>()
                .is_ok_and(|ip| ip.is_loopback())
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
        self.admin_token().is_some()
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

    /// Rejects configurations that would start the router unsafely.
    ///
    /// Two rules:
    /// - a non-loopback bind needs an admin token unless the operator explicitly
    ///   opts out with `server.allow_unauthenticated_admin`;
    /// - a valid `secrets.key` is mandatory so upstream credentials are never
    ///   stored in plaintext.
    pub fn validate(&self) -> Result<()> {
        if !self.server.binds_loopback()
            && self.server.admin_token().is_none()
            && !self.server.allow_unauthenticated_admin
        {
            return Err(Error::Config(format!(
                "refusing to bind non-loopback host '{}' without server.admin_token: \
                 the /api routes expose upstream credentials and mint client keys. \
                 Set ALNAIR_ROUTER__SERVER__ADMIN_TOKEN, or bind 127.0.0.1, or set \
                 server.allow_unauthenticated_admin = true to override.",
                self.server.host
            )));
        }

        let Some(key) = &self.secrets.key else {
            return Err(Error::Config(
                "secrets.key is required: upstream credentials are encrypted at rest. \
                 Generate one with `openssl rand -hex 32` and set \
                 ALNAIR_ROUTER__SECRETS__KEY (or secrets.key in config.toml)."
                    .to_string(),
            ));
        };
        crate::crypto::parse_key(key)?;

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

    const TEST_SECRET: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    fn secrets() -> SecretsConfig {
        SecretsConfig {
            key: Some(TEST_SECRET.to_string()),
        }
    }

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
            assert!(
                server(host, None).binds_loopback(),
                "{host} should be loopback"
            );
        }
        for host in ["0.0.0.0", "192.168.1.10", "example.com"] {
            assert!(
                !server(host, None).binds_loopback(),
                "{host} should not be loopback"
            );
        }
    }

    #[test]
    fn blank_admin_token_counts_as_unset() {
        assert_eq!(server("127.0.0.1", Some("   ")).admin_token(), None);
        assert_eq!(
            server("127.0.0.1", Some("secret")).admin_token(),
            Some("secret")
        );
    }

    #[test]
    fn non_loopback_bind_without_token_is_rejected() {
        let config = RouterConfig {
            server: server("0.0.0.0", None),
            secrets: secrets(),
            ..RouterConfig::default()
        };

        let error = config.validate().expect_err("must refuse");
        assert!(matches!(error, Error::Config(_)));
    }

    #[test]
    fn non_loopback_bind_with_token_is_allowed() {
        let config = RouterConfig {
            server: server("0.0.0.0", Some("secret")),
            secrets: secrets(),
            ..RouterConfig::default()
        };

        assert!(config.validate().is_ok());
    }

    #[test]
    fn loopback_bind_without_token_is_allowed() {
        let config = RouterConfig {
            secrets: secrets(),
            ..RouterConfig::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn allow_unauthenticated_admin_permits_non_loopback_without_token() {
        let config = RouterConfig {
            server: ServerConfig {
                allow_unauthenticated_admin: true,
                ..server("0.0.0.0", None)
            },
            secrets: secrets(),
            ..RouterConfig::default()
        };

        assert!(config.validate().is_ok());
        assert!(!config.server.requires_admin_token());
    }

    #[test]
    fn admin_token_is_required_when_explicitly_configured() {
        assert!(server("127.0.0.1", Some("secret")).requires_admin_token());
        assert!(!server("127.0.0.1", None).requires_admin_token());
        assert!(server("0.0.0.0", Some("secret")).requires_admin_token());
    }

    #[test]
    fn missing_secret_key_is_rejected() {
        let error = RouterConfig::default().validate().expect_err("must refuse");
        assert!(
            error.to_string().contains("secrets.key"),
            "message should name secrets.key: {error}"
        );
    }

    #[test]
    fn invalid_secret_key_is_rejected() {
        let config = RouterConfig {
            secrets: SecretsConfig {
                key: Some("not-a-key".to_string()),
            },
            ..RouterConfig::default()
        };

        assert!(config.validate().is_err());
    }

    #[test]
    fn retry_defaults_are_pinned() {
        let routing = RoutingConfig::default();
        assert_eq!(routing.max_retries_per_tier, 2);
        assert_eq!(routing.max_retry_delay_ms, 30_000);
    }

    #[test]
    fn catalog_and_timeout_defaults_are_pinned() {
        let routing = RoutingConfig::default();
        assert_eq!(routing.catalog_ttl_ms, 1_000);
        assert_eq!(routing.connect_timeout_ms, 10_000);
        assert_eq!(routing.idle_timeout_ms, 60_000);
    }
}
