//! Effective key rules: plan/key merge plus the model allowlist matcher.
//!
//! A key's own fields win; the plan fills what the key leaves empty. For the
//! budget the key's amount and mode travel together, so a plan budget only
//! applies when the key sets no amount of its own.

use crate::db::repos::api_keys::ApiKey;
use crate::db::repos::key_plans::KeyPlan;
use crate::error::{Error, Result};
use crate::limits::BudgetMode;

/// Effective rules for one authenticated key.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KeyPolicy {
    /// Model patterns; empty means "any model".
    pub allowed_models: Vec<String>,
    pub rate_limit_per_minute: Option<u32>,
    pub monthly_budget_usd: Option<f64>,
    pub budget_mode: BudgetMode,
}

impl KeyPolicy {
    /// Merges a key with its plan: fields set on the key win.
    pub fn resolve(key: &ApiKey, plan: Option<&KeyPlan>) -> Self {
        let allowed_models = key
            .allowed_models()
            .map(<[String]>::to_vec)
            .or_else(|| plan.map(|plan| plan.allowed_models().to_vec()))
            .unwrap_or_default();

        // A key budget without its own mode means "cap disabled": keep the pair
        // together instead of borrowing the plan's mode.
        let (monthly_budget_usd, budget_mode) = match key.budget_usd() {
            Some(amount) => (Some(amount), key.budget_mode()),
            None => match plan.and_then(KeyPlan::budget_usd) {
                Some(amount) => (
                    Some(amount),
                    plan.map(KeyPlan::budget_mode).unwrap_or_default(),
                ),
                None => (None, BudgetMode::Off),
            },
        };

        Self {
            allowed_models,
            rate_limit_per_minute: key
                .rate_limit()
                .or_else(|| plan.and_then(KeyPlan::rate_limit)),
            monthly_budget_usd,
            budget_mode,
        }
    }

    /// True when no allowlist is configured.
    pub fn unrestricted(&self) -> bool {
        self.allowed_models.is_empty()
    }

    /// True when `model` matches the allowlist.
    ///
    /// Patterns are case-insensitive: `*` allows everything, `prefix/*` allows
    /// the prefix and anything under it, anything else must match exactly.
    pub fn allows(&self, model: &str) -> bool {
        if self.unrestricted() {
            return true;
        }

        let requested = model.trim().to_lowercase();
        self.allowed_models
            .iter()
            .any(|pattern| matches_pattern(pattern, &requested))
    }

    /// Fails with `403` when the model is outside the allowlist.
    pub fn ensure_model(&self, model: &str) -> Result<()> {
        if self.allows(model) {
            return Ok(());
        }
        Err(Error::Forbidden(format!(
            "model '{model}' is not allowed for this API key"
        )))
    }
}

/// Matches one allowlist pattern against a lowercased model reference.
fn matches_pattern(pattern: &str, requested: &str) -> bool {
    let pattern = pattern.trim().to_lowercase();

    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix("/*") {
        return !prefix.is_empty() && requested.starts_with(&format!("{prefix}/"));
    }

    pattern == requested
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::types::Json;

    fn key(allowed: Option<&[&str]>, rate: Option<i64>, budget: Option<f64>, mode: &str) -> ApiKey {
        ApiKey {
            id: "key-1".to_string(),
            name: "test".to_string(),
            key_hash: "hash".to_string(),
            prefix: "sk-router-".to_string(),
            enabled: 1,
            rate_limit_per_minute: rate,
            monthly_budget_usd: budget,
            budget_mode: mode.to_string(),
            plan_id: None,
            allowed_models: allowed
                .map(|models| Json(models.iter().map(|m| m.to_string()).collect())),
            created_at: chrono::Utc::now(),
            last_used_at: None,
        }
    }

    fn plan(allowed: &[&str], rate: Option<i64>, budget: Option<f64>, mode: &str) -> KeyPlan {
        KeyPlan {
            id: "plan-1".to_string(),
            name: "team".to_string(),
            description: String::new(),
            allowed_models: Json(allowed.iter().map(|m| m.to_string()).collect()),
            rate_limit_per_minute: rate,
            monthly_budget_usd: budget,
            budget_mode: mode.to_string(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn key_fields_win_over_the_plan() {
        let key = key(Some(&["key-only"]), Some(10), Some(5.0), "block");
        let plan = plan(&["plan-only"], Some(99), Some(99.0), "warn");

        let policy = KeyPolicy::resolve(&key, Some(&plan));

        assert_eq!(policy.allowed_models, vec!["key-only".to_string()]);
        assert_eq!(policy.rate_limit_per_minute, Some(10));
        assert_eq!(policy.monthly_budget_usd, Some(5.0));
        assert_eq!(policy.budget_mode, BudgetMode::Block);
    }

    #[test]
    fn plan_fills_everything_the_key_leaves_empty() {
        let key = key(None, None, None, "off");
        let plan = plan(&["openai/*"], Some(30), Some(20.0), "block");

        let policy = KeyPolicy::resolve(&key, Some(&plan));

        assert_eq!(policy.allowed_models, vec!["openai/*".to_string()]);
        assert_eq!(policy.rate_limit_per_minute, Some(30));
        assert_eq!(policy.monthly_budget_usd, Some(20.0));
        assert_eq!(policy.budget_mode, BudgetMode::Block);
    }

    #[test]
    fn key_budget_without_a_mode_disables_the_plan_cap() {
        let key = key(None, None, Some(7.0), "off");
        let plan = plan(&[], None, Some(20.0), "block");

        let policy = KeyPolicy::resolve(&key, Some(&plan));

        assert_eq!(policy.monthly_budget_usd, Some(7.0));
        assert_eq!(policy.budget_mode, BudgetMode::Off);
    }

    #[test]
    fn without_any_rules_every_model_is_allowed() {
        let policy = KeyPolicy::resolve(&key(None, None, None, "off"), None);

        assert!(policy.unrestricted());
        assert!(policy.ensure_model("anything/at-all").is_ok());
    }

    #[test]
    fn exact_patterns_match_case_insensitively() {
        let policy = KeyPolicy::resolve(&key(Some(&["GPT-4o"]), None, None, "off"), None);

        assert!(policy.allows("gpt-4o"));
        assert!(policy.allows("GPT-4O"));
        assert!(!policy.allows("gpt-4o-mini"));
        assert!(policy.ensure_model("gpt-4o-mini").is_err());
    }

    #[test]
    fn wildcards_match_the_prefix_and_its_children_only() {
        let policy = KeyPolicy::resolve(&key(Some(&["openai/*", "*"]), None, None, "off"), None);

        assert!(policy.allows("openai/gpt-4o"));
        assert!(policy.allows("anything"));
        assert!(policy.allows(""));

        let prefixed = KeyPolicy::resolve(&key(Some(&["openai/*"]), None, None, "off"), None);
        assert!(prefixed.allows("openai/gpt-4o"));
        assert!(prefixed.allows("OPENAI/GPT-4O"));
        assert!(!prefixed.allows("openai"));
        assert!(!prefixed.allows("anthropic/claude"));
    }

    #[test]
    fn forbidden_errors_are_403() {
        let policy = KeyPolicy::resolve(&key(Some(&["openai/*"]), None, None, "off"), None);

        let error = policy.ensure_model("anthropic/claude").expect_err("denied");
        assert_eq!(error.status(), axum::http::StatusCode::FORBIDDEN);
    }
}
