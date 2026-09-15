//! Dashboard-managed runtime settings: read, patch, and reset.

use axum::Json;
use axum::extract::State;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};

use crate::config::RouterConfig;
use crate::error::{Error, Result};
use crate::settings::SettingsOverrides;
use crate::state::AppState;

/// `PATCH /api/settings` body. Absent fields stay untouched; `null` forces an
/// empty value for `admin_token` and `default_connection`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct SettingsPatch {
    pub require_api_key: Option<bool>,
    #[serde(deserialize_with = "crate::db::repos::double_option")]
    pub admin_token: Option<Option<String>>,
    pub readiness_upstream_checks: Option<bool>,
    pub public_usage: Option<bool>,
    #[serde(deserialize_with = "crate::db::repos::double_option")]
    pub default_connection: Option<Option<String>>,
    pub max_attempts: Option<usize>,
    pub max_retries_per_tier: Option<usize>,
    pub max_retry_delay_ms: Option<u64>,
    pub catalog_ttl_ms: Option<u64>,
    pub connect_timeout_ms: Option<u64>,
    pub idle_timeout_ms: Option<u64>,
    pub max_concurrent: Option<usize>,
    pub max_concurrent_per_connection: Option<usize>,
    pub acquire_timeout_ms: Option<u64>,
    pub requests_per_minute: Option<u32>,
    pub burst: Option<u32>,
    pub pricing_sync_enabled: Option<bool>,
    pub pricing_sync_interval_secs: Option<u64>,
    pub pricing_source_url: Option<String>,
}

impl SettingsPatch {
    /// Merges the patch into the stored overrides. Blank strings force the
    /// nullable values to empty; nonsensical combinations are rejected.
    fn merge_into(self, overrides: &mut SettingsOverrides) -> Result<()> {
        fn set<T>(target: &mut Option<T>, value: Option<T>) {
            if let Some(value) = value {
                *target = Some(value);
            }
        }

        set(&mut overrides.require_api_key, self.require_api_key);

        if let Some(token) = self.admin_token {
            overrides.admin_token = Some(non_blank(token));
        }
        set(
            &mut overrides.readiness_upstream_checks,
            self.readiness_upstream_checks,
        );
        set(&mut overrides.public_usage, self.public_usage);
        if let Some(connection) = self.default_connection {
            overrides.default_connection = Some(non_blank(connection));
        }

        if let Some(value) = self.max_attempts {
            if value == 0 {
                return Err(Error::BadRequest(
                    "router.max_attempts must be at least 1".to_string(),
                ));
            }
            overrides.max_attempts = Some(value);
        }
        set(
            &mut overrides.max_retries_per_tier,
            self.max_retries_per_tier,
        );
        set(&mut overrides.max_retry_delay_ms, self.max_retry_delay_ms);
        set(&mut overrides.catalog_ttl_ms, self.catalog_ttl_ms);
        set(&mut overrides.connect_timeout_ms, self.connect_timeout_ms);
        set(&mut overrides.idle_timeout_ms, self.idle_timeout_ms);
        set(&mut overrides.max_concurrent, self.max_concurrent);
        set(
            &mut overrides.max_concurrent_per_connection,
            self.max_concurrent_per_connection,
        );
        set(&mut overrides.acquire_timeout_ms, self.acquire_timeout_ms);
        set(&mut overrides.requests_per_minute, self.requests_per_minute);
        set(&mut overrides.burst, self.burst);
        set(
            &mut overrides.pricing_sync_enabled,
            self.pricing_sync_enabled,
        );
        set(
            &mut overrides.pricing_sync_interval_secs,
            self.pricing_sync_interval_secs,
        );

        if let Some(url) = self.pricing_source_url {
            let url = url.trim().to_string();
            if url.is_empty() {
                return Err(Error::BadRequest(
                    "pricing.source_url cannot be empty".to_string(),
                ));
            }
            overrides.pricing_source_url = Some(url);
        }

        Ok(())
    }
}

