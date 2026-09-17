//! In-process counters exposed in Prometheus text format.
//!
//! Counters are deliberately coarse: no per-model or per-key labels, so
//! cardinality cannot explode. Scrape `GET /api/metrics` (admin-token guarded
//! when configured).

use std::fmt::Write;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::token_saver::SavingsTotals;
use crate::upstream::Attempt;

/// Cumulative counters for routing, usage, and rejection paths.
#[derive(Default)]
pub struct Metrics {
    requests_total: AtomicU64,
    request_latency_ms_total: AtomicU64,
    attempts_total: AtomicU64,
    failed_attempts_total: AtomicU64,
    failover_requests_total: AtomicU64,
    tokens_prompt_total: AtomicU64,
    tokens_completion_total: AtomicU64,
    tokens_cached_total: AtomicU64,
    cost_micros_total: AtomicU64,
    rate_limited_total: AtomicU64,
    budget_blocked_total: AtomicU64,
    token_saver_rtk_total: AtomicU64,
    token_saver_headroom_total: AtomicU64,
    token_saver_terse_total: AtomicU64,
    token_saver_caveman_total: AtomicU64,
    token_saver_ponytail_total: AtomicU64,
}

impl Metrics {
    /// Records one request and its time-to-first-byte.
    pub fn record_request(&self, latency_ms: u64) {
        self.requests_total.fetch_add(1, Ordering::Relaxed);
        self.request_latency_ms_total
            .fetch_add(latency_ms, Ordering::Relaxed);
    }

