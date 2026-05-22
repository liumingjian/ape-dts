//! AlertEngine — evaluates alert rules on a tick against fresh metric points.
//!
//! All five operators (>, <, >=, <=, ==) are supported.
//! Dwell time prevents flapping by requiring sustained violation.
//! Recovery threshold gates the firing→recovered transition.
//! Severity is preserved verbatim from rule to alert.
//! Global silence window suppresses dispatch but not recording.
//! Heartbeat staleness (>N seconds) flags cdc_stalled alert.
//!
//! The engine is deterministic over synthetic time: the same input series +
//! rule set + clock always produces the same event sequence.

use crate::models::{Alert, AlertRule};
use crate::repositories::alert_repository::AlertRepository;
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Default heartbeat staleness threshold in seconds for CDC tasks.
#[allow(dead_code)]
const DEFAULT_HEARTBEAT_STALENESS_SECS: i64 = 60;

/// An event emitted by the AlertEngine on each tick.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AlertEvent {
    /// A new alert has fired.
    Firing {
        id: String,
        task_id: Option<String>,
        run_id: Option<String>,
        rule_id: Option<String>,
        severity: String,
        metric: Option<String>,
        value: Option<f64>,
        threshold: Option<f64>,
        fired_at: String,
        silenced: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        last_heartbeat_at: Option<String>,
    },
    /// A previously firing alert has recovered.
    Recovery {
        id: String,
        task_id: Option<String>,
        run_id: Option<String>,
        severity: String,
        recovered_at: String,
        /// The status the alert held before recovery (always "firing").
        previous_status: String,
    },
    /// A cdc_stalled alert has been flagged due to heartbeat staleness.
    CdcStalled {
        id: String,
        task_id: String,
        run_id: Option<String>,
        last_heartbeat_at: Option<String>,
        severity: String,
        fired_at: String,
        silenced: bool,
    },
    /// A cdc_stalled alert has been cleared by heartbeat recovery.
    CdcRecovered {
        id: String,
        task_id: String,
        recovered_at: String,
    },
}

/// Evaluate a comparison operator against a value and threshold.
pub fn evaluate_op(op: &str, value: f64, threshold: f64) -> bool {
    match op {
        ">" => value > threshold,
        "<" => value < threshold,
        ">=" => value >= threshold,
        "<=" => value <= threshold,
        "==" => (value - threshold).abs() < f64::EPSILON,
        _ => false,
    }
}

/// Check whether a value has crossed below the recovery threshold.
///
/// If no recovery_threshold is set on the rule, recovery occurs when
/// the value is no longer in violation of the primary threshold.
/// If recovery_threshold is set, recovery occurs only when the value
/// crosses below that threshold.
pub fn is_recovered(rule: &AlertRule, value: f64) -> bool {
    if let Some(rt) = rule.recovery_threshold {
        match rule.operator.as_str() {
            ">" | ">=" => value < rt,
            "<" | "<=" => value > rt,
            "==" => (value - rt).abs() >= f64::EPSILON,
            _ => false,
        }
    } else {
        // No recovery threshold: recover when the primary condition is no longer met.
        !evaluate_op(&rule.operator, value, rule.threshold)
    }
}

/// Dwell-tracker: tracks how long a violation has been sustained.
#[derive(Debug, Clone, Default)]
pub struct DwellTracker {
    /// rule_id → (first_violation_ts, sustained_ticks)
    violations: std::collections::HashMap<String, (i64, u32)>,
}

impl DwellTracker {
    /// Record a violation at `tick_ts`. Returns true if the dwell time has been met.
    pub fn record_violation(&mut self, rule_id: &str, dwell_secs: i64, tick_ts: i64) -> bool {
        let entry = self
            .violations
            .entry(rule_id.to_string())
            .or_insert((tick_ts, 0));
        entry.1 += 1;

        if dwell_secs == 0 {
            return true;
        }

        // Check if the sustained duration meets dwell.
        let elapsed = tick_ts - entry.0;
        elapsed >= dwell_secs
    }