/// Trims a value, treating blank as absent.
fn non_blank(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

#[derive(Debug, Serialize)]
pub struct ServerSettingsView {
    pub require_api_key: bool,
    pub readiness_upstream_checks: bool,
    pub public_usage: bool,
    pub admin_token_set: bool,
}

#[derive(Debug, Serialize)]
pub struct RouterSettingsView {
    pub default_connection: Option<String>,
    pub max_attempts: usize,
    pub max_retries_per_tier: usize,
    pub max_retry_delay_ms: u64,
    pub catalog_ttl_ms: u64,
    pub connect_timeout_ms: u64,
    pub idle_timeout_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct LimitsSettingsView {
    pub max_concurrent: usize,
    pub max_concurrent_per_connection: usize,
    pub acquire_timeout_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct RateLimitSettingsView {
    pub requests_per_minute: u32,
    pub burst: u32,
}

#[derive(Debug, Serialize)]
pub struct PricingSettingsView {
    pub sync_enabled: bool,
    pub sync_interval_secs: u64,
    pub source_url: String,
}

/// Values that only change by editing `config.toml` and restarting; reported so
/// the dashboard can show the full picture without pretending to edit them.
#[derive(Debug, Serialize)]
pub struct DeploymentView {
    pub host: String,
    pub port: u16,
    pub binds_loopback: bool,
    pub serve_dashboard: bool,
    pub tray: bool,
    pub allow_unauthenticated_admin: bool,
    pub cors_origins: Vec<String>,
    pub database_url: String,
    pub secrets_key_set: bool,
}

#[derive(Debug, Serialize)]
pub struct SettingsResponse {
    pub server: ServerSettingsView,
    pub router: RouterSettingsView,
    pub limits: LimitsSettingsView,
    pub rate_limit: RateLimitSettingsView,
    pub pricing: PricingSettingsView,
    pub overrides: Vec<&'static str>,
    pub deployment: DeploymentView,
}

impl SettingsResponse {
    fn build(config: &RouterConfig, overrides: &SettingsOverrides) -> Self {
        Self {
            server: ServerSettingsView {
                require_api_key: config.server.require_api_key,
                readiness_upstream_checks: config.server.readiness_upstream_checks,
                public_usage: config.server.public_usage,
                admin_token_set: config.server.requires_admin_token(),
            },
            router: RouterSettingsView {
                default_connection: config.router.default_connection.clone(),
                max_attempts: config.router.max_attempts,
                max_retries_per_tier: config.router.max_retries_per_tier,
                max_retry_delay_ms: config.router.max_retry_delay_ms,
                catalog_ttl_ms: config.router.catalog_ttl_ms,
                connect_timeout_ms: config.router.connect_timeout_ms,
                idle_timeout_ms: config.router.idle_timeout_ms,
            },
            limits: LimitsSettingsView {
                max_concurrent: config.limits.max_concurrent,
                max_concurrent_per_connection: config.limits.max_concurrent_per_connection,
                acquire_timeout_ms: config.limits.acquire_timeout_ms,
            },
            rate_limit: RateLimitSettingsView {
                requests_per_minute: config.rate_limit.requests_per_minute,
                burst: config.rate_limit.burst,
            },
            pricing: PricingSettingsView {
                sync_enabled: config.pricing.sync_enabled,
                sync_interval_secs: config.pricing.sync_interval_secs,
                source_url: config.pricing.source_url.clone(),
            },
            overrides: overrides.keys(),
            deployment: DeploymentView {
                host: config.server.host.clone(),
                port: config.server.port,
                binds_loopback: config.server.binds_loopback(),
                serve_dashboard: config.server.serve_dashboard,
                tray: config.server.tray,
                allow_unauthenticated_admin: config.server.allow_unauthenticated_admin,
                cors_origins: config.server.cors_origins.clone(),
                database_url: config.database_url(),
                secrets_key_set: config.secrets.key.is_some(),
            },
        }
    }
}

/// `GET /api/settings` — effective values plus the override list.
pub async fn get_settings(State(state): State<AppState>) -> Result<impl IntoResponse> {
    let overrides = state.settings().get().await?;
    let config = state.config_snapshot();
    Ok(Json(SettingsResponse::build(&config, &overrides)))
}

/// `PATCH /api/settings` — persists overrides and hot-applies them.
pub async fn update_settings(
    State(state): State<AppState>,
    Json(patch): Json<SettingsPatch>,
) -> Result<impl IntoResponse> {
    let mut overrides = state.settings().get().await?;
    patch.merge_into(&mut overrides)?;

    // Reject unsafe results before they are persisted.
    if let Err(error) = state.merged_config(&overrides) {
        return Err(Error::BadRequest(error.to_string()));
    }
    state.settings().save(&overrides).await?;

    let config = state.apply_overrides(&overrides).await?;
    Ok(Json(SettingsResponse::build(&config, &overrides)))
}

/// `DELETE /api/settings` — drops every override, restoring file/env values.
pub async fn reset_settings(State(state): State<AppState>) -> Result<impl IntoResponse> {
    state.settings().clear().await?;
    let overrides = SettingsOverrides::default();
    let config = state.apply_overrides(&overrides).await?;
    Ok(Json(SettingsResponse::build(&config, &overrides)))
}
