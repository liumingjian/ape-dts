//! Two-phase orchestration for RDB `snapshot_and_cdc` tasks.
//!
//! The dt-main engine does not natively expose `snapshot_and_cdc` as an
//! `extract_type` for MySQL or Oracle. The zero-data-loss flow is to:
//!
//! 1. Capture the CDC start marker BEFORE starting the snapshot task.
//! 2. Run the snapshot task to completion.
//! 3. Run a CDC task seeded with the captured marker so any rows changed
//!    during the snapshot are replayed (the sinker uses upserts to make this
//!    idempotent).
//!
//! This module materialises that flow inside a single Run by:
//!   - Detecting `is_two_phase_task(task)`.
//!   - Capturing `start_time_utc` or Oracle `start_scn` at start time.
//!   - Rendering two distinct INIs (phase 1 = snapshot, phase 2 = cdc with
//!     the marker set), persisting the phase 2 INI + metadata in the
//!     run directory so the supervisor can spawn it after phase 1 exits
//!     cleanly.
//!
//! No silent fallbacks: if any step fails, the Run fails loudly.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::ini_renderer;
use crate::models::Task;

#[path = "two_phase_oracle.rs"]
mod oracle_phase;

pub const PHASE2_INI_FILE: &str = "phase2.ini";
pub const PHASE_STATE_FILE: &str = "phase_state.json";

/// Persisted phase metadata used by the supervisor to drive the transition
/// from phase 1 (snapshot) to phase 2 (cdc).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PhaseState {
    pub current_phase: u8,
    pub start_time_utc: String,
    pub start_scn: Option<u64>,
    pub phase2_ini_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase2Start {
    Time(String),
    OracleScn(u64),
}

/// Engines for which `snapshot_and_cdc` is currently supported as a managed
/// two-phase Run.
fn is_supported_source(db_type_source: &str) -> bool {
    matches!(
        db_type_source,
        "mysql" | "pg" | "gaussdb_pg" | "gaussdb_mysql" | "gaussdb_oracle" | "oracle"
    )
}

