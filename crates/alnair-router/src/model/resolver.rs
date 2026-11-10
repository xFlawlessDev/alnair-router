//! Pure resolution of model references against a loaded catalog snapshot.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::db::repos::aliases::Alias;
use crate::db::repos::combos::{Combo, ComboEntry};
use crate::db::repos::connections::Connection;
use crate::error::{Error, Result};

/// Maximum combo nesting depth before resolution is abandoned.
pub const MAX_DEPTH: usize = 8;

/// One upstream the request can be dispatched to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTarget {
    /// Source connection id, used for per-connection concurrency caps.
    pub connection_id: String,
    /// Source connection name, used in logs and the live activity view.
    pub connection_name: String,
    /// `openai-compatible` or `anthropic-native`.
    pub provider_type: String,
    pub base_url: String,
    /// Path appended to `base_url` for a search request; `None` means `/search`.
    pub search_path: Option<String>,
    /// Search fallback tier, when the connection is a search provider.
    pub search_tier: Option<i64>,
    pub model: String,
    /// Credentials to try for this target, in rotation order. The primary
    /// connection key comes first, then enabled extra accounts.
    pub api_keys: Vec<String>,
    pub custom_headers: BTreeMap<String, String>,
    /// Connect/first-byte timeout override, in milliseconds; `None` inherits the
    /// router default and `Some(0)` disables the timeout.
    pub connect_timeout_ms: Option<u64>,
    /// Stream idle timeout override, in milliseconds; `None` inherits.
    pub idle_timeout_ms: Option<u64>,
    /// Model id used for price lookups when the upstream id differs from the
    /// catalog; `None` uses `model`.
    pub pricing_model: Option<String>,
    /// Prompt-cache retention sent upstream: `none`, `short` or `long`.
    pub cache_retention: String,
    /// How the credential is presented: `api_key` (provider default) or
    /// `bearer`. OAuth/subscription session tokens require `bearer`.
    pub auth_style: String,
    /// OpenAI wire API this connection speaks: `chat` or `responses`. Only read
    /// for an openai-compatible target.
    pub wire_api: String,
    /// OAuth accounts backing this target, in rotation order, when the
    /// connection authenticates with OAuth instead of a static key. Only ids
    /// travel here; the executor resolves each token just in time so the cached
    /// catalog never holds a short-lived secret.
    pub oauth_account_ids: Vec<String>,
    /// Provenance, e.g. `alias:glm` or `combo:free-forever#2`.
    pub source: String,
}

/// Immutable snapshot of routing config used to resolve references.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub connections: Vec<Connection>,
    pub aliases: Vec<Alias>,
    pub combos: Vec<Combo>,
    pub combo_entries: Vec<ComboEntry>,
}

impl ResolvedTarget {
    /// First configured key. Non-rotating callers (media proxying) use this.
    pub fn primary_key(&self) -> Option<&str> {
        self.api_keys.first().map(String::as_str)
    }
}

/// Resolves model references against a [`Catalog`].
#[derive(Debug, Clone)]
pub struct Resolver {
    connections_by_name: HashMap<String, Connection>,
    connections_by_id: HashMap<String, Connection>,
    aliases_by_prefix: HashMap<String, Alias>,
    combos_by_name: HashMap<String, Combo>,
    /// Every combo name, including disabled ones, so a disabled combo is not
    /// mistaken for a bare model name.
    known_combos: HashSet<String>,
    entries_by_combo: HashMap<String, Vec<ComboEntry>>,
    default_connection: Option<String>,
    max_attempts: usize,
}

