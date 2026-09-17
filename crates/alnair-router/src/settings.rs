//! Dashboard-managed runtime settings.
//!
//! The admin UI stores sparse overrides in the `settings` table. They are
//! merged over the file/env configuration at startup and hot-applied when the
//! admin saves, so changes survive restarts without editing `config.toml`.
//! Deployment-only values (bind address, storage, encryption key) stay out of
//! this layer.

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::config::RouterConfig;
use crate::error::{Error, Result};

/// Persisted overrides, one `Some` per field the admin has changed.
///
/// `admin_token` and `default_connection` are double options: `None` leaves the
/// file/env value in place, `Some(None)` forces the empty value.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SettingsOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub require_api_key: Option<bool>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::db::repos::double_option"
    )]
    pub admin_token: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readiness_upstream_checks: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_usage: Option<bool>,
    /// Keep a reversible copy of minted client keys, enabling reveal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store_key_secrets: Option<bool>,
    /// Bind every interface (LAN access); the listener rebinds when this flips.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lan_access: Option<bool>,
    /// Browser origins allowed to call the API cross-origin. `Some(vec![])`
    /// disables CORS headers entirely; `["*"]` allows any origin.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cors_origins: Option<Vec<String>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::db::repos::double_option"
    )]
    pub default_connection: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_attempts: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_retries_per_tier: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_retry_delay_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub catalog_ttl_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connect_timeout_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idle_timeout_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_concurrent: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_concurrent_per_connection: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acquire_timeout_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requests_per_minute: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub burst: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pricing_sync_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pricing_sync_interval_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pricing_source_url: Option<String>,
    pub slimmer_enabled: Option<bool>,
    pub slimmer_level: Option<String>,
    pub headroom_enabled: Option<bool>,
    pub headroom_url: Option<String>,
    pub headroom_timeout_ms: Option<u64>,
    pub terse_enabled: Option<bool>,
    pub caveman_enabled: Option<bool>,
    pub caveman_level: Option<String>,
    pub ponytail_enabled: Option<bool>,
    pub ponytail_level: Option<String>,
}

impl SettingsOverrides {
    /// Writes every present override onto `config`.
    pub fn apply(&self, config: &mut RouterConfig) {
        if let Some(value) = self.require_api_key {
            config.server.require_api_key = value;
        }
        if let Some(token) = &self.admin_token {
            config.server.admin_token = token.clone();
        }
        if let Some(value) = self.readiness_upstream_checks {
            config.server.readiness_upstream_checks = value;
        }
        if let Some(value) = self.public_usage {
            config.server.public_usage = value;
        }
        if let Some(value) = self.store_key_secrets {
            config.server.store_key_secrets = value;
        }
        if let Some(value) = self.lan_access {
            config.server.lan_access = value;
        }
        if let Some(value) = &self.cors_origins {
            config.server.cors_origins = value.clone();
        }
        if let Some(value) = &self.default_connection {
            config.router.default_connection = value.clone();
        }
        if let Some(value) = self.max_attempts {
            config.router.max_attempts = value;
        }
        if let Some(value) = self.max_retries_per_tier {
            config.router.max_retries_per_tier = value;
        }
        if let Some(value) = self.max_retry_delay_ms {
            config.router.max_retry_delay_ms = value;
        }
        if let Some(value) = self.catalog_ttl_ms {
            config.router.catalog_ttl_ms = value;
        }
        if let Some(value) = self.connect_timeout_ms {
            config.router.connect_timeout_ms = value;
        }
        if let Some(value) = self.idle_timeout_ms {
            config.router.idle_timeout_ms = value;
        }
        if let Some(value) = self.max_concurrent {
            config.limits.max_concurrent = value;
        }
        if let Some(value) = self.max_concurrent_per_connection {
            config.limits.max_concurrent_per_connection = value;
        }
        if let Some(value) = self.acquire_timeout_ms {
            config.limits.acquire_timeout_ms = value;
        }
        if let Some(value) = self.requests_per_minute {
            config.rate_limit.requests_per_minute = value;
        }
        if let Some(value) = self.burst {
            config.rate_limit.burst = value;
        }
        if let Some(value) = self.pricing_sync_enabled {
            config.pricing.sync_enabled = value;
        }
        if let Some(value) = self.pricing_sync_interval_secs {
            config.pricing.sync_interval_secs = value;
        }
        if let Some(value) = &self.pricing_source_url {
            config.pricing.source_url = value.clone();
        }
        if let Some(value) = self.slimmer_enabled {
            config.token_saver.slimmer_enabled = value;
        }
        if let Some(value) = &self.slimmer_level {
            config.token_saver.slimmer_level = value.clone();
        }
        if let Some(value) = self.headroom_enabled {
            config.token_saver.headroom_enabled = value;
        }
        if let Some(value) = &self.headroom_url {
            config.token_saver.headroom_url = value.clone();
        }
        if let Some(value) = self.headroom_timeout_ms {
            config.token_saver.headroom_timeout_ms = value;
        }
        if let Some(value) = self.terse_enabled {
            config.token_saver.terse_enabled = value;
        }
        if let Some(value) = self.caveman_enabled {
            config.token_saver.caveman_enabled = value;
        }
        if let Some(value) = &self.caveman_level {
            config.token_saver.caveman_level = value.clone();
        }
        if let Some(value) = self.ponytail_enabled {
            config.token_saver.ponytail_enabled = value;
        }
        if let Some(value) = &self.ponytail_level {
            config.token_saver.ponytail_level = value.clone();
        }
    }