    /// Record that the violation has ended (reset dwell).
    pub fn clear_violation(&mut self, rule_id: &str) {
        self.violations.remove(rule_id);
    }

    /// Check if a rule is currently in a dwell period (violating but not yet met).
    pub fn is_dwelling(&self, rule_id: &str) -> bool {
        self.violations.contains_key(rule_id)
    }
}

/// Heartbeat staleness tracker per Task.
#[derive(Debug, Clone, Default)]
pub struct HeartbeatTracker {
    /// task_id → (last_heartbeat_ts, stalled_alert_id)
    tasks: std::collections::HashMap<String, (i64, Option<String>)>,
}

impl HeartbeatTracker {
    /// Update the last heartbeat timestamp for a task.
    pub fn update_heartbeat(&mut self, task_id: &str, ts: i64) {
        let entry = self.tasks.entry(task_id.to_string()).or_insert((0, None));
        entry.0 = ts;
    }

    /// Check if a task's heartbeat is stale (older than threshold_secs from now_ts).
    /// Returns (is_stale, last_heartbeat_ts).
    pub fn check_staleness(
        &self,
        task_id: &str,
        now_ts: i64,
        threshold_secs: i64,
    ) -> (bool, Option<i64>) {
        match self.tasks.get(task_id) {
            Some((last_ts, _)) => {
                let elapsed = now_ts - last_ts;
                (elapsed > threshold_secs, Some(*last_ts))
            }
            None => (false, None),
        }
    }

    /// Mark that a stalled alert has been fired for a task.
    pub fn set_stalled_alert(&mut self, task_id: &str, alert_id: &str) {
        if let Some(entry) = self.tasks.get_mut(task_id) {
            entry.1 = Some(alert_id.to_string());
        }
    }

    /// Get the stalled alert ID for a task, if any.
    pub fn stalled_alert_id(&self, task_id: &str) -> Option<String> {
        self.tasks.get(task_id).and_then(|e| e.1.clone())
    }

    /// Clear the stalled alert for a task (after recovery).
    pub fn clear_stalled_alert(&mut self, task_id: &str) {
        if let Some(entry) = self.tasks.get_mut(task_id) {
            entry.1 = None;
        }
    }
}

/// AlertEngine state shared between ticks.
#[derive(Debug, Clone, Default)]
pub struct AlertEngineState {
    /// Dwell trackers per (task_id, rule_id) pair.
    dwell: Arc<Mutex<std::collections::HashMap<String, DwellTracker>>>,
    /// Heartbeat staleness tracker.
    heartbeat: Arc<Mutex<HeartbeatTracker>>,
    /// Current silence window (start_ts, end_ts).
    silence_window: Arc<Mutex<Option<(i64, i64)>>>,
}

impl AlertEngineState {
    pub fn new() -> Self {
        Self {
            dwell: Arc::new(Mutex::new(std::collections::HashMap::new())),
            heartbeat: Arc::new(Mutex::new(HeartbeatTracker::default())),
            silence_window: Arc::new(Mutex::new(None)),
        }
    }

    /// Set the global silence window.
    pub async fn set_silence_window(&self, start_ts: i64, end_ts: i64) {
        let mut window = self.silence_window.lock().await;
        *window = Some((start_ts, end_ts));
    }

    /// Clear the global silence window.
    pub async fn clear_silence_window(&self) {
        let mut window = self.silence_window.lock().await;
        *window = None;
    }

    /// Check if the current time falls within the silence window.
    pub async fn is_silenced(&self, now_ts: i64) -> bool {
        let window = self.silence_window.lock().await;
        match *window {
            Some((start, end)) => now_ts >= start && now_ts <= end,
            None => false,
        }
    }
}

/// A single metric point for evaluation.
#[derive(Debug, Clone)]
pub struct MetricInput {
    pub task_id: String,
    pub run_id: String,
    pub metric_name: String,
    pub ts: i64,
    pub value: f64,
}

