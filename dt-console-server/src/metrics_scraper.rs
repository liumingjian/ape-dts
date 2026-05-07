//! MetricsScraper — polls each running Run's /metrics:9090 every 10s,
//! parses Prometheus text via prometheus-parse, writes to MetricPointRepository.
//!
//! Scrape failure → metrics_unavailable alert event; recovery clears it.
//! Pause stops ingestion; resume restarts.
//! Never silently drops a parsed sample.

use crate::models::MetricPoint;
use crate::repositories::metric_point_repository::MetricPointRepository;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Default scrape interval in seconds.
const DEFAULT_SCRAPE_INTERVAL_SECS: u64 = 10;

/// Default metrics port on the engine subprocess.
const DEFAULT_METRICS_PORT: u16 = 9090;

/// Number of consecutive scrape failures before emitting metrics_unavailable.
const FAILURE_THRESHOLD: u32 = 3;

/// Target for a single scrape: (task_id, run_id, metrics_host, metrics_port).
#[derive(Debug, Clone)]
pub struct ScrapeTarget {
    pub task_id: String,
    pub run_id: String,
    pub host: String,
    pub port: u16,
}

/// Tracks scrape-failure state per (task_id, run_id).
#[derive(Debug, Clone, Default)]
struct FailureState {
    consecutive_failures: u32,
    alert_fired: bool,
}

/// Shared state for the MetricsScraper, including the set of active targets.
#[derive(Debug, Clone, Default)]
pub struct ScraperState {
    /// Active scrape targets (running Runs).
    targets: Arc<Mutex<Vec<ScrapeTarget>>>,
    /// Per-target failure tracking.
    failures: Arc<Mutex<std::collections::HashMap<String, FailureState>>>,
    /// Set of run_ids currently paused (should not be scraped).
    paused: Arc<Mutex<HashSet<String>>>,
}

impl ScraperState {
    pub fn new() -> Self {
        Self {
            targets: Arc::new(Mutex::new(Vec::new())),
            failures: Arc::new(Mutex::new(std::collections::HashMap::new())),
            paused: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Add or update a scrape target.
    pub async fn add_target(&self, target: ScrapeTarget) {
        let mut targets = self.targets.lock().await;
        // Remove any existing target for the same task_id.
        targets.retain(|t| t.task_id != target.task_id);
        targets.push(target);
    }

    /// Remove a scrape target by task_id.
    pub async fn remove_target(&self, task_id: &str) {
        let mut targets = self.targets.lock().await;
        targets.retain(|t| t.task_id != task_id);
        let mut failures = self.failures.lock().await;
        failures.remove(task_id);
    }

    /// Mark a run as paused (stops scraping).
    pub async fn pause(&self, run_id: &str) {
        let mut paused = self.paused.lock().await;
        paused.insert(run_id.to_string());
    }

    /// Mark a run as resumed (restarts scraping).
    pub async fn resume(&self, run_id: &str) {
        let mut paused = self.paused.lock().await;
        paused.remove(run_id);
    }

    /// Check if a run is paused.
    async fn is_paused(&self, run_id: &str) -> bool {
        let paused = self.paused.lock().await;
        paused.contains(run_id)
    }

    /// Get current targets snapshot.
    async fn get_targets(&self) -> Vec<ScrapeTarget> {
        let targets = self.targets.lock().await;
        targets.clone()
    }

    /// Record a scrape failure for a target.
    /// Returns true if metrics_unavailable alert should be fired.
    async fn record_failure(&self, task_id: &str, run_id: &str) -> bool {
        let mut failures = self.failures.lock().await;
        let key = format!("{task_id}:{run_id}");
        let state = failures.entry(key).or_default();
        state.consecutive_failures += 1;
        if state.consecutive_failures >= FAILURE_THRESHOLD && !state.alert_fired {
            state.alert_fired = true;
            true
        } else {
            false
        }
    }

    /// Record a scrape success for a target.
    /// Returns true if a metrics_unavailable alert should be cleared.
    async fn record_success(&self, task_id: &str, run_id: &str) -> bool {
        let mut failures = self.failures.lock().await;
        let key = format!("{task_id}:{run_id}");
        let was_alerted = failures.get(&key).map(|s| s.alert_fired).unwrap_or(false);
        if let Some(state) = failures.get_mut(&key) {
            state.consecutive_failures = 0;
            if state.alert_fired {
                state.alert_fired = false;
                return true;
            }
        }
        was_alerted
    }
}

/// Parsed sample from the Prometheus text output, with the f64 value extracted.
pub struct ParsedSample {
    pub metric_name: String,
    pub value: f64,
}

/// Parse Prometheus exposition text into a list of (metric_name, f64_value) pairs.
///
/// Extracts the numeric value from Counter, Gauge, and Untyped samples.
/// Histogram and Summary samples are skipped (they are composite types).
/// Never silently drops a known-value sample — unknown metric names are
/// still included with their parsed value.
pub fn parse_prometheus_text(text: &str) -> Vec<ParsedSample> {
    let lines = text.lines().map(|l| Ok(l.to_string()));
    let scrape = match prometheus_parse::Scrape::parse(lines) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("prometheus parse error: {e}");
            return Vec::new();
        }
    };

