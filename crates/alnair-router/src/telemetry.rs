//! In-memory activity tracking for the live dashboard.
//!
//! This is a ring buffer of recent events plus per-connection counters and an
//! in-flight gauge. It is deliberately ephemeral: restarting the router clears
//! everything, and nothing here is persisted (usage rows are the durable log).

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::Instant;

use chrono::{DateTime, Utc};
use serde::Serialize;

const MAX_EVENTS: usize = 500;

/// One line in the console feed.
#[derive(Debug, Clone, Serialize)]
pub struct ActivityEvent {
    pub seq: u64,
    pub at: DateTime<Utc>,
    /// `info`, `warn`, or `error`.
    pub level: &'static str,
    /// Dotted kind, e.g. `attempt.start`, `request.completed`, `auth.denied`.
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
}

/// An upstream attempt that has not produced its first chunk yet.
#[derive(Debug, Clone, Serialize)]
pub struct ActiveAttempt {
    pub connection_id: String,
    pub connection: String,
    pub model: String,
    pub source: String,
    pub tier: usize,
    pub elapsed_ms: u64,
}

/// Rolling counters for one connection.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ConnectionActivity {
    pub id: String,
    pub name: String,
    pub in_flight: u64,
    pub requests: u64,
    pub failures: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_latency_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_activity: Option<DateTime<Utc>>,
}

impl ConnectionActivity {
    /// Average first-chunk latency in milliseconds, or `None` before any request.
    pub fn average_latency_ms(&self) -> Option<f64> {
        (self.requests > 0).then(|| self.total_latency_ms as f64 / self.requests as f64)
    }
}

/// What `GET /api/activity` returns.
#[derive(Debug, Clone, Serialize)]
pub struct ActivitySnapshot {
    pub uptime_ms: u64,
    pub active: Vec<ActiveAttempt>,
    pub connections: Vec<ConnectionActivity>,
    pub events: Vec<ActivityEvent>,
}

struct ActiveEntry {
    connection_id: String,
    connection: String,
    model: String,
    source: String,
    tier: usize,
    started: Instant,
}

#[derive(Default)]
struct Inner {
    events: VecDeque<ActivityEvent>,
    connections: HashMap<String, ConnectionActivity>,
    active: HashMap<u64, ActiveEntry>,
    seq: u64,
}

/// Ephemeral activity store shared through [`crate::state::AppState`].
pub struct ActivityTracker {
    started: Instant,
    inner: Mutex<Inner>,
}