/// Returns `true` iff the task's extractor JSON has
/// `extract_type=snapshot_and_cdc` AND the source engine is one of the
/// supported source engines.
pub fn is_two_phase_task(task: &Task) -> bool {
    if !is_supported_source(&task.db_type_source) {
        return false;
    }
    let extractor: serde_json::Value =
        serde_json::from_str(&task.extractor_config).unwrap_or_default();
    let extract_type = extractor
        .get("extract_type")
        .or_else(|| extractor.get("extractType"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    extract_type == "snapshot_and_cdc"
}

/// Format a `chrono::DateTime<Utc>` as the canonical `start_time_utc` string
/// expected by `dt_common::utils::TimeUtil::datetime_from_utc_str`.
pub fn format_start_time_utc(now: chrono::DateTime<chrono::Utc>) -> String {
    now.format("%Y-%m-%d %H:%M:%S%.3f").to_string()
}

/// Override the `extract_type` (and a snapshot-specific `parallel_type` hint)
/// inside a JSON value, returning the modified value.
fn override_string(mut v: serde_json::Value, key: &str, value: &str) -> serde_json::Value {
    if let Some(obj) = v.as_object_mut() {
        obj.insert(
            key.to_string(),
            serde_json::Value::String(value.to_string()),
        );
    } else {
        let mut obj = serde_json::Map::new();
        obj.insert(
            key.to_string(),
            serde_json::Value::String(value.to_string()),
        );
        v = serde_json::Value::Object(obj);
    }
    v
}

/// Build the phase 1 (snapshot) Task by cloning and overriding fields.
fn make_phase1_task(task: &Task) -> Task {
    let extractor_json: serde_json::Value =
        serde_json::from_str(&task.extractor_config).unwrap_or_default();
    let extractor = override_string(extractor_json, "extract_type", "snapshot");

    let mut clone = task.clone();
    clone.extractor_config = extractor.to_string();
    clone.kind = "snapshot".to_string();
    clone
}

/// Build the phase 2 (cdc) Task seeded with the pre-snapshot capture.
fn make_phase2_task(task: &Task, start: &Phase2Start) -> Task {
    let extractor_json: serde_json::Value =
        serde_json::from_str(&task.extractor_config).unwrap_or_default();
    let extractor = override_string(extractor_json, "extract_type", "cdc");
    let extractor = apply_phase2_start(extractor, start);
    let parallelizer_json: serde_json::Value =
        serde_json::from_str(&task.parallelizer_config).unwrap_or_default();
    let parallelizer = override_string(
        parallelizer_json,
        "parallel_type",
        phase2_parallel_type(task),
    );

    let mut clone = task.clone();
    clone.extractor_config = extractor.to_string();
    clone.parallelizer_config = parallelizer.to_string();
    clone.kind = "cdc".to_string();
    clone
}

fn phase2_parallel_type(task: &Task) -> &'static str {
    if task.db_type_source == "oracle" || task.db_type_target == "oracle" {
        "serial"
    } else {
        "rdb_merge"
    }
}

fn apply_phase2_start(extractor: serde_json::Value, start: &Phase2Start) -> serde_json::Value {
    match start {
        Phase2Start::Time(start_time_utc) => {
            override_string(extractor, "start_time_utc", start_time_utc)
        }
        Phase2Start::OracleScn(start_scn) => {
            let extractor = override_string(extractor, "cdc_mode", "logminer");
            override_u64(extractor, "start_scn", *start_scn)
        }
    }
}

fn override_u64(mut v: serde_json::Value, key: &str, value: u64) -> serde_json::Value {
    if let Some(obj) = v.as_object_mut() {
        obj.insert(key.to_string(), serde_json::Value::from(value));
    } else {
        let mut obj = serde_json::Map::new();
        obj.insert(key.to_string(), serde_json::Value::from(value));
        v = serde_json::Value::Object(obj);
    }
    v
}

/// Render the phase 1 INI for a two-phase task. The caller is expected to
/// invoke `prepare_run_dir` to also persist the phase 2 INI before spawning.
pub fn render_phase1_ini(task: &Task) -> String {
    ini_renderer::render(&make_phase1_task(task))
}

/// Render the phase 2 INI for a two-phase task seeded with `start_time_utc`.
pub fn render_phase2_ini(task: &Task, start_time_utc: &str) -> String {
    ini_renderer::render(&make_phase2_task(
        task,
        &Phase2Start::Time(start_time_utc.to_string()),
    ))
}

/// Capture a CDC start marker before phase 1 starts.
pub async fn capture_phase2_start(task: &Task) -> std::io::Result<TwoPhaseStart> {
    if task.db_type_source == "oracle" {
        let start_scn = oracle_phase::capture_current_scn(task).await?;
        return Ok(TwoPhaseStart {
            start_time_utc: String::new(),
            start_scn: Some(start_scn),
        });
    }

    Ok(TwoPhaseStart {
        start_time_utc: format_start_time_utc(chrono::Utc::now()),
        start_scn: None,
    })
}

/// Capture `start_time_utc`, render both INIs, write the phase 2 INI and a
/// `phase_state.json` marker into `run_dir`, and return the phase 1 INI plus
/// the captured timestamp. Idempotent; safe to call before `LocalExecutor::spawn`.
pub fn prepare_run_dir(
    task: &Task,
    run_dir: &Path,
    start: TwoPhaseStart,
) -> std::io::Result<TwoPhasePrep> {
    std::fs::create_dir_all(run_dir)?;
    let phase1_ini = render_phase1_ini(task);
    let phase2_start = start.to_phase2_start();
    let phase2_ini = ini_renderer::render(&make_phase2_task(task, &phase2_start));

    let phase2_path = run_dir.join(PHASE2_INI_FILE);
    std::fs::write(&phase2_path, &phase2_ini)?;

    let state = PhaseState {
        current_phase: 1,
        start_time_utc: start.start_time_utc.clone(),
        start_scn: start.start_scn,
        phase2_ini_path: phase2_path.to_string_lossy().to_string(),
    };
    let state_json = serde_json::to_string_pretty(&state)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(run_dir.join(PHASE_STATE_FILE), state_json)?;

    Ok(TwoPhasePrep {
        phase1_ini,
        phase2_ini,
        start_time_utc: start.start_time_utc,
        start_scn: start.start_scn,
    })
}

/// Result of [`prepare_run_dir`].
pub struct TwoPhasePrep {
    pub phase1_ini: String,
    pub phase2_ini: String,
    pub start_time_utc: String,
    pub start_scn: Option<u64>,
}

pub struct TwoPhaseStart {
    pub start_time_utc: String,
    pub start_scn: Option<u64>,
}

impl TwoPhaseStart {
    fn to_phase2_start(&self) -> Phase2Start {
        match self.start_scn {
            Some(start_scn) => Phase2Start::OracleScn(start_scn),
            None => Phase2Start::Time(self.start_time_utc.clone()),
        }
    }
}

/// Read the persisted phase state from `run_dir`, if any.
pub fn read_phase_state(run_dir: &Path) -> Option<PhaseState> {
    let path = run_dir.join(PHASE_STATE_FILE);
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Update the persisted phase state, marking phase 2 as the current phase.
pub fn mark_phase_advanced(run_dir: &Path) -> std::io::Result<()> {
    if let Some(mut state) = read_phase_state(run_dir) {
        state.current_phase = 2;
        let state_json = serde_json::to_string_pretty(&state)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(run_dir.join(PHASE_STATE_FILE), state_json)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "two_phase_tests.rs"]
mod tests;