    let mut results = Vec::new();
    for sample in &scrape.samples {
        let value = match &sample.value {
            prometheus_parse::Value::Counter(v) => *v,
            prometheus_parse::Value::Gauge(v) => *v,
            prometheus_parse::Value::Untyped(v) => *v,
            // Histogram and Summary are composite; log them but don't store as a scalar.
            prometheus_parse::Value::Histogram(_) => {
                tracing::debug!(
                    event = "unknown_metric",
                    metric_type = "histogram",
                    metric_name = %sample.metric,
                    "skipping histogram metric in scrape"
                );
                continue;
            }
            prometheus_parse::Value::Summary(_) => {
                tracing::debug!(
                    event = "unknown_metric",
                    metric_type = "summary",
                    metric_name = %sample.metric,
                    "skipping summary metric in scrape"
                );
                continue;
            }
        };
        results.push(ParsedSample {
            metric_name: sample.metric.clone(),
            value,
        });
    }
    results
}

/// Perform a single scrape of a /metrics endpoint.
async fn scrape_endpoint(host: &str, port: u16) -> Result<String, String> {
    let url = format!("http://{host}:{port}/metrics");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| format!("client build failed: {e}"))?;
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("scrape request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("scrape returned HTTP {}", resp.status()));
    }
    resp.text()
        .await
        .map_err(|e| format!("scrape body read failed: {e}"))
}

/// Run one tick of the scraper: scrape all active targets, write points.
async fn scrape_tick(pool: &sqlx::SqlitePool, state: &ScraperState) {
    let targets = state.get_targets().await;

    for target in targets {
        // Skip paused runs.
        if state.is_paused(&target.run_id).await {
            continue;
        }

        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

        match scrape_endpoint(&target.host, target.port).await {
            Ok(body) => {
                let samples = parse_prometheus_text(&body);

                // Write all parsed samples to the store.
                let points: Vec<MetricPoint> = samples
                    .iter()
                    .map(|s| MetricPoint {
                        id: 0,
                        task_id: target.task_id.clone(),
                        run_id: target.run_id.clone(),
                        metric_name: s.metric_name.clone(),
                        ts: now.clone(),
                        value: s.value,
                    })
                    .collect();

                if !points.is_empty() {
                    if let Err(e) = MetricPointRepository::create_batch(pool, &points).await {
                        tracing::warn!(
                            "metric point batch insert failed for run {}: {e}",
                            target.run_id
                        );
                    }
                }

                // Record success; clear alert if needed.
                let should_clear = state.record_success(&target.task_id, &target.run_id).await;
                if should_clear {
                    tracing::info!(
                        event = "metrics_available",
                        task_id = %target.task_id,
                        run_id = %target.run_id,
                        "metrics scrape recovered"
                    );
                }
            }
            Err(e) => {
                // Record failure; fire alert if threshold reached.
                let should_alert = state.record_failure(&target.task_id, &target.run_id).await;
                if should_alert {
                    tracing::warn!(
                        event = "metrics_unavailable",
                        task_id = %target.task_id,
                        run_id = %target.run_id,
                        reason = %e,
                        "metrics_unavailable alert: scrape failed"
                    );
                }
            }
        }
    }
}