impl Catalog {
    /// Builds a resolver over this snapshot.
    pub fn resolver(&self, default_connection: Option<String>, max_attempts: usize) -> Resolver {
        let enabled: Vec<Connection> = self
            .connections
            .iter()
            .filter(|c| c.is_enabled())
            .cloned()
            .collect();

        let connections_by_name = enabled
            .iter()
            .map(|c| (c.name.clone(), c.clone()))
            .collect();
        let connections_by_id = enabled.iter().map(|c| (c.id.clone(), c.clone())).collect();

        let aliases_by_prefix = self
            .aliases
            .iter()
            .filter(|a| a.is_enabled())
            .map(|a| (a.prefix.clone(), a.clone()))
            .collect();

        let known_combos = self
            .combos
            .iter()
            .map(|c| c.name.to_ascii_lowercase())
            .collect();

        let combos_by_name = self
            .combos
            .iter()
            .filter(|c| c.is_enabled())
            .map(|c| (c.name.clone(), c.clone()))
            .collect();

        let mut entries_by_combo: HashMap<String, Vec<ComboEntry>> = HashMap::new();
        for entry in self.combo_entries.iter().filter(|e| e.is_enabled()) {
            entries_by_combo
                .entry(entry.combo_id.clone())
                .or_default()
                .push(entry.clone());
        }
        for entries in entries_by_combo.values_mut() {
            entries.sort_by_key(|e| e.position);
        }

        Resolver {
            connections_by_name,
            connections_by_id,
            aliases_by_prefix,
            combos_by_name,
            known_combos,
            entries_by_combo,
            default_connection,
            max_attempts: max_attempts.max(1),
        }
    }
}

impl Resolver {
    /// Every enabled search provider or connection with a search tier, as a target
    /// with no model. Search providers have no model, so the model field stays
    /// empty and the upstream path comes from `search_path`.
    pub fn search_targets(&self) -> Vec<ResolvedTarget> {
        let mut targets: Vec<ResolvedTarget> = self
            .connections_by_id
            .values()
            .filter(|connection| {
                connection.search_tier.is_some()
                    || crate::providers::search_preset(connection.provider_id.as_deref()).is_some()
            })
            .map(|connection| target_from(connection, String::new(), "search".to_string()))
            .collect();
        // Stable order inside a tier, so the round-robin rotation is predictable.
        targets.sort_by(|a, b| a.connection_name.cmp(&b.connection_name));
        targets
    }

    /// Resolves `reference` into an ordered attempt list.
    ///
    /// A combo expands recursively (bounded by [`MAX_DEPTH`]); cycles are
    /// broken by tracking the combo names on the current path.
    pub fn resolve(&self, reference: &str) -> Result<Vec<ResolvedTarget>> {
        let reference = reference.trim();
        if reference.is_empty() {
            return Err(Error::BadRequest("model is required".to_string()));
        }

        let mut targets = Vec::new();
        let mut path = HashSet::new();
        self.resolve_inner(reference, &mut path, &mut targets, 0)?;

        if targets.is_empty() {
            return Err(Error::UnknownModel(reference.to_string()));
        }
        targets.truncate(self.max_attempts);
        Ok(targets)
    }