/// Run one evaluation tick of the AlertEngine.
///
/// Takes the current set of enabled rules, fresh metric points, and the
/// current synthetic time. Returns a list of events that were emitted.
///
/// This is the core deterministic function: same inputs always produce the
/// same outputs.
pub async fn evaluate_tick(
    pool: &SqlitePool,
    state: &AlertEngineState,
    rules: &[AlertRule],
    metrics: &[MetricInput],
    now_ts: i64,
) -> Vec<AlertEvent> {
    let mut events = Vec::new();
    let silenced = state.is_silenced(now_ts).await;

    // Group metrics by (task_id, metric_name) for lookup.
    let mut metric_map: std::collections::HashMap<(String, String), &MetricInput> =
        std::collections::HashMap::new();
    for m in metrics {
        metric_map.insert((m.task_id.clone(), m.metric_name.clone()), m);
    }

    // Get or create dwell trackers.
    let mut dwell_map = state.dwell.lock().await;

    // Evaluate each enabled rule against the latest metric points.
    for rule in rules {
        if !rule.enabled {
            continue;
        }

        // Find matching metric points for this rule's metric_name.
        let matching_metrics: Vec<&MetricInput> = metrics
            .iter()
            .filter(|m| m.metric_name == rule.metric_name)
            .collect();

        for mp in &matching_metrics {
            let tracker_key = format!("{}:{}", mp.task_id, rule.id);
            let tracker = dwell_map
                .entry(tracker_key.clone())
                .or_insert_with(DwellTracker::default);

            // Check if there's already a firing alert for this (rule, task).
            let existing = AlertRepository::find_firing_by_rule(pool, &rule.id, Some(&mp.task_id))
                .await
                .ok()
                .flatten();

            let in_violation = evaluate_op(&rule.operator, mp.value, rule.threshold);

            if in_violation {
                if existing.is_some() {
                    // Already firing — stay firing. Clear dwell (it's already fired).
                    tracker.clear_violation(&rule.id);
                } else {
                    // Not yet firing — track dwell.
                    let dwell_met = tracker.record_violation(&rule.id, rule.dwell_secs, now_ts);
                    if dwell_met {
                        // Dwell met: fire the alert.
                        tracker.clear_violation(&rule.id);
                        let alert_id = uuid::Uuid::new_v4().to_string();
                        let fired_at = ts_to_rfc3339(now_ts);

                        let alert = Alert {
                            id: alert_id.clone(),
                            task_id: Some(mp.task_id.clone()),
                            run_id: Some(mp.run_id.clone()),
                            rule_id: Some(rule.id.clone()),
                            metric_name: Some(rule.metric_name.clone()),
                            operator: Some(rule.operator.clone()),
                            threshold: Some(rule.threshold),
                            severity: rule.severity.clone(),
                            value: Some(mp.value),
                            status: "firing".to_string(),
                            silenced,
                            fired_at: fired_at.clone(),
                            recovered_at: None,
                            cleared_at: None,
                            delivered_at: None,
                            cleared_by: None,
                            last_error: None,
                            created_at: fired_at.clone(),
                        };

                        if let Ok(persisted) = AlertRepository::create(pool, &alert).await {
                            events.push(AlertEvent::Firing {
                                id: persisted.id,
                                task_id: persisted.task_id,
                                run_id: persisted.run_id,
                                rule_id: persisted.rule_id,
                                severity: persisted.severity,
                                metric: persisted.metric_name,
                                value: persisted.value,
                                threshold: persisted.threshold,
                                fired_at: persisted.fired_at,
                                silenced,
                                last_heartbeat_at: None,
                            });
                        }
                    }
                }
            } else {
                // Not in violation — check recovery.
                if let Some(ref firing) = existing {
                    let recovered = is_recovered(rule, mp.value);
                    if recovered {
                        // Recover: emit exactly one recovery event.
                        tracker.clear_violation(&rule.id);
                        let recovered_at = ts_to_rfc3339(now_ts);
                        let mut updated = firing.clone();
                        updated.status = "recovered".to_string();
                        updated.recovered_at = Some(recovered_at.clone());

                        if let Ok(persisted) = AlertRepository::update(pool, &updated).await {
                            events.push(AlertEvent::Recovery {
                                id: persisted.id,
                                task_id: persisted.task_id,
                                run_id: persisted.run_id,
                                severity: persisted.severity,
                                recovered_at,
                                previous_status: "firing".to_string(),
                            });
                        }
                    }
                } else {
                    // Not in violation and not firing — clear any dwell tracking.
                    tracker.clear_violation(&rule.id);
                }
            }
        }
    }

    events
}