    /// Applies the overrides over `base` and rejects unsafe results.
    pub fn merge(&self, base: &RouterConfig) -> Result<RouterConfig> {
        let mut config = base.clone();
        self.apply(&mut config);
        config.validate()?;
        Ok(config)
    }

    pub fn is_empty(&self) -> bool {
        self.keys().is_empty()
    }

    /// Dotted keys of every field an override is stored for, e.g.
    /// `server.require_api_key`. The dashboard renders these as "custom".
    pub fn keys(&self) -> Vec<&'static str> {
        let mut keys = Vec::new();
        if self.require_api_key.is_some() {
            keys.push("server.require_api_key");
        }
        if self.admin_token.is_some() {
            keys.push("server.admin_token");
        }
        if self.readiness_upstream_checks.is_some() {
            keys.push("server.readiness_upstream_checks");
        }
        if self.public_usage.is_some() {
            keys.push("server.public_usage");
        }
        if self.store_key_secrets.is_some() {
            keys.push("server.store_key_secrets");
        }
        if self.lan_access.is_some() {
            keys.push("server.lan_access");
        }
        if self.cors_origins.is_some() {
            keys.push("server.cors_origins");
        }
        if self.default_connection.is_some() {
            keys.push("router.default_connection");
        }
        if self.max_attempts.is_some() {
            keys.push("router.max_attempts");
        }
        if self.max_retries_per_tier.is_some() {
            keys.push("router.max_retries_per_tier");
        }
        if self.max_retry_delay_ms.is_some() {
            keys.push("router.max_retry_delay_ms");
        }
        if self.catalog_ttl_ms.is_some() {
            keys.push("router.catalog_ttl_ms");
        }
        if self.connect_timeout_ms.is_some() {
            keys.push("router.connect_timeout_ms");
        }
        if self.idle_timeout_ms.is_some() {
            keys.push("router.idle_timeout_ms");
        }
        if self.max_concurrent.is_some() {
            keys.push("limits.max_concurrent");
        }
        if self.max_concurrent_per_connection.is_some() {
            keys.push("limits.max_concurrent_per_connection");
        }
        if self.acquire_timeout_ms.is_some() {
            keys.push("limits.acquire_timeout_ms");
        }
        if self.requests_per_minute.is_some() {
            keys.push("rate_limit.requests_per_minute");
        }
        if self.burst.is_some() {
            keys.push("rate_limit.burst");
        }
        if self.pricing_sync_enabled.is_some() {
            keys.push("pricing.sync_enabled");
        }
        if self.pricing_sync_interval_secs.is_some() {
            keys.push("pricing.sync_interval_secs");
        }
        if self.pricing_source_url.is_some() {
            keys.push("pricing.source_url");
        }
        if self.slimmer_enabled.is_some() {
            keys.push("token_saver.slimmer_enabled");
        }
        if self.slimmer_level.is_some() {
            keys.push("token_saver.slimmer_level");
        }
        if self.headroom_enabled.is_some() {
            keys.push("token_saver.headroom_enabled");
        }
        if self.headroom_url.is_some() {
            keys.push("token_saver.headroom_url");
        }
        if self.headroom_timeout_ms.is_some() {
            keys.push("token_saver.headroom_timeout_ms");
        }
        if self.terse_enabled.is_some() {
            keys.push("token_saver.terse_enabled");
        }
        if self.caveman_enabled.is_some() {
            keys.push("token_saver.caveman_enabled");
        }
        if self.caveman_level.is_some() {
            keys.push("token_saver.caveman_level");
        }
        if self.ponytail_enabled.is_some() {
            keys.push("token_saver.ponytail_enabled");
        }
        if self.ponytail_level.is_some() {
            keys.push("token_saver.ponytail_level");
        }
        keys
    }
}