impl Default for ActivityTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl ActivityTracker {
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
            inner: Mutex::new(Inner::default()),
        }
    }

    /// Starts an upstream attempt and returns a token to finish it with.
    pub fn begin_attempt(
        &self,
        connection_id: &str,
        connection: &str,
        model: &str,
        source: &str,
        tier: usize,
    ) -> u64 {
        let mut inner = self.inner.lock().expect("activity tracker poisoned");
        inner.seq += 1;
        let token = inner.seq;

        let entry = inner
            .connections
            .entry(connection_id.to_string())
            .or_insert_with(|| ConnectionActivity {
                id: connection_id.to_string(),
                name: connection.to_string(),
                ..Default::default()
            });
        entry.in_flight += 1;
        entry.name = connection.to_string();

        inner.active.insert(
            token,
            ActiveEntry {
                connection_id: connection_id.to_string(),
                connection: connection.to_string(),
                model: model.to_string(),
                source: source.to_string(),
                tier,
                started: Instant::now(),
            },
        );

        push_event(
            &mut inner,
            "info",
            "attempt.start",
            Some(connection.to_string()),
            Some(model.to_string()),
            format!("tier {tier} → {connection} / {model} ({source})"),
            None,
            None,
        );

        token
    }

    /// Finishes an attempt started with [`Self::begin_attempt`].
    pub fn finish_attempt(&self, token: u64, ok: bool, detail: &str) {
        let mut inner = self.inner.lock().expect("activity tracker poisoned");
        let Some(entry) = inner.active.remove(&token) else {
            return;
        };
        let latency_ms = entry.started.elapsed().as_millis() as u64;

        if let Some(stats) = inner.connections.get_mut(&entry.connection_id) {
            stats.in_flight = stats.in_flight.saturating_sub(1);
            stats.requests += 1;
            stats.total_latency_ms += latency_ms;
            stats.last_activity = Some(Utc::now());
            if !ok {
                stats.failures += 1;
            }
        }

        let (level, kind, message) = if ok {
            (
                "info",
                "attempt.completed",
                format!(
                    "tier {} answered in {} ms ({detail})",
                    entry.tier, latency_ms
                ),
            )
        } else {
            (
                "warn",
                "attempt.failed",
                format!(
                    "tier {} failed after {} ms: {detail}",
                    entry.tier, latency_ms
                ),
            )
        };

        push_event(
            &mut inner,
            level,
            kind,
            Some(entry.connection),
            Some(entry.model),
            message,
            Some(latency_ms),
            None,
        );
    }

    /// Records a general event (HTTP calls, rejections, handler completions).
    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &self,
        level: &'static str,
        kind: &'static str,
        connection: Option<&str>,
        model: Option<&str>,
        message: String,
        latency_ms: Option<u64>,
        status: Option<u16>,
    ) {
        let mut inner = self.inner.lock().expect("activity tracker poisoned");
        push_event(
            &mut inner,
            level,
            kind,
            connection.map(str::to_string),
            model.map(str::to_string),
            message,
            latency_ms,
            status,
        );
    }

    /// Adds token counters to a connection's rolling stats.
    pub fn record_usage(&self, connection_id: &str, prompt: u64, completion: u64) {
        let mut inner = self.inner.lock().expect("activity tracker poisoned");
        if let Some(stats) = inner.connections.get_mut(connection_id) {
            stats.prompt_tokens += prompt;
            stats.completion_tokens += completion;
        }
    }

    /// Point-in-time snapshot for `/api/activity`.
    pub fn snapshot(&self, events: usize) -> ActivitySnapshot {
        let inner = self.inner.lock().expect("activity tracker poisoned");
        let now = Utc::now();

        let mut active: Vec<ActiveAttempt> = inner
            .active
            .values()
            .map(|entry| ActiveAttempt {
                connection_id: entry.connection_id.clone(),
                connection: entry.connection.clone(),
                model: entry.model.clone(),
                source: entry.source.clone(),
                tier: entry.tier,
                elapsed_ms: entry.started.elapsed().as_millis() as u64,
            })
            .collect();
        active.sort_by(|a, b| a.connection.cmp(&b.connection));

        let mut connections: Vec<ConnectionActivity> =
            inner.connections.values().cloned().collect();
        connections.sort_by(|a, b| a.name.cmp(&b.name));

        let events: Vec<ActivityEvent> = inner
            .events
            .iter()
            .rev()
            .take(events)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        let _ = now;
        ActivitySnapshot {
            uptime_ms: self.started.elapsed().as_millis() as u64,
            active,
            connections,
            events,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn push_event(
    inner: &mut Inner,
    level: &'static str,
    kind: &'static str,
    connection: Option<String>,
    model: Option<String>,
    message: String,
    latency_ms: Option<u64>,
    status: Option<u16>,
) {
    inner.seq += 1;
    let event = ActivityEvent {
        seq: inner.seq,
        at: Utc::now(),
        level,
        kind,
        connection,
        model,
        message,
        latency_ms,
        status,
    };
    if inner.events.len() >= MAX_EVENTS {
        inner.events.pop_front();
    }
    inner.events.push_back(event);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attempts_update_counters_and_events() {
        let tracker = ActivityTracker::new();
        let token = tracker.begin_attempt("c1", "9router", "deepseek", "alias:9r", 1);
        tracker.finish_attempt(token, true, "first chunk ready");

        let failed = tracker.begin_attempt("c1", "9router", "deepseek", "alias:9r", 2);
        tracker.finish_attempt(failed, false, "boom");

        let snapshot = tracker.snapshot(50);
        assert!(snapshot.active.is_empty());

        let stats = snapshot
            .connections
            .iter()
            .find(|stats| stats.id == "c1")
            .expect("connection stats");
        assert_eq!(stats.requests, 2);
        assert_eq!(stats.failures, 1);
        assert_eq!(stats.in_flight, 0);
        assert!(stats.average_latency_ms().is_some());

        let kinds: Vec<&str> = snapshot.events.iter().map(|event| event.kind).collect();
        assert_eq!(
            kinds,
            vec![
                "attempt.start",
                "attempt.completed",
                "attempt.start",
                "attempt.failed"
            ]
        );
    }

    #[test]
    fn in_flight_is_visible_until_finished() {
        let tracker = ActivityTracker::new();
        let token = tracker.begin_attempt("c1", "9router", "m", "default:9router", 1);

        let snapshot = tracker.snapshot(10);
        assert_eq!(snapshot.active.len(), 1);
        assert_eq!(snapshot.connections[0].in_flight, 1);

        tracker.finish_attempt(token, true, "done");
        assert!(tracker.snapshot(10).active.is_empty());
    }

    #[test]
    fn events_are_capped() {
        let tracker = ActivityTracker::new();
        for index in 0..(MAX_EVENTS + 20) {
            tracker.record(
                "info",
                "test",
                None,
                None,
                format!("event {index}"),
                None,
                None,
            );
        }

        let snapshot = tracker.snapshot(MAX_EVENTS + 100);
        assert_eq!(snapshot.events.len(), MAX_EVENTS);
        assert!(
            snapshot
                .events
                .last()
                .expect("last")
                .message
                .contains("519"),
            "newest event should be retained"
        );
        assert!(
            !snapshot
                .events
                .iter()
                .any(|event| event.message.contains("event 0")),
            "oldest events should be evicted"
        );
    }

    #[test]
    fn usage_counters_attach_to_known_connections() {
        let tracker = ActivityTracker::new();
        let token = tracker.begin_attempt("c1", "9router", "m", "alias:9r", 1);
        tracker.finish_attempt(token, true, "ok");
        tracker.record_usage("c1", 100, 20);
        tracker.record_usage("missing", 1, 1);

        let snapshot = tracker.snapshot(10);
        assert_eq!(snapshot.connections[0].prompt_tokens, 100);
        assert_eq!(snapshot.connections[0].completion_tokens, 20);
    }
}
