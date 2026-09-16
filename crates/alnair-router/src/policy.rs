//! Effective key rules: plan/key merge plus the model allowlist matcher.
//!
//! A key's own fields win; the plan fills what the key leaves empty. Budgets
//! and token limits merge per window, so a key can add a daily cap on top of a
//! plan's monthly one. The budget mode travels with the key's own limits: as
//! soon as the key sets any amount, its mode decides — a key amount with mode
//! `off` disables every cap, including the plan's.

use crate::db::repos::api_keys::ApiKey;
use crate::db::repos::key_plans::KeyPlan;
use crate::error::{Error, Result};
use crate::limits::{BudgetMode, BudgetWindows, TokenWindows};

/// Effective rules for one authenticated key.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KeyPolicy {
    /// Model patterns; empty means "any model".
    pub allowed_models: Vec<String>,
    pub rate_limit_per_minute: Option<u32>,
    /// Spend caps per calendar window; empty means uncapped.
    pub budgets: BudgetWindows,
    /// Token caps per calendar window; empty means uncapped.
    pub token_limits: TokenWindows,
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

        let key_budgets = key.budget_windows();
        let plan_budgets = plan.map(KeyPlan::budget_windows).unwrap_or_default();
        let budgets = BudgetWindows::from_parts(
            key_budgets.daily.or(plan_budgets.daily),
            key_budgets.weekly.or(plan_budgets.weekly),
            key_budgets.monthly.or(plan_budgets.monthly),
            key_budgets.lifetime.or(plan_budgets.lifetime),
        );

        let key_tokens = key.token_windows();
        let plan_tokens = plan.map(KeyPlan::token_windows).unwrap_or_default();
        let token_limits = TokenWindows::from_parts(
            key_tokens.daily.or(plan_tokens.daily),
            key_tokens.weekly.or(plan_tokens.weekly),
            key_tokens.monthly.or(plan_tokens.monthly),
            key_tokens.lifetime.or(plan_tokens.lifetime),
        );

        // The mode belongs to whoever supplies the limits. A key amount with
        // no mode means "cap disabled", matching the pre-window behaviour.
        let key_supplies_limits = !key_budgets.is_empty() || !key_tokens.is_empty();
        let plan_supplies_limits = !plan_budgets.is_empty() || !plan_tokens.is_empty();
        let budget_mode = if key_supplies_limits {
            key.budget_mode()
        } else if plan_supplies_limits {
            plan.map(KeyPlan::budget_mode).unwrap_or_default()
        } else {
            BudgetMode::Off
        };

        Self {
            allowed_models,
            rate_limit_per_minute: key
                .rate_limit()
                .or_else(|| plan.and_then(KeyPlan::rate_limit)),
            budgets,
            token_limits,
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

    fn key(
        allowed: Option<&[&str]>,
        rate: Option<i64>,
        budgets: BudgetWindows,
        mode: &str,
    ) -> ApiKey {
        key_with_tokens(allowed, rate, budgets, TokenWindows::default(), mode)
    }

    fn key_with_tokens(
        allowed: Option<&[&str]>,
        rate: Option<i64>,
        budgets: BudgetWindows,
        tokens: TokenWindows,
        mode: &str,
    ) -> ApiKey {
        ApiKey {
            id: "key-1".to_string(),
            name: "test".to_string(),
            key_hash: "hash".to_string(),
            secret_enc: None,
            prefix: "sk-router-".to_string(),
            enabled: 1,
            rate_limit_per_minute: rate,
            daily_budget_usd: budgets.daily,
            weekly_budget_usd: budgets.weekly,
            monthly_budget_usd: budgets.monthly,
            lifetime_budget_usd: budgets.lifetime,
            daily_token_limit: tokens.daily,
            weekly_token_limit: tokens.weekly,
            monthly_token_limit: tokens.monthly,
            lifetime_token_limit: tokens.lifetime,
            budget_mode: mode.to_string(),
            plan_id: None,
            allowed_models: allowed
                .map(|models| Json(models.iter().map(|m| m.to_string()).collect())),
            created_at: chrono::Utc::now(),
            last_used_at: None,
            expires_at: None,
        }
    }

    fn plan(allowed: &[&str], rate: Option<i64>, budgets: BudgetWindows, mode: &str) -> KeyPlan {
        plan_with_tokens(allowed, rate, budgets, TokenWindows::default(), mode)
    }

    fn plan_with_tokens(
        allowed: &[&str],
        rate: Option<i64>,
        budgets: BudgetWindows,
        tokens: TokenWindows,
        mode: &str,
    ) -> KeyPlan {
        KeyPlan {
            id: "plan-1".to_string(),
            name: "team".to_string(),
            description: String::new(),
            allowed_models: Json(allowed.iter().map(|m| m.to_string()).collect()),
            rate_limit_per_minute: rate,
            daily_budget_usd: budgets.daily,
            weekly_budget_usd: budgets.weekly,
            monthly_budget_usd: budgets.monthly,
            lifetime_budget_usd: budgets.lifetime,
            daily_token_limit: tokens.daily,
            weekly_token_limit: tokens.weekly,
            monthly_token_limit: tokens.monthly,
            lifetime_token_limit: tokens.lifetime,
            budget_mode: mode.to_string(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            expires_at: None,
        }
    }

    fn budgets(
        daily: Option<f64>,
        weekly: Option<f64>,
        monthly: Option<f64>,
        lifetime: Option<f64>,
    ) -> BudgetWindows {
        BudgetWindows::from_parts(daily, weekly, monthly, lifetime)
    }

    fn tokens(
        daily: Option<i64>,
        weekly: Option<i64>,
        monthly: Option<i64>,
        lifetime: Option<i64>,
    ) -> TokenWindows {
        TokenWindows::from_parts(daily, weekly, monthly, lifetime)
    }

    #[test]
    fn key_fields_win_over_the_plan() {
        let key = key(
            Some(&["key-only"]),
            Some(10),
            budgets(Some(5.0), None, Some(50.0), Some(500.0)),
            "block",
        );
        let plan = plan(
            &["plan-only"],
            Some(99),
            budgets(None, Some(99.0), Some(99.0), Some(999.0)),
            "warn",
        );

        let policy = KeyPolicy::resolve(&key, Some(&plan));

        assert_eq!(policy.allowed_models, vec!["key-only".to_string()]);
        assert_eq!(policy.rate_limit_per_minute, Some(10));
        assert_eq!(policy.budgets.daily, Some(5.0));
        assert_eq!(policy.budgets.monthly, Some(50.0));
        assert_eq!(policy.budgets.lifetime, Some(500.0));
        // The plan still fills the windows the key leaves empty.
        assert_eq!(policy.budgets.weekly, Some(99.0));
        assert_eq!(policy.budget_mode, BudgetMode::Block);
    }

    #[test]
    fn plan_fills_everything_the_key_leaves_empty() {
        let key = key(None, None, budgets(None, None, None, None), "off");
        let plan = plan(
            &["openai/*"],
            Some(30),
            budgets(Some(2.0), Some(10.0), Some(20.0), Some(200.0)),
            "block",
        );

        let policy = KeyPolicy::resolve(&key, Some(&plan));

        assert_eq!(policy.allowed_models, vec!["openai/*".to_string()]);
        assert_eq!(policy.rate_limit_per_minute, Some(30));
        assert_eq!(policy.budgets.daily, Some(2.0));
        assert_eq!(policy.budgets.weekly, Some(10.0));
        assert_eq!(policy.budgets.monthly, Some(20.0));
        assert_eq!(policy.budgets.lifetime, Some(200.0));
        assert_eq!(policy.budget_mode, BudgetMode::Block);
    }

    #[test]
    fn key_budget_without_a_mode_disables_the_plan_cap() {
        let key = key(None, None, budgets(None, None, Some(7.0), None), "off");
        let plan = plan(&[], None, budgets(None, None, Some(20.0), None), "block");

        let policy = KeyPolicy::resolve(&key, Some(&plan));

        assert_eq!(policy.budgets.monthly, Some(7.0));
        assert_eq!(policy.budget_mode, BudgetMode::Off);
    }

    #[test]
    fn token_limits_merge_per_window_and_travel_with_the_key_mode() {
        let key = key_with_tokens(
            None,
            None,
            budgets(None, None, None, None),
            tokens(None, None, Some(1_000_000), None),
            "warn",
        );
        let plan = plan_with_tokens(
            &[],
            None,
            budgets(None, None, Some(20.0), None),
            tokens(Some(5_000_000), Some(50_000_000), None, Some(500_000_000)),
            "block",
        );

        let policy = KeyPolicy::resolve(&key, Some(&plan));

        assert_eq!(policy.token_limits.daily, Some(5_000_000));
        assert_eq!(policy.token_limits.weekly, Some(50_000_000));
        assert_eq!(policy.token_limits.monthly, Some(1_000_000));
        assert_eq!(policy.token_limits.lifetime, Some(500_000_000));
        assert_eq!(policy.budgets.monthly, Some(20.0));
        // The key sets a token limit, so its own mode decides.
        assert_eq!(policy.budget_mode, BudgetMode::Warn);
    }

    #[test]
    fn plan_can_supply_only_token_limits() {
        let key = key(None, None, budgets(None, None, None, None), "off");
        let plan = plan_with_tokens(
            &[],
            None,
            budgets(None, None, None, None),
            tokens(Some(1_000), None, None, None),
            "block",
        );

        let policy = KeyPolicy::resolve(&key, Some(&plan));

        assert_eq!(policy.token_limits.daily, Some(1_000));
        assert_eq!(policy.budget_mode, BudgetMode::Block);
    }

    #[test]
    fn without_any_rules_every_model_is_allowed() {
        let policy = KeyPolicy::resolve(
            &key(None, None, budgets(None, None, None, None), "off"),
            None,
        );

        assert!(policy.unrestricted());
        assert!(policy.ensure_model("anything/at-all").is_ok());
        assert!(policy.budgets.is_empty());
    }

    #[test]
    fn exact_patterns_match_case_insensitively() {
        let policy = KeyPolicy::resolve(
            &key(
                Some(&["GPT-4o"]),
                None,
                budgets(None, None, None, None),
                "off",
            ),
            None,
        );

        assert!(policy.allows("gpt-4o"));
        assert!(policy.allows("GPT-4O"));
        assert!(!policy.allows("gpt-4o-mini"));
        assert!(policy.ensure_model("gpt-4o-mini").is_err());
    }

    #[test]
    fn wildcards_match_the_prefix_and_its_children_only() {
        let policy = KeyPolicy::resolve(
            &key(
                Some(&["openai/*", "*"]),
                None,
                budgets(None, None, None, None),
                "off",
            ),
            None,
        );

        assert!(policy.allows("openai/gpt-4o"));
        assert!(policy.allows("anything"));
        assert!(policy.allows(""));

        let prefixed = KeyPolicy::resolve(
            &key(
                Some(&["openai/*"]),
                None,
                budgets(None, None, None, None),
                "off",
            ),
            None,
        );
        assert!(prefixed.allows("openai/gpt-4o"));
        assert!(prefixed.allows("OPENAI/GPT-4O"));
        assert!(!prefixed.allows("openai"));
        assert!(!prefixed.allows("anthropic/claude"));
    }

    #[test]
    fn forbidden_errors_are_403() {
        let policy = KeyPolicy::resolve(
            &key(
                Some(&["openai/*"]),
                None,
                budgets(None, None, None, None),
                "off",
            ),
            None,
        );

        let error = policy.ensure_model("anthropic/claude").expect_err("denied");
        assert_eq!(error.status(), axum::http::StatusCode::FORBIDDEN);
    }
}
