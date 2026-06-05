//! HTTP handler for GET /api/runs/:id/objects.
//!
//! Reads `<run.log_dir>/finished.log`, parses each line as an engine
//! `RdbSnapshotFinished` event, joins against the planned table list from the
//! task's `filter_config.do_tbs`, and returns `[{schema, table, state}]` where
//! state is `pending | loading | completed`.
//!
//! - 404 + RUN_NOT_FOUND for unknown run_id.
//! - 200 + `[]` when finished.log is missing or run has no log_dir / task_id.
//! - Malformed log lines are skipped (warn log emitted).
//! - Duplicate `RdbSnapshotFinished` lines for the same (schema, table) are
//!   idempotent (single row in output).
//! - Lines referencing a table NOT in the planned list are ignored.

use actix_web::{get, web, HttpResponse, ResponseError};
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;

use crate::error::{codes, ApiError};
use crate::middleware::rbac::{self, RbacAction};
use crate::models::UserContext;
use crate::repositories::run_repository::RunRepository;
use crate::repositories::task_repository::TaskRepository;

/// Response item for the objects endpoint.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ObjectState {
    schema: String,
    table: String,
    state: String,
}

/// A parsed `do_tbs` pattern. Used to match completed tables against
/// the task's filter configuration.
#[derive(Debug, Clone)]
enum TablePattern {
    /// Exact match: `schema.table`
    Exact { schema: String, table: String },
    /// Schema wildcard: `schema.*` matches any table in the given schema.
    SchemaWildcard { schema: String },
    /// Global wildcard: `*.*` matches any table in any schema.
    GlobalWildcard,
}

impl TablePattern {
    fn matches(&self, schema: &str, table: &str) -> bool {
        match self {
            TablePattern::Exact { schema: s, table: t } => s == schema && t == table,
            TablePattern::SchemaWildcard { schema: s } => s == schema,
            TablePattern::GlobalWildcard => true,
        }
    }
}

/// GET /api/runs/:id/objects — per-table object state for a Run.
#[get("/runs/{id}/objects")]
pub async fn get_objects(
    pool: web::Data<sqlx::SqlitePool>,
    user: UserContext,
    path: web::Path<String>,
) -> HttpResponse {
    if let Err(e) = rbac::require_action(&user, RbacAction::TaskRead) {
        return e.error_response();
    }

    let run_id = path.into_inner();

    // Validate the Run exists.
    let run = match RunRepository::find_by_id(&pool, &run_id).await {
        Ok(r) => r,
        Err(_) => {
            return ApiError::with_details(
                codes::RUN_NOT_FOUND,
                "Run not found",
                serde_json::json!({ "id": run_id }),
            )
            .error_response();
        }
    };

    // If the run has no log_dir, return [] (nothing to read).
    let log_dir = match &run.log_dir {
        Some(d) if !d.is_empty() => d.clone(),
        _ => return HttpResponse::Ok().json(Vec::<ObjectState>::new()),
    };

    // If the run has no task_id, we can't look up the planned table list.
    let task_id = match &run.task_id {
        Some(tid) => tid.clone(),
        None => return HttpResponse::Ok().json(Vec::<ObjectState>::new()),
    };

    // Look up the task to get the filter config.
    let task = match TaskRepository::find_by_id(&pool, &task_id).await {
        Ok(t) => t,
        Err(_) => return HttpResponse::Ok().json(Vec::<ObjectState>::new()),
    };

    // Parse planned table patterns from filter_config.do_tbs.
    let patterns = parse_table_patterns(&task.filter_config);

    // If there are no patterns, we can't produce any table states.
    if patterns.is_empty() {
        return HttpResponse::Ok().json(Vec::<ObjectState>::new());
    }

    // Read and parse finished.log.
    // If the file doesn't exist yet, return [] (VAL-ORCH-024).
    let finished_path = Path::new(&log_dir).join("finished.log");
    if !finished_path.exists() {
        return HttpResponse::Ok().json(Vec::<ObjectState>::new());
    }

    let completed_tables = read_finished_log(&finished_path);

    // Build response:
    // 1. Explicit (non-wildcard) entries from do_tbs, all as "pending".
    // 2. Mark entries as "completed" if they appear in finished.log.
    // 3. Add completed tables matching wildcard patterns but not in the
    //    explicit list (appear directly as "completed").
    let mut response: Vec<ObjectState> = Vec::new();
    let mut seen = HashSet::new();

    for pattern in &patterns {
        if let TablePattern::Exact { schema, table } = pattern {
            let state = if completed_tables.contains(&(schema.clone(), table.clone())) {
                "completed"
            } else {
                "pending"
            };
            if seen.insert((schema.clone(), table.clone())) {
                response.push(ObjectState {
                    schema: schema.clone(),
                    table: table.clone(),
                    state: state.to_string(),
                });
            }
        }
    }

    // Completed tables matching wildcard patterns but not already listed.
    for (schema, table) in &completed_tables {
        if seen.contains(&(schema.clone(), table.clone())) {
            continue;
        }
        let matches_any = patterns.iter().any(|p| p.matches(schema, table));
        if matches_any {
            if seen.insert((schema.clone(), table.clone())) {
                response.push(ObjectState {
                    schema: schema.clone(),
                    table: table.clone(),
                    state: "completed".to_string(),
                });
            }
        }
    }

    HttpResponse::Ok().json(response)
}