    fn resolve_inner(
        &self,
        reference: &str,
        path: &mut HashSet<String>,
        out: &mut Vec<ResolvedTarget>,
        depth: usize,
    ) -> Result<()> {
        if depth > MAX_DEPTH {
            return Err(Error::BadRequest(format!(
                "combo nesting exceeds maximum depth of {MAX_DEPTH}"
            )));
        }

        // 1. Combo by name.
        let combo_key = reference.to_ascii_lowercase();
        if self.known_combos.contains(&combo_key) {
            let Some(combo) = self.combos_by_name.get(&combo_key) else {
                return Err(Error::NoRoute(format!("combo '{reference}' is disabled")));
            };

            if !path.insert(combo_key.clone()) {
                tracing::warn!(combo = %combo.name, "combo cycle detected; skipping");
                return Ok(());
            }

            let entries = self
                .entries_by_combo
                .get(&combo.id)
                .cloned()
                .unwrap_or_default();

            for entry in &entries {
                let mut nested = Vec::new();
                self.resolve_inner(&entry.model_ref, path, &mut nested, depth + 1)?;
                for mut target in nested {
                    // Tier number reflects the declared position, so disabling an
                    // entry does not renumber the remaining tiers.
                    target.source = format!("combo:{}#{}", combo.name, entry.position + 1);
                    out.push(target);
                }
            }

            path.remove(&combo_key);
            return Ok(());
        }

        // 2. Alias by prefix.
        if let Some((prefix, model)) = reference.split_once('/') {
            let prefix = prefix.trim().to_ascii_lowercase();
            let model = model.trim();
            if model.is_empty() {
                return Err(Error::BadRequest(format!(
                    "model reference '{reference}' has an empty model segment"
                )));
            }
            if let Some(alias) = self.aliases_by_prefix.get(&prefix) {
                let connection = self
                    .connections_by_id
                    .get(&alias.connection_id)
                    .ok_or_else(|| {
                        Error::NoRoute(format!(
                            "alias '{prefix}' points at a disabled or missing connection"
                        ))
                    })?;

                let model = alias
                    .model_override
                    .clone()
                    .unwrap_or_else(|| model.to_string());

                let mut target = target_from(connection, model, format!("alias:{prefix}"));
                if alias.pricing_model.is_some() {
                    target.pricing_model = alias.pricing_model.clone();
                }
                out.push(target);
                return Ok(());
            }
        }

        // 3. A bare alias prefix, but only when it pins a model: the override
        //    makes the model unambiguous, so `kr` can stand in for `kr/anything`.
        if !reference.contains('/')
            && let Some(alias) = self.aliases_by_prefix.get(&reference.to_ascii_lowercase())
            && let Some(model) = alias.model_override.clone()
        {
            let connection = self
                .connections_by_id
                .get(&alias.connection_id)
                .ok_or_else(|| {
                    Error::NoRoute(format!(
                        "alias '{reference}' points at a disabled or missing connection"
                    ))
                })?;

            let mut target = target_from(connection, model, format!("alias:{}", alias.prefix));
            if alias.pricing_model.is_some() {
                target.pricing_model = alias.pricing_model.clone();
            }
            out.push(target);
            return Ok(());
        }

        // 4. Bare model name → default connection.
        if !reference.contains('/')
            && let Some(name) = &self.default_connection
            && let Some(connection) = self.connections_by_name.get(name)
        {
            out.push(target_from(
                connection,
                reference.to_string(),
                format!("default:{name}"),
            ));
            return Ok(());
        }

        Err(Error::UnknownModel(reference.to_string()))
    }
}

fn target_from(connection: &Connection, model: String, source: String) -> ResolvedTarget {
    let preset = crate::providers::search_preset(connection.provider_id.as_deref());
    let mut api_keys = Vec::new();
    if let Some(key) = &connection.api_key
        && !key.trim().is_empty()
    {
        api_keys.push(key.clone());
    }
    api_keys.extend(connection.extra_keys.iter().cloned());

    ResolvedTarget {
        connection_id: connection.id.clone(),
        connection_name: connection.name.clone(),
        provider_type: connection.provider_type.clone(),
        base_url: connection.base_url.clone(),
        search_path: connection.search_path.clone(),
        search_tier: connection.search_tier.or_else(|| {
            preset
                .as_ref()
                .and_then(|preset| preset.default_search_tier)
        }),
        model,
        api_keys,
        custom_headers: connection.headers(),
        connect_timeout_ms: non_negative(connection.connect_timeout_ms),
        idle_timeout_ms: non_negative(connection.idle_timeout_ms),
        pricing_model: connection.pricing_model.clone(),
        cache_retention: connection.cache_retention.clone(),
        auth_style: connection.auth_style.clone(),
        wire_api: connection.wire_api.clone(),
        oauth_account_ids: connection.oauth_account_ids.clone(),
        source,
    }
}

fn non_negative(value: Option<i64>) -> Option<u64> {
    value.and_then(|value| u64::try_from(value).ok())
}