/// Reads and writes the single `settings` row.
pub struct SettingsRepository {
    pool: SqlitePool,
}

impl SettingsRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Stored overrides; an absent row means "nothing customized".
    pub async fn get(&self) -> Result<SettingsOverrides> {
        let row: Option<(String,)> = sqlx::query_as("SELECT data FROM settings WHERE id = 1")
            .fetch_optional(&self.pool)
            .await?;

        match row {
            Some((data,)) => serde_json::from_str(&data)
                .map_err(|error| Error::Internal(format!("invalid stored settings: {error}"))),
            None => Ok(SettingsOverrides::default()),
        }
    }

    pub async fn save(&self, overrides: &SettingsOverrides) -> Result<()> {
        let data = serde_json::to_string(overrides)
            .map_err(|error| Error::Internal(format!("cannot encode settings: {error}")))?;

        sqlx::query(
            "INSERT INTO settings (id, data, updated_at) VALUES (1, ?, ?)
             ON CONFLICT(id) DO UPDATE SET data = excluded.data, updated_at = excluded.updated_at",
        )
        .bind(&data)
        .bind(chrono::Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Drops every override, restoring the file/env configuration.
    pub async fn clear(&self) -> Result<()> {
        sqlx::query("DELETE FROM settings WHERE id = 1")
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SecretsConfig;
    use crate::db::Db;

    fn config() -> RouterConfig {
        RouterConfig {
            secrets: SecretsConfig {
                key: Some("ab".repeat(32)),
            },
            ..RouterConfig::default()
        }
    }

    #[test]
    fn overrides_replace_configured_values() {
        let mut config = config();
        config.server.require_api_key = false;
        config.router.max_attempts = 5;

        let overrides = SettingsOverrides {
            require_api_key: Some(true),
            max_attempts: Some(2),
            ..SettingsOverrides::default()
        };

        overrides.apply(&mut config);
        assert!(config.server.require_api_key);
        assert_eq!(config.router.max_attempts, 2);
        assert_eq!(
            overrides.keys(),
            vec!["server.require_api_key", "router.max_attempts"]
        );
    }

    #[test]
    fn null_overrides_force_empty_values() {
        let mut config = config();
        config.server.admin_token = Some("secret".to_string());
        config.router.default_connection = Some("legacy".to_string());

        let overrides = SettingsOverrides {
            admin_token: Some(None),
            default_connection: Some(None),
            ..SettingsOverrides::default()
        };

        overrides.apply(&mut config);
        assert!(config.server.admin_token().is_none());
        assert!(config.router.default_connection.is_none());
    }

    #[test]
    fn merge_rejects_clearing_the_admin_token_on_a_public_bind() {
        let mut base = config();
        base.server.host = "0.0.0.0".to_string();
        base.server.admin_token = Some("secret".to_string());

        let overrides = SettingsOverrides {
            admin_token: Some(None),
            ..SettingsOverrides::default()
        };

        assert!(overrides.merge(&base).is_err());
    }

    #[tokio::test]
    async fn repository_round_trips_overrides() {
        let db = Db::connect_in_memory().await.expect("db");
        let repository = SettingsRepository::new(db.pool.clone());

        assert!(repository.get().await.expect("empty").is_empty());

        let overrides = SettingsOverrides {
            require_api_key: Some(true),
            default_connection: Some(None),
            ..SettingsOverrides::default()
        };
        repository.save(&overrides).await.expect("save");

        let loaded = repository.get().await.expect("load");
        assert_eq!(loaded.require_api_key, Some(true));
        assert_eq!(loaded.default_connection, Some(None));

        repository.clear().await.expect("clear");
        assert!(repository.get().await.expect("cleared").is_empty());
    }
}