/// Parse `do_tbs` from `filter_config` into a list of table patterns.
///
/// `do_tbs` can be:
/// - A comma-separated string: `"schema1.table1,schema2.table2"`
/// - A JSON array: `["schema1.table1", "schema2.table2"]`
/// - Empty / missing / null
///
/// Each entry is split on the first `.` to extract a pattern:
/// - `schema.table` → Exact match
/// - `schema.*` → SchemaWildcard (any table in that schema)
/// - `*.*` → GlobalWildcard (any table in any schema)
fn parse_table_patterns(filter_config: &str) -> Vec<TablePattern> {
    let filter: serde_json::Value = serde_json::from_str(filter_config).unwrap_or_default();
    let do_tbs = filter.get("do_tbs").or_else(|| filter.get("doTbs"));

    let raw_patterns: Vec<String> = match do_tbs {
        Some(serde_json::Value::String(s)) => s
            .split(',')
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .map(String::from)
            .collect(),
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
            .filter(|p| !p.is_empty())
            .collect(),
        _ => return Vec::new(),
    };

    let mut patterns = Vec::new();
    let mut seen_exact = HashSet::new();

    for raw in raw_patterns {
        if let Some(dot) = raw.find('.') {
            let schema_part = &raw[..dot];
            let table_part = &raw[dot + 1..];
            if schema_part.is_empty() || table_part.is_empty() {
                continue;
            }
            if schema_part == "*" && table_part == "*" {
                patterns.push(TablePattern::GlobalWildcard);
            } else if table_part == "*" {
                patterns.push(TablePattern::SchemaWildcard {
                    schema: schema_part.to_string(),
                });
            } else {
                let key = (schema_part.to_string(), table_part.to_string());
                if seen_exact.insert(key.clone()) {
                    patterns.push(TablePattern::Exact {
                        schema: key.0,
                        table: key.1,
                    });
                }
            }
        }
    }

    patterns
}