/// Spawn the background scraper loop.
///
/// Runs every `interval_secs` seconds, scraping all active targets.
/// This function does not block — it spawns a tokio task and returns.
pub fn spawn_scraper(pool: sqlx::SqlitePool, state: ScraperState, interval_secs: u64) {
    let interval = if interval_secs == 0 {
        DEFAULT_SCRAPE_INTERVAL_SECS
    } else {
        interval_secs
    };

    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(interval)).await;
            scrape_tick(&pool, &state).await;
        }
    });
}

/// Create a ScrapeTarget from Run information.
///
/// Uses the metrics_config from the Task to determine host/port.
/// If metrics is not configured, defaults to 127.0.0.1:9090.
pub fn scrape_target_from_run(task_id: &str, run_id: &str) -> ScrapeTarget {
    ScrapeTarget {
        task_id: task_id.to_string(),
        run_id: run_id.to_string(),
        host: "127.0.0.1".to_string(),
        port: DEFAULT_METRICS_PORT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_prometheus_text_extracts_gauge_values() {
        let text = "# HELP extractor_rps_avg Average RPS\n\
                     # TYPE extractor_rps_avg gauge\n\
                     extractor_rps_avg 42.5\n";
        let samples = parse_prometheus_text(text);
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].metric_name, "extractor_rps_avg");
        assert!((samples[0].value - 42.5).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_prometheus_text_stores_names_verbatim() {
        let text =
            "extractor_rps_avg 1.0\npipeline_buffer_size_avg 2.0\nsinker_bps_avg_by_sec 3.0\n";
        let samples = parse_prometheus_text(text);
        assert_eq!(samples.len(), 3);
        assert_eq!(samples[0].metric_name, "extractor_rps_avg");
        assert_eq!(samples[1].metric_name, "pipeline_buffer_size_avg");
        assert_eq!(samples[2].metric_name, "sinker_bps_avg_by_sec");
    }

    #[test]
    fn parse_prometheus_text_handles_counter_and_untyped() {
        let text = "# TYPE my_counter counter\n\
                     my_counter_total 100\n\
                     unknown_metric 55.5\n";
        let samples = parse_prometheus_text(text);
        assert_eq!(samples.len(), 2);
        assert_eq!(samples[0].metric_name, "my_counter_total");
        assert!((samples[0].value - 100.0).abs() < f64::EPSILON);
        assert_eq!(samples[1].metric_name, "unknown_metric");
        assert!((samples[1].value - 55.5).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn scraper_state_add_remove_target() {
        let state = ScraperState::new();
        state
            .add_target(ScrapeTarget {
                task_id: "t1".into(),
                run_id: "r1".into(),
                host: "127.0.0.1".into(),
                port: 9090,
            })
            .await;
        let targets = state.get_targets().await;
        assert_eq!(targets.len(), 1);

        state.remove_target("t1").await;
        let targets = state.get_targets().await;
        assert!(targets.is_empty());
    }

    #[tokio::test]
    async fn scraper_state_pause_resume() {
        let state = ScraperState::new();
        state.pause("r1").await;
        assert!(state.is_paused("r1").await);
        state.resume("r1").await;
        assert!(!state.is_paused("r1").await);
    }

    #[tokio::test]
    async fn scraper_state_failure_threshold_fires_alert() {
        let state = ScraperState::new();
        // First two failures should not fire.
        assert!(!state.record_failure("t1", "r1").await);
        assert!(!state.record_failure("t1", "r1").await);
        // Third failure should fire.
        assert!(state.record_failure("t1", "r1").await);
        // Fourth failure should NOT re-fire (already fired).
        assert!(!state.record_failure("t1", "r1").await);
    }

    #[tokio::test]
    async fn scraper_state_success_clears_alert() {
        let state = ScraperState::new();
        // Fire the alert.
        assert!(!state.record_failure("t1", "r1").await);
        assert!(!state.record_failure("t1", "r1").await);
        assert!(state.record_failure("t1", "r1").await);
        // Success should clear the alert.
        assert!(state.record_success("t1", "r1").await);
        // Subsequent success should not claim to clear again.
        assert!(!state.record_success("t1", "r1").await);
    }
}