    /// Records every tier attempt made for one request.
    pub fn record_attempts(&self, attempts: &[Attempt]) {
        self.attempts_total
            .fetch_add(attempts.len() as u64, Ordering::Relaxed);
        let failed = attempts
            .iter()
            .filter(|attempt| !attempt.outcome.is_success())
            .count() as u64;
        self.failed_attempts_total
            .fetch_add(failed, Ordering::Relaxed);
        if attempts.len() > 1 {
            self.failover_requests_total.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Records token and cost usage for a completed request.
    pub fn record_usage(&self, prompt: u64, completion: u64, cached: u64, cost_usd: f64) {
        self.tokens_prompt_total
            .fetch_add(prompt, Ordering::Relaxed);
        self.tokens_completion_total
            .fetch_add(completion, Ordering::Relaxed);
        self.tokens_cached_total
            .fetch_add(cached, Ordering::Relaxed);
        let micros = (cost_usd.max(0.0) * 1_000_000.0).round() as u64;
        self.cost_micros_total.fetch_add(micros, Ordering::Relaxed);
    }

    /// Records a request rejected by a rate or concurrency limit.
    pub fn record_rate_limited(&self) {
        self.rate_limited_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Records a request rejected by a budget cap.
    pub fn record_budget_blocked(&self) {
        self.budget_blocked_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Records final estimated output savings after completion usage is known.
    pub fn record_token_totals(&self, totals: SavingsTotals) {
        self.token_saver_rtk_total
            .fetch_add(totals.saved_rtk_tokens, Ordering::Relaxed);
        self.token_saver_headroom_total
            .fetch_add(totals.saved_headroom_tokens, Ordering::Relaxed);
        self.token_saver_terse_total
            .fetch_add(totals.saved_terse_tokens, Ordering::Relaxed);
        self.token_saver_caveman_total
            .fetch_add(totals.saved_caveman_tokens, Ordering::Relaxed);
        self.token_saver_ponytail_total
            .fetch_add(totals.saved_ponytail_tokens, Ordering::Relaxed);
    }

    /// Renders every counter in Prometheus text exposition format (0.0.4).
    pub fn render(&self) -> String {
        let mut out = String::new();

        let mut counter = |name: &str, help: &str, value: u64| {
            let _ = writeln!(out, "# HELP {name} {help}");
            let _ = writeln!(out, "# TYPE {name} counter");
            let _ = writeln!(out, "{name} {value}");
        };

        counter(
            "alnair_router_requests_total",
            "Requests handled by the router",
            self.requests_total.load(Ordering::Relaxed),
        );
        counter(
            "alnair_router_request_latency_ms_total",
            "Sum of request time-to-first-byte in milliseconds",
            self.request_latency_ms_total.load(Ordering::Relaxed),
        );
        counter(
            "alnair_router_attempts_total",
            "Upstream attempts, one per tier tried",
            self.attempts_total.load(Ordering::Relaxed),
        );
        counter(
            "alnair_router_failed_attempts_total",
            "Upstream attempts that failed",
            self.failed_attempts_total.load(Ordering::Relaxed),
        );
        counter(
            "alnair_router_failover_requests_total",
            "Requests that needed more than one tier",
            self.failover_requests_total.load(Ordering::Relaxed),
        );
        counter(
            "alnair_router_prompt_tokens_total",
            "Prompt tokens recorded",
            self.tokens_prompt_total.load(Ordering::Relaxed),
        );
        counter(
            "alnair_router_completion_tokens_total",
            "Completion tokens recorded",
            self.tokens_completion_total.load(Ordering::Relaxed),
        );
        counter(
            "alnair_router_cached_tokens_total",
            "Cached prompt tokens recorded",
            self.tokens_cached_total.load(Ordering::Relaxed),
        );
        counter(
            "alnair_router_cost_micros_total",
            "Recorded upstream cost in micro-USD",
            self.cost_micros_total.load(Ordering::Relaxed),
        );
        counter(
            "alnair_router_rate_limited_total",
            "Requests rejected by a rate or concurrency limit",
            self.rate_limited_total.load(Ordering::Relaxed),
        );
        counter(
            "alnair_router_budget_blocked_total",
            "Requests rejected by a budget cap",
            self.budget_blocked_total.load(Ordering::Relaxed),
        );
        counter(
            "alnair_router_token_saver_rtk_tokens_total",
            "Measured input tokens saved by RTK/Slimmer",
            self.token_saver_rtk_total.load(Ordering::Relaxed),
        );
        counter(
            "alnair_router_token_saver_headroom_tokens_total",
            "Measured input tokens saved by Headroom",
            self.token_saver_headroom_total.load(Ordering::Relaxed),
        );
        counter(
            "alnair_router_token_saver_terse_tokens_total",
            "Estimated output tokens saved by Terse",
            self.token_saver_terse_total.load(Ordering::Relaxed),
        );
        counter(
            "alnair_router_token_saver_caveman_tokens_total",
            "Estimated output tokens saved by Caveman",
            self.token_saver_caveman_total.load(Ordering::Relaxed),
        );
        counter(
            "alnair_router_token_saver_ponytail_tokens_total",
            "Estimated output tokens saved by Ponytail",
            self.token_saver_ponytail_total.load(Ordering::Relaxed),
        );

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::upstream::AttemptOutcome;

    fn attempt(index: usize, success: bool) -> Attempt {
        Attempt {
            index,
            source: format!("tier{index}"),
            provider_type: "openai-compatible".to_string(),
            connection_name: "openai-main".to_string(),
            model: "gpt-4o".to_string(),
            outcome: if success {
                AttemptOutcome::Succeeded
            } else {
                AttemptOutcome::Failed("boom".to_string())
            },
            latency_ms: 10,
        }
    }

    #[test]
    fn render_includes_all_counters() {
        let metrics = Metrics::default();
        metrics.record_request(120);
        metrics.record_attempts(&[attempt(1, false), attempt(2, true)]);
        metrics.record_usage(100, 20, 5, 0.0015);
        metrics.record_rate_limited();
        metrics.record_budget_blocked();

        let rendered = metrics.render();

        assert!(rendered.contains("alnair_router_requests_total 1"));
        assert!(rendered.contains("alnair_router_request_latency_ms_total 120"));
        assert!(rendered.contains("alnair_router_attempts_total 2"));
        assert!(rendered.contains("alnair_router_failed_attempts_total 1"));
        assert!(rendered.contains("alnair_router_failover_requests_total 1"));
        assert!(rendered.contains("alnair_router_prompt_tokens_total 100"));
        assert!(rendered.contains("alnair_router_completion_tokens_total 20"));
        assert!(rendered.contains("alnair_router_cached_tokens_total 5"));
        assert!(rendered.contains("alnair_router_cost_micros_total 1500"));
        assert!(rendered.contains("alnair_router_rate_limited_total 1"));
        assert!(rendered.contains("alnair_router_budget_blocked_total 1"));
        assert!(rendered.contains("# TYPE alnair_router_requests_total counter"));
    }

    #[test]
    fn single_tier_requests_are_not_failovers() {
        let metrics = Metrics::default();
        metrics.record_attempts(&[attempt(1, true)]);
        assert!(
            metrics
                .render()
                .contains("alnair_router_failover_requests_total 0")
        );
    }
}