/// Read `finished.log` and return the set of (schema, table) pairs that
/// have completed (i.e., appear as `RdbSnapshotFinished`).
///
/// Each line has the format:
/// `2024-04-01 03:25:18.701725 | {"type":"RdbSnapshotFinished","db_type":"mysql","schema":"test_db_1","tb":"one_pk_no_uk"}`
///
/// Malformed lines are skipped with a warn-level log.
fn read_finished_log(path: &Path) -> HashSet<(String, String)> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(path = %path.display(), "failed to read finished.log: {e}");
            return HashSet::new();
        }
    };

    let mut completed = HashSet::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Find the JSON portion: everything between the first '{' and last '}'.
        let left = match line.find('{') {
            Some(i) => i,
            None => {
                tracing::warn!(line = line, "malformed finished.log line: no JSON object found");
                continue;
            }
        };
        let right = match line.rfind('}') {
            Some(i) => i,
            None => {
                tracing::warn!(line = line, "malformed finished.log line: no closing brace");
                continue;
            }
        };

        let json_str = &line[left..=right];
        let json_val: serde_json::Value = match serde_json::from_str(json_str) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(line = line, "malformed finished.log line: invalid JSON: {e}");
                continue;
            }
        };

        // Only process RdbSnapshotFinished entries.
        let entry_type = json_val.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if entry_type != "RdbSnapshotFinished" {
            continue;
        }

        let schema = match json_val.get("schema").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => {
                tracing::warn!(line = line, "RdbSnapshotFinished missing schema field");
                continue;
            }
        };
        let table = match json_val.get("tb").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => {
                tracing::warn!(line = line, "RdbSnapshotFinished missing tb field");
                continue;
            }
        };

        completed.insert((schema, table));
    }

    completed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_table_patterns_comma_separated() {
        let filter = r#"{"do_dbs":"","do_tbs":"test_db.t1,test_db.t2","ignore_dbs":"","ignore_tbs":""}"#;
        let patterns = parse_table_patterns(filter);
        assert_eq!(patterns.len(), 2);
        assert!(matches!(
            &patterns[0],
            TablePattern::Exact { schema, table }
            if schema == "test_db" && table == "t1"
        ));
        assert!(matches!(
            &patterns[1],
            TablePattern::Exact { schema, table }
            if schema == "test_db" && table == "t2"
        ));
    }

    #[test]
    fn test_parse_table_patterns_array() {
        let filter = r#"{"do_dbs":[],"do_tbs":["db1.tbl1","db2.tbl2"],"ignore_dbs":[]}"#;
        let patterns = parse_table_patterns(filter);
        assert_eq!(patterns.len(), 2);
    }

    #[test]
    fn test_parse_table_patterns_empty() {
        let filter = r#"{"do_dbs":"","do_tbs":"","ignore_dbs":""}"#;
        let patterns = parse_table_patterns(filter);
        assert!(patterns.is_empty());
    }

    #[test]
    fn test_parse_table_patterns_deduplicates_exact() {
        let filter = r#"{"do_tbs":"db.t1,db.t1"}"#;
        let patterns = parse_table_patterns(filter);
        assert_eq!(patterns.len(), 1);
    }

    #[test]
    fn test_parse_table_patterns_schema_wildcard() {
        let filter = r#"{"do_tbs":"test_db.*"}"#;
        let patterns = parse_table_patterns(filter);
        assert_eq!(patterns.len(), 1);
        assert!(matches!(
            &patterns[0],
            TablePattern::SchemaWildcard { schema }
            if schema == "test_db"
        ));
    }

    #[test]
    fn test_parse_table_patterns_global_wildcard() {
        let filter = r#"{"do_tbs":"*.*"}"#;
        let patterns = parse_table_patterns(filter);
        assert_eq!(patterns.len(), 1);
        assert!(matches!(&patterns[0], TablePattern::GlobalWildcard));
    }

    #[test]
    fn test_table_pattern_matches() {
        let exact = TablePattern::Exact {
            schema: "test_db".to_string(),
            table: "t1".to_string(),
        };
        assert!(exact.matches("test_db", "t1"));
        assert!(!exact.matches("test_db", "t2"));
        assert!(!exact.matches("other_db", "t1"));

        let schema_wc = TablePattern::SchemaWildcard {
            schema: "test_db".to_string(),
        };
        assert!(schema_wc.matches("test_db", "t1"));
        assert!(schema_wc.matches("test_db", "anything"));
        assert!(!schema_wc.matches("other_db", "t1"));

        let global = TablePattern::GlobalWildcard;
        assert!(global.matches("any_db", "any_table"));
    }

    #[test]
    fn test_read_finished_log() {
        let dir = std::env::temp_dir().join("dt-objects-unit-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("finished.log");
        std::fs::write(
            &path,
            r#"2024-04-01 03:25:18.701725 | {"type":"RdbSnapshotFinished","db_type":"mysql","schema":"test_db_1","tb":"one_pk_no_uk"}
2024-04-01 03:25:19.701725 | {"type":"RdbSnapshotFinished","db_type":"mysql","schema":"test_db_1","tb":"numeric_table"}"#,
        )
        .unwrap();

        let completed = read_finished_log(&path);
        assert_eq!(completed.len(), 2);
        assert!(completed.contains(&("test_db_1".to_string(), "one_pk_no_uk".to_string())));
        assert!(completed.contains(&("test_db_1".to_string(), "numeric_table".to_string())));
    }

    #[test]
    fn test_read_finished_log_malformed_lines_skipped() {
        let dir = std::env::temp_dir().join("dt-objects-unit-malformed");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("finished.log");
        std::fs::write(
            &path,
            r#"2024-04-01 03:25:18.701725 | {"type":"RdbSnapshotFinished","db_type":"mysql","schema":"test_db","tb":"t1"}
not valid json
truncated without braces
2024-04-01 03:25:19.701725 | {"type":"RdbSnapshotFinished","db_type":"mysql","schema":"test_db","tb":"t2"}"#,
        )
        .unwrap();

        let completed = read_finished_log(&path);
        assert_eq!(completed.len(), 2);
    }

    #[test]
    fn test_read_finished_log_duplicate_lines() {
        let dir = std::env::temp_dir().join("dt-objects-unit-dup");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("finished.log");
        std::fs::write(
            &path,
            format!(
                "{line}\n{line}",
                line = r#"2024-04-01 03:25:18.701725 | {"type":"RdbSnapshotFinished","db_type":"mysql","schema":"test_db","tb":"t1"}"#
            ),
        )
        .unwrap();

        let completed = read_finished_log(&path);
        assert_eq!(completed.len(), 1);
    }

    #[test]
    fn test_read_finished_log_ignores_task_finished() {
        let dir = std::env::temp_dir().join("dt-objects-unit-taskfin");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("finished.log");
        std::fs::write(
            &path,
            r#"2024-04-01 03:25:18.701725 | {"type":"RdbSnapshotFinished","db_type":"mysql","schema":"test_db","tb":"t1"}
2024-04-01 03:26:00.000000 | task finished"#,
        )
        .unwrap();

        let completed = read_finished_log(&path);
        assert_eq!(completed.len(), 1);
        assert!(completed.contains(&("test_db".to_string(), "t1".to_string())));
    }
}