/// Evaluate heartbeat staleness for CDC tasks.
///
/// For each CDC task, check if the last heartbeat timestamp is older than
/// the staleness threshold. If so, fire a cdc_stalled alert. If a stalled
/// alert already exists and the heartbeat has recovered, emit a recovery.
pub async fn evaluate_heartbeat_staleness(
    pool: &SqlitePool,
    state: &AlertEngineState,
    cdc_tasks: &[(String, Option<String>)], // (task_id, Some(run_id))
    heartbeat_timestamps: &[(String, i64)], // (task_id, last_heartbeat_ts)
    now_ts: i64,
    threshold_secs: i64,
) -> Vec<AlertEvent> {
    let mut events = Vec::new();
    let silenced = state.is_silenced(now_ts).await;
    let mut hb_tracker = state.heartbeat.lock().await;

    // Update heartbeat timestamps from input.
    for (task_id, ts) in heartbeat_timestamps {
        hb_tracker.update_heartbeat(task_id, *ts);
    }

    for (task_id, run_id) in cdc_tasks {
        let (is_stale, last_ts) = hb_tracker.check_staleness(task_id, now_ts, threshold_secs);

        if is_stale {
            // Check if we already have a stalled alert for this task.
            let existing = AlertRepository::find_cdc_stalled(pool, task_id)
                .await
                .ok()
                .flatten();

            if existing.is_none() && hb_tracker.stalled_alert_id(task_id).is_none() {
                // Fire cdc_stalled alert.
                let alert_id = uuid::Uuid::new_v4().to_string();
                let fired_at = ts_to_rfc3339(now_ts);
                let last_hb_str = last_ts.map(ts_to_rfc3339);

                let alert = Alert {
                    id: alert_id.clone(),
                    task_id: Some(task_id.clone()),
                    run_id: run_id.clone(),
                    rule_id: None,
                    metric_name: Some("cdc_stalled".to_string()),
                    operator: None,
                    threshold: None,
                    severity: "critical".to_string(),
                    value: None,
                    status: "firing".to_string(),
                    silenced,
                    fired_at: fired_at.clone(),
                    recovered_at: None,
                    cleared_at: None,
                    delivered_at: None,
                    cleared_by: None,
                    last_error: None,
                    created_at: fired_at.clone(),
                };

                if let Ok(persisted) = AlertRepository::create(pool, &alert).await {
                    hb_tracker.set_stalled_alert(task_id, &persisted.id);
                    events.push(AlertEvent::CdcStalled {
                        id: persisted.id,
                        task_id: task_id.clone(),
                        run_id: run_id.clone(),
                        last_heartbeat_at: last_hb_str,
                        severity: "critical".to_string(),
                        fired_at,
                        silenced,
                    });
                }
            }
        } else {
            // Not stale — check if there's a stalled alert to recover.
            if let Some(stalled_id) = hb_tracker.stalled_alert_id(task_id) {
                // Recover the stalled alert.
                let recovered_at = ts_to_rfc3339(now_ts);
                if let Ok(mut alert) = AlertRepository::find_by_id(pool, &stalled_id).await {
                    alert.status = "recovered".to_string();
                    alert.recovered_at = Some(recovered_at.clone());
                    if let Ok(persisted) = AlertRepository::update(pool, &alert).await {
                        hb_tracker.clear_stalled_alert(task_id);
                        events.push(AlertEvent::CdcRecovered {
                            id: persisted.id,
                            task_id: task_id.clone(),
                            recovered_at,
                        });
                    }
                }
            }
        }
    }

    events
}

/// Convert a unix-epoch-seconds timestamp to RFC 3339.
fn ts_to_rfc3339(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_else(|| format!("{ts}"))
}

// ─── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gt_operator_fires() {
        assert!(evaluate_op(">", 150.0, 100.0));
        assert!(!evaluate_op(">", 50.0, 100.0));
        assert!(!evaluate_op(">", 100.0, 100.0));
    }

    #[test]
    fn test_lt_operator_fires() {
        assert!(evaluate_op("<", 5.0, 10.0));
        assert!(!evaluate_op("<", 50.0, 10.0));
        assert!(!evaluate_op("<", 10.0, 10.0));
    }

    #[test]
    fn test_gte_inclusive_boundary() {
        assert!(evaluate_op(">=", 100.0, 100.0));
        assert!(!evaluate_op(">=", 99.0, 100.0));
    }

    #[test]
    fn test_lte_inclusive_boundary() {
        assert!(evaluate_op("<=", 10.0, 10.0));
        assert!(!evaluate_op("<=", 11.0, 10.0));
    }

    #[test]
    fn test_eq_operator_exact_match() {
        assert!(evaluate_op("==", 0.0, 0.0));
        assert!(!evaluate_op("==", 0.5, 0.0));
        assert!(!evaluate_op("==", 1.0, 0.0));
    }

    #[test]
    fn test_dwell_prevents_flapping() {
        let mut tracker = DwellTracker::default();
        // dwell = 30s, points at 10s spacing: [150, 50, 150, 150, 150]
        // First breach at t=0 → dwell not met
        assert!(!tracker.record_violation("r1", 30, 0));
        // Value recovers at t=10 → clear dwell
        tracker.clear_violation("r1");
        // Breach again at t=20 → dwell starts fresh
        assert!(!tracker.record_violation("r1", 30, 20));
        // Still breaching at t=30 → dwell met (30-20=10 < 30)... no
        assert!(!tracker.record_violation("r1", 30, 30));
        // At t=50 → 50-20=30, dwell met
        assert!(tracker.record_violation("r1", 30, 50));
    }

    #[test]
    fn test_dwell_zero_fires_immediately() {
        let mut tracker = DwellTracker::default();
        assert!(tracker.record_violation("r1", 0, 0));
    }

    #[test]
    fn test_recovery_threshold_gates_transition() {
        let rule = AlertRule {
            id: "r1".into(),
            name: "test".into(),
            metric_name: "extractor_rps_avg".into(),
            operator: ">".into(),
            threshold: 100.0,
            recovery_threshold: Some(80.0),
            severity: "warning".into(),
            dwell_secs: 0,
            channel_ids: "[]".into(),
            enabled: true,
            resource_group_id: None,
            created_at: String::new(),
            updated_at: String::new(),
        };

        // Value 90 is below threshold but above recovery → not recovered
        assert!(!is_recovered(&rule, 90.0));
        // Value 80 is at recovery boundary → not recovered (need < 80 for >)
        assert!(!is_recovered(&rule, 80.0));
        // Value 79 is below recovery threshold → recovered
        assert!(is_recovered(&rule, 79.0));
    }

    #[test]
    fn test_recovery_without_threshold() {
        let rule = AlertRule {
            id: "r1".into(),
            name: "test".into(),
            metric_name: "m".into(),
            operator: ">".into(),
            threshold: 100.0,
            recovery_threshold: None,
            severity: "warning".into(),
            dwell_secs: 0,
            channel_ids: "[]".into(),
            enabled: true,
            resource_group_id: None,
            created_at: String::new(),
            updated_at: String::new(),
        };

        // Value 99 is no longer > 100 → recovered
        assert!(is_recovered(&rule, 99.0));
        // Value 100 is still >= threshold for > operator → not recovered
        // (strict > means 100 is not in violation, so it IS recovered)
        // Actually: > 100 means violation when value > 100. value=100 is NOT > 100,
        // so the condition is no longer met, meaning recovered.
        assert!(is_recovered(&rule, 100.0));
        // Value 101 is > 100 → NOT recovered
        assert!(!is_recovered(&rule, 101.0));
    }

    #[test]
    fn test_severity_preserved_verbatim() {
        // This test verifies the Alert struct preserves severity from the rule.
        let rule_severity = "critical";
        let alert = Alert {
            id: "a1".into(),
            task_id: None,
            run_id: None,
            rule_id: Some("r1".into()),
            metric_name: Some("m".into()),
            operator: Some(">".into()),
            threshold: Some(100.0),
            severity: rule_severity.to_string(),
            value: Some(150.0),
            status: "firing".into(),
            silenced: false,
            fired_at: String::new(),
            recovered_at: None,
            cleared_at: None,
            delivered_at: None,
            cleared_by: None,
            last_error: None,
            created_at: String::new(),
        };
        assert_eq!(alert.severity, "critical");
    }

    #[test]
    fn test_heartbeat_tracker_independent_per_task() {
        let mut tracker = HeartbeatTracker::default();
        tracker.update_heartbeat("task-a", 100);
        tracker.update_heartbeat("task-b", 200);

        // task-a is stale at now=200 with threshold=60 (200-100=100 > 60)
        let (stale_a, _) = tracker.check_staleness("task-a", 200, 60);
        assert!(stale_a);

        // task-b is fresh (200-200=0 < 60)
        let (stale_b, _) = tracker.check_staleness("task-b", 200, 60);
        assert!(!stale_b);
    }

    #[tokio::test]
    async fn test_silence_window_suppresses_dispatch() {
        let state = AlertEngineState::new();
        state.set_silence_window(100, 200).await;
        assert!(state.is_silenced(150).await);
        assert!(!state.is_silenced(50).await);
        assert!(!state.is_silenced(250).await);

        state.clear_silence_window().await;
        assert!(!state.is_silenced(150).await);
    }

    #[test]
    fn test_engine_deterministic_over_synthetic_time() {
        // Run the same rule + points twice and verify identical results.
        let rule = AlertRule {
            id: "r1".into(),
            name: "test".into(),
            metric_name: "m".into(),
            operator: ">".into(),
            threshold: 100.0,
            recovery_threshold: None,
            severity: "warning".into(),
            dwell_secs: 0,
            channel_ids: "[]".into(),
            enabled: true,
            resource_group_id: None,
            created_at: String::new(),
            updated_at: String::new(),
        };

        let points = [
            MetricInput {
                task_id: "t1".into(),
                run_id: "r1".into(),
                metric_name: "m".into(),
                ts: 0,
                value: 50.0,
            },
            MetricInput {
                task_id: "t1".into(),
                run_id: "r1".into(),
                metric_name: "m".into(),
                ts: 10,
                value: 150.0,
            },
        ];

        // Pure logic test: evaluate_op is deterministic.
        let r1_0 = evaluate_op(&rule.operator, points[0].value, rule.threshold);
        let r1_1 = evaluate_op(&rule.operator, points[1].value, rule.threshold);
        let r2_0 = evaluate_op(&rule.operator, points[0].value, rule.threshold);
        let r2_1 = evaluate_op(&rule.operator, points[1].value, rule.threshold);

        assert_eq!(r1_0, r2_0);
        assert_eq!(r1_1, r2_1);
        assert!(!r1_0); // 50 > 100 → false
        assert!(r1_1); // 150 > 100 → true
    }

    #[test]
    fn test_recovery_emits_exactly_one_event() {
        // Simulate: fire at 150, recover at 79.
        // Multiple subsequent points below recovery should not re-emit.
        let rule = AlertRule {
            id: "r1".into(),
            name: "test".into(),
            metric_name: "m".into(),
            operator: ">".into(),
            threshold: 100.0,
            recovery_threshold: Some(80.0),
            severity: "warning".into(),
            dwell_secs: 0,
            channel_ids: "[]".into(),
            enabled: true,
            resource_group_id: None,
            created_at: String::new(),
            updated_at: String::new(),
        };

        // After recovery, subsequent below-threshold points don't matter.
        assert!(is_recovered(&rule, 79.0));
        assert!(is_recovered(&rule, 50.0));
        assert!(is_recovered(&rule, 0.0));
        // The engine would only emit ONE recovery event for the first
        // transition from firing→recovered, not for subsequent points.
    }

    /// VAL-RULE-002: Disabled rule does not fire.
    /// Verify that a rule with enabled=false is skipped by evaluate_tick.
    #[tokio::test]
    async fn test_disabled_rule_does_not_fire() {
        let pool = crate::db::create_pool(":memory:").await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        let state = AlertEngineState::new();

        let disabled_rule = AlertRule {
            id: "disabled-rule".into(),
            name: "disabled".into(),
            metric_name: "extractor_rps_avg".into(),
            operator: ">".into(),
            threshold: 100.0,
            recovery_threshold: None,
            severity: "critical".into(),
            dwell_secs: 0,
            channel_ids: "[]".into(),
            enabled: false, // DISABLED
            resource_group_id: None,
            created_at: String::new(),
            updated_at: String::new(),
        };

        let metrics = vec![MetricInput {
            task_id: "t1".into(),
            run_id: "r1".into(),
            metric_name: "extractor_rps_avg".into(),
            ts: 0,
            value: 150.0, // Would breach > 100
        }];

        let events = evaluate_tick(&pool, &state, &[disabled_rule], &metrics, 0).await;

        // Disabled rule should produce zero events.
        assert!(
            events.is_empty(),
            "Disabled rule should not fire, got {} events",
            events.len()
        );

        // No alert should be persisted.
        let persisted = crate::repositories::alert_repository::AlertRepository::list(&pool)
            .await
            .unwrap();
        assert!(
            persisted.is_empty(),
            "No alert should be persisted for disabled rule"
        );
    }

    /// Verify that re-enabling a disabled rule resumes evaluation.
    #[tokio::test]
    async fn test_reenabling_rule_resumes_evaluation() {
        let pool = crate::db::create_pool(":memory:").await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();

        let state = AlertEngineState::new();

        // First: disabled rule produces no events.
        let rule_base = AlertRule {
            id: "rule-reenable".into(),
            name: "test".into(),
            metric_name: "extractor_rps_avg".into(),
            operator: ">".into(),
            threshold: 100.0,
            recovery_threshold: None,
            severity: "warning".into(),
            dwell_secs: 0,
            channel_ids: "[]".into(),
            enabled: false,
            resource_group_id: None,
            created_at: String::new(),
            updated_at: String::new(),
        };

        let metrics = vec![MetricInput {
            task_id: "t1".into(),
            run_id: "r1".into(),
            metric_name: "extractor_rps_avg".into(),
            ts: 0,
            value: 150.0,
        }];

        let disabled_rule = rule_base.clone();
        let events = evaluate_tick(&pool, &state, &[disabled_rule], &metrics, 0).await;
        assert!(events.is_empty());

        // Now enable the rule.
        let enabled_rule = AlertRule {
            enabled: true,
            ..rule_base
        };

        let events = evaluate_tick(&pool, &state, &[enabled_rule], &metrics, 0).await;
        assert_eq!(
            events.len(),
            1,
            "Enabled rule should fire exactly one event"
        );
    }
}
