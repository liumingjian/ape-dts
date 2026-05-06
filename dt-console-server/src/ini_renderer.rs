//! IniRenderer — pure function Task → INI string built on dt-common::config::* types.
//!
//! The renderer converts a persisted Task model (with JSON config columns) into
//! an INI text that is byte-compatible with `dt_common::config::TaskConfig::new()`.
//!
//! Design constraints:
//! - Deterministic: same input always produces the same output (no HashMap iteration).
//! - Optional sections (`[processor]`, `[data_marker]`, `[metacenter]`) are omitted
//!   when the corresponding JSON config is empty/null.
//! - `[metrics]` section is emitted only when the `metrics` feature is enabled AND
//!   `metrics_config` is non-empty in the Task.
//! - Sections are emitted in a stable order matching the engine's INI convention:
//!   `[global]`, `[extractor]`, `[sinker]`, `[filter]`, `[router]`,
//!   `[parallelizer]`, `[pipeline]`, `[resumer]`, `[runtime]`,
//!   optional: `[processor]`, `[data_marker]`, `[metacenter]`, `[metrics]`.

use crate::models::Task;

/// Render a Task model to an INI string.
///
/// The output is deterministic: for the same Task input, the same byte string
/// is always produced. Optional sections are omitted when their JSON config
/// is empty/null. The `[metrics]` section is gated on the `metrics` Cargo
/// feature and non-empty `metrics_config`.
pub fn render(task: &Task) -> String {
    let mut sections: Vec<(String, Vec<(String, String)>)> = Vec::new();

    // ── [global] ──────────────────────────────────────────────────────────
    sections.push((
        "global".to_string(),
        vec![("task_id".to_string(), task.task_id.clone())],
    ));

    // ── [extractor] ──────────────────────────────────────────────────────
    let extractor: serde_json::Value =
        serde_json::from_str(&task.extractor_config).unwrap_or_default();
    sections.push(render_extractor(&task.db_type_source, &extractor));

    // ── [sinker] ──────────────────────────────────────────────────────────
    let sinker: serde_json::Value = serde_json::from_str(&task.sinker_config).unwrap_or_default();
    let source_endpoint: serde_json::Value =
        serde_json::from_str(&task.source_endpoint).unwrap_or_default();
    sections.push(render_sinker(
        &task.db_type_target,
        &sinker,
        &source_endpoint,
    ));

    // ── [filter] ──────────────────────────────────────────────────────────
    let filter: serde_json::Value = serde_json::from_str(&task.filter_config).unwrap_or_default();
    sections.push(render_filter(&filter));

    // ── [router] ──────────────────────────────────────────────────────────
    let router: serde_json::Value = serde_json::from_str(&task.router_config).unwrap_or_default();
    sections.push(render_router(&router));

    // ── [parallelizer] ────────────────────────────────────────────────────
    let parallelizer: serde_json::Value =
        serde_json::from_str(&task.parallelizer_config).unwrap_or_default();
    sections.push(render_parallelizer(&parallelizer, &task.kind));

    // ── [pipeline] ────────────────────────────────────────────────────────
    let pipeline: serde_json::Value =
        serde_json::from_str(&task.pipeline_config).unwrap_or_default();
    sections.push(render_pipeline(&pipeline));

    // ── [resumer] ─────────────────────────────────────────────────────────
    let resumer: serde_json::Value = serde_json::from_str(&task.resumer_config).unwrap_or_default();
    let resumer_section = render_resumer(&resumer, &task.runtime_config);
    if !resumer_section.1.is_empty() {
        sections.push(resumer_section);
    }

    // ── [runtime] ─────────────────────────────────────────────────────────
    let runtime: serde_json::Value = serde_json::from_str(&task.runtime_config).unwrap_or_default();
    sections.push(render_runtime(&runtime));

    // ── Optional sections ──────────────────────────────────────────────────
    // [processor]
    let processor: serde_json::Value =
        serde_json::from_str(&task.processor_config).unwrap_or_default();
    if has_non_empty_fields(&processor) {
        sections.push(render_processor(&processor));
    }

    // [data_marker]
    // Not present in current Task model columns — omitted unless extended.

    // [metacenter]
    // Not present in current Task model columns — omitted unless extended.

    // [metrics] — gated on feature flag AND non-empty config
    #[cfg(feature = "metrics")]
    {
        let metrics: serde_json::Value =
            serde_json::from_str(&task.metrics_config).unwrap_or_default();
        if has_non_empty_fields(&metrics) {
            sections.push(render_metrics(&metrics));
        }
    }

    // ── Assemble INI text ─────────────────────────────────────────────────
    let mut out = String::new();
    for (i, (section_name, kv_pairs)) in sections.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&format!("[{section_name}]\n"));
        for (key, value) in kv_pairs {
            out.push_str(&format!("{key}={value}\n"));
        }
    }
    out
}

/// Check whether a JSON object has any non-empty fields.
fn has_non_empty_fields(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Object(map) => map.values().any(|val| !is_empty_value(val)),
        _ => false,
    }
}

fn is_empty_value(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Null => true,
        serde_json::Value::String(s) => s.is_empty(),
        serde_json::Value::Object(m) => m.is_empty(),
        serde_json::Value::Array(a) => a.is_empty(),
        serde_json::Value::Bool(b) => !b, // false = empty, true = has content
        serde_json::Value::Number(n) => n.as_f64() == Some(0.0),
    }
}

// ── Section renderers ──────────────────────────────────────────────────────

fn render_extractor(
    db_type_source: &str,
    extractor: &serde_json::Value,
) -> (String, Vec<(String, String)>) {
    let mut kv = Vec::new();

    let extract_type = extractor
        .get("extractType")
        .or_else(|| extractor.get("extract_type"))
        .and_then(|v| v.as_str())
        .unwrap_or("snapshot");

    // Always present
    kv.push(("db_type".into(), db_type_source.into()));
    kv.push(("extract_type".into(), extract_type.into()));

    // URL — prefer the top-level `url` field; fall back to `source_endpoint.url`
    let url = extractor.get("url").and_then(|v| v.as_str()).unwrap_or("");
    if !url.is_empty() {
        kv.push(("url".into(), url.into()));
    }

    // Connection auth — username/password
    let username = extractor
        .get("username")
        .or_else(|| {
            extractor
                .get("connection_auth")
                .and_then(|ca| ca.get("username"))
        })
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let password = extractor
        .get("password")
        .or_else(|| {
            extractor
                .get("connection_auth")
                .and_then(|ca| ca.get("password"))
        })
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if !username.is_empty() {
        kv.push(("username".into(), username.into()));
    }
    if !password.is_empty() {
        kv.push(("password".into(), password.into()));
    }

    // Extract-type-specific fields — emitted in a deterministic order
    match extract_type {
        "snapshot" => {
            push_opt_usize(&mut kv, extractor, "sample_interval", "sample_interval");
            push_opt_usize(&mut kv, extractor, "parallel_size", "parallel_size");
            push_opt_usize(&mut kv, extractor, "batch_size", "batch_size");
            push_opt_string(&mut kv, extractor, "partition_cols", "partition_cols");
        }
        "cdc" => {
            // MySQL / GaussDB MySQL CDC fields
            push_opt_string(&mut kv, extractor, "binlog_filename", "binlog_filename");
            push_opt_u32(&mut kv, extractor, "binlog_position", "binlog_position");
            push_opt_u64(&mut kv, extractor, "server_id", "server_id");
            push_opt_bool(&mut kv, extractor, "gtid_enabled", "gtid_enabled");
            push_opt_string(&mut kv, extractor, "gtid_set", "gtid_set");
            push_opt_u64(
                &mut kv,
                extractor,
                "binlog_heartbeat_interval_secs",
                "binlog_heartbeat_interval_secs",
            );
            push_opt_u64(
                &mut kv,
                extractor,
                "binlog_timeout_secs",
                "binlog_timeout_secs",
            );
            // Common CDC fields
            push_opt_u64(
                &mut kv,
                extractor,
                "heartbeat_interval_secs",
                "heartbeat_interval_secs",
            );
            push_opt_string(&mut kv, extractor, "heartbeat_tb", "heartbeat_tb");
            push_opt_u64(
                &mut kv,
                extractor,
                "keepalive_idle_secs",
                "keepalive_idle_secs",
            );
            push_opt_u64(
                &mut kv,
                extractor,
                "keepalive_interval_secs",
                "keepalive_interval_secs",
            );

            // PG / GaussDB CDC fields
            push_opt_string(&mut kv, extractor, "slot_name", "slot_name");
            push_opt_string(&mut kv, extractor, "pub_name", "pub_name");
            push_opt_string(&mut kv, extractor, "start_lsn", "start_lsn");
            push_opt_bool(
                &mut kv,
                extractor,
                "recreate_slot_if_exists",
                "recreate_slot_if_exists",
            );
            push_opt_string(&mut kv, extractor, "ddl_meta_tb", "ddl_meta_tb");

            // Oracle CDC fields
            push_opt_string(&mut kv, extractor, "cdc_mode", "cdc_mode");
            push_opt_u64(
                &mut kv,
                extractor,
                "poll_interval_millis",
                "poll_interval_millis",
            );
            push_opt_usize(&mut kv, extractor, "poll_batch_size", "poll_batch_size");
            push_opt_u64(&mut kv, extractor, "start_scn", "start_scn");

            // Time range
            push_opt_string(&mut kv, extractor, "start_time_utc", "start_time_utc");
            push_opt_string(&mut kv, extractor, "end_time_utc", "end_time_utc");
        }
        "check_log" => {
            push_req_string(&mut kv, extractor, "check_log_dir", "check_log_dir");
            push_opt_usize(&mut kv, extractor, "batch_size", "batch_size");
        }
        "struct" => {
            push_opt_usize(&mut kv, extractor, "db_batch_size", "db_batch_size");
        }
        _ => {}
    }

    ("extractor".into(), kv)
}

fn render_sinker(
    db_type_target: &str,
    sinker: &serde_json::Value,
    source_endpoint: &serde_json::Value,
) -> (String, Vec<(String, String)>) {
    let mut kv = Vec::new();

    let sink_type = sinker
        .get("sinkType")
        .or_else(|| sinker.get("sink_type"))
        .and_then(|v| v.as_str())
        .unwrap_or("write");

    // Always present
    kv.push(("db_type".into(), db_type_target.into()));
    kv.push(("sink_type".into(), sink_type.into()));

    // URL
    let url = sinker.get("url").and_then(|v| v.as_str()).unwrap_or("");
    if !url.is_empty() {
        kv.push(("url".into(), url.into()));
    } else {
        // For struct/check, source URL may serve as sinker URL
        let src_url = source_endpoint
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !src_url.is_empty() && (sink_type == "struct" || sink_type == "check") {
            kv.push(("url".into(), src_url.into()));
        }
    }

    // Connection auth
    let username = sinker
        .get("username")
        .or_else(|| {
            sinker
                .get("connection_auth")
                .and_then(|ca| ca.get("username"))
        })
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let password = sinker
        .get("password")
        .or_else(|| {
            sinker
                .get("connection_auth")
                .and_then(|ca| ca.get("password"))
        })
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if !username.is_empty() {
        kv.push(("username".into(), username.into()));
    }
    if !password.is_empty() {
        kv.push(("password".into(), password.into()));
    }

    // Common sinker fields
    push_opt_usize(&mut kv, sinker, "batch_size", "batch_size");
    push_opt_u32(&mut kv, sinker, "max_connections", "max_connections");

    // Type-specific
    match sink_type {
        "write" => {
            push_opt_bool(&mut kv, sinker, "replace", "replace");
            push_opt_bool(
                &mut kv,
                sinker,
                "disable_foreign_key_checks",
                "disable_foreign_key_checks",
            );
            push_opt_string(
                &mut kv,
                sinker,
                "transaction_isolation",
                "transaction_isolation",
            );
        }
        "check" => {
            push_opt_string(&mut kv, sinker, "check_log_dir", "check_log_dir");
            push_opt_string(
                &mut kv,
                sinker,
                "check_log_file_size",
                "check_log_file_size",
            );
            push_opt_bool(&mut kv, sinker, "output_full_row", "output_full_row");
            push_opt_bool(&mut kv, sinker, "output_revise_sql", "output_revise_sql");
            push_opt_bool(
                &mut kv,
                sinker,
                "revise_match_full_row",
                "revise_match_full_row",
            );
            push_opt_u64(
                &mut kv,
                sinker,
                "retry_interval_secs",
                "retry_interval_secs",
            );
            push_opt_u32(&mut kv, sinker, "max_retries", "max_retries");
        }
        "struct" => {
            push_opt_string(&mut kv, sinker, "conflict_policy", "conflict_policy");
        }
        "sql" => {
            push_opt_bool(&mut kv, sinker, "reverse", "reverse");
        }
        _ => {}
    }

    ("sinker".into(), kv)
}

fn render_filter(filter: &serde_json::Value) -> (String, Vec<(String, String)>) {
    let mut kv = Vec::new();

    // Deterministic key order
    push_opt_string(&mut kv, filter, "do_dbs", "do_dbs");
    push_opt_string(&mut kv, filter, "ignore_dbs", "ignore_dbs");
    push_opt_string(&mut kv, filter, "do_tbs", "do_tbs");
    push_opt_string(&mut kv, filter, "ignore_tbs", "ignore_tbs");
    push_opt_string(&mut kv, filter, "do_events", "do_events");
    push_opt_string(&mut kv, filter, "do_ddls", "do_ddls");
    push_opt_string(&mut kv, filter, "do_dcls", "do_dcls");
    push_opt_string(&mut kv, filter, "do_structures", "do_structures");
    push_opt_string(&mut kv, filter, "ignore_cols", "ignore_cols");
    push_opt_string(&mut kv, filter, "ignore_cmds", "ignore_cmds");
    push_opt_string(&mut kv, filter, "where_conditions", "where_conditions");

    // Ensure do_dbs/do_tbs/ignore_dbs/ignore_tbs always present with at least empty string
    ensure_key_present(&mut kv, "do_dbs");
    ensure_key_present(&mut kv, "ignore_dbs");
    ensure_key_present(&mut kv, "do_tbs");
    ensure_key_present(&mut kv, "ignore_tbs");

    ("filter".into(), kv)
}

fn render_router(router: &serde_json::Value) -> (String, Vec<(String, String)>) {
    let mut kv = Vec::new();

    push_opt_string(&mut kv, router, "db_map", "db_map");
    push_opt_string(&mut kv, router, "tb_map", "tb_map");
    push_opt_string(&mut kv, router, "col_map", "col_map");
    push_opt_string(&mut kv, router, "topic_map", "topic_map");

    // Always emit these even when empty
    ensure_key_present(&mut kv, "db_map");
    ensure_key_present(&mut kv, "tb_map");
    ensure_key_present(&mut kv, "col_map");

    ("router".into(), kv)
}

fn render_parallelizer(
    parallelizer: &serde_json::Value,
    kind: &str,
) -> (String, Vec<(String, String)>) {
    let mut kv = Vec::new();

    // Derive parallel_type from kind if not explicitly set
    let parallel_type = parallelizer
        .get("parallel_type")
        .and_then(|v| v.as_str())
        .unwrap_or(match kind {
            "snapshot" => "snapshot",
            "check" => "rdb_check",
            "cdc" => "rdb_merge",
            "struct" => "serial",
            _ => "serial",
        });

    let parallel_size = parallelizer
        .get("parallel_size")
        .and_then(|v| v.as_u64())
        .unwrap_or(1);

    kv.push(("parallel_type".into(), parallel_type.into()));
    kv.push(("parallel_size".into(), parallel_size.to_string()));

    ("parallelizer".into(), kv)
}

fn render_pipeline(pipeline: &serde_json::Value) -> (String, Vec<(String, String)>) {
    let mut kv = Vec::new();

    let buffer_size = pipeline
        .get("buffer_size")
        .or_else(|| {
            pipeline
                .get("capacity_limiter")
                .and_then(|cl| cl.get("buffer_size"))
        })
        .and_then(|v| v.as_u64())
        .unwrap_or(16000);
    let checkpoint_interval_secs = pipeline
        .get("checkpoint_interval_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(10);

    kv.push(("buffer_size".into(), buffer_size.to_string()));
    kv.push((
        "checkpoint_interval_secs".into(),
        checkpoint_interval_secs.to_string(),
    ));

    push_opt_usize(
        &mut kv,
        pipeline,
        "batch_sink_interval_secs",
        "batch_sink_interval_secs",
    );
    push_opt_u64(
        &mut kv,
        pipeline,
        "counter_time_window_secs",
        "counter_time_window_secs",
    );
    push_opt_u64(
        &mut kv,
        pipeline,
        "counter_max_sub_count",
        "counter_max_sub_count",
    );
    push_opt_string(&mut kv, pipeline, "pipeline_type", "pipeline_type");

    ("pipeline".into(), kv)
}

fn render_resumer(
    resumer: &serde_json::Value,
    runtime_config: &str,
) -> (String, Vec<(String, String)>) {
    let mut kv = Vec::new();

    let resume_type = resumer
        .get("resume_type")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if resume_type.is_empty() || resume_type == "dummy" {
        // No [resumer] section for Dummy type
        return ("resumer".into(), kv);
    }

    kv.push(("resume_type".into(), resume_type.into()));

    match resume_type {
        "from_log" => {
            let runtime: serde_json::Value =
                serde_json::from_str(runtime_config).unwrap_or_default();
            let default_log_dir = runtime
                .get("log_dir")
                .and_then(|v| v.as_str())
                .unwrap_or("./logs");
            let log_dir = resumer
                .get("log_dir")
                .and_then(|v| v.as_str())
                .unwrap_or(default_log_dir);
            kv.push(("log_dir".into(), log_dir.into()));
            push_opt_string(&mut kv, resumer, "config_file", "config_file");
        }
        "from_target" | "from_db" => {
            push_opt_string(&mut kv, resumer, "url", "url");
            push_opt_string(&mut kv, resumer, "db_type", "db_type");
            push_opt_string(&mut kv, resumer, "table_full_name", "table_full_name");
            push_opt_usize(&mut kv, resumer, "max_connections", "max_connections");
            // Connection auth for resumer
            push_opt_string(&mut kv, resumer, "username", "username");
            push_opt_string(&mut kv, resumer, "password", "password");
        }
        _ => {}
    }

    ("resumer".into(), kv)
}

fn render_runtime(runtime: &serde_json::Value) -> (String, Vec<(String, String)>) {
    let mut kv = Vec::new();

    let log_level = runtime
        .get("log_level")
        .and_then(|v| v.as_str())
        .unwrap_or("info");
    let log_dir = runtime
        .get("log_dir")
        .and_then(|v| v.as_str())
        .unwrap_or("./logs");
    let log4rs_file = runtime
        .get("log4rs_file")
        .and_then(|v| v.as_str())
        .unwrap_or("./log4rs.yaml");

    kv.push(("log_level".into(), log_level.into()));
    kv.push(("log4rs_file".into(), log4rs_file.into()));
    kv.push(("log_dir".into(), log_dir.into()));

    push_opt_usize(&mut kv, runtime, "tb_parallel_size", "tb_parallel_size");

    ("runtime".into(), kv)
}

fn render_processor(processor: &serde_json::Value) -> (String, Vec<(String, String)>) {
    let mut kv = Vec::new();
    push_opt_string(&mut kv, processor, "lua_code_file", "lua_code_file");
    ("processor".into(), kv)
}

#[cfg(feature = "metrics")]
fn render_metrics(metrics: &serde_json::Value) -> (String, Vec<(String, String)>) {
    let mut kv = Vec::new();

    let http_host = metrics
        .get("http_host")
        .and_then(|v| v.as_str())
        .unwrap_or("0.0.0.0");
    let http_port = metrics
        .get("http_port")
        .and_then(|v| v.as_u64())
        .unwrap_or(9090);
    let workers = metrics.get("workers").and_then(|v| v.as_u64()).unwrap_or(2);

    kv.push(("http_host".into(), http_host.into()));
    kv.push(("http_port".into(), http_port.to_string()));
    kv.push(("workers".into(), workers.to_string()));

    // Labels: k1=v1,k2=v2
    if let Some(labels) = metrics.get("metrics_labels") {
        if let Some(map) = labels.as_object() {
            if !map.is_empty() {
                // Deterministic order by sorted keys
                let mut pairs: Vec<(String, String)> = map
                    .iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                    .collect();
                pairs.sort_by(|a, b| a.0.cmp(&b.0));
                let labels_str = pairs
                    .iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join(",");
                kv.push(("labels".into(), labels_str));
            }
        }
    }

    ("metrics".into(), kv)
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn push_opt_string(
    kv: &mut Vec<(String, String)>,
    json: &serde_json::Value,
    json_key: &str,
    ini_key: &str,
) {
    if let Some(val) = json.get(json_key).and_then(|v| v.as_str()) {
        if !val.is_empty() {
            kv.push((ini_key.into(), val.into()));
        }
    }
}

fn push_req_string(
    kv: &mut Vec<(String, String)>,
    json: &serde_json::Value,
    json_key: &str,
    ini_key: &str,
) {
    let val = json.get(json_key).and_then(|v| v.as_str()).unwrap_or("");
    kv.push((ini_key.into(), val.into()));
}

fn push_opt_usize(
    kv: &mut Vec<(String, String)>,
    json: &serde_json::Value,
    json_key: &str,
    ini_key: &str,
) {
    if let Some(val) = json.get(json_key).and_then(|v| v.as_u64()) {
        kv.push((ini_key.into(), val.to_string()));
    }
}

fn push_opt_u32(
    kv: &mut Vec<(String, String)>,
    json: &serde_json::Value,
    json_key: &str,
    ini_key: &str,
) {
    if let Some(val) = json.get(json_key).and_then(|v| v.as_u64()) {
        kv.push((ini_key.into(), val.to_string()));
    }
}

fn push_opt_u64(
    kv: &mut Vec<(String, String)>,
    json: &serde_json::Value,
    json_key: &str,
    ini_key: &str,
) {
    if let Some(val) = json.get(json_key).and_then(|v| v.as_u64()) {
        kv.push((ini_key.into(), val.to_string()));
    }
}

fn push_opt_bool(
    kv: &mut Vec<(String, String)>,
    json: &serde_json::Value,
    json_key: &str,
    ini_key: &str,
) {
    if let Some(val) = json.get(json_key).and_then(|v| v.as_bool()) {
        kv.push((ini_key.into(), val.to_string()));
    }
}

/// Ensure a key is present in the kv list (emit empty value if missing).
fn ensure_key_present(kv: &mut Vec<(String, String)>, key: &str) {
    if !kv.iter().any(|(k, _)| k == key) {
        kv.push((key.into(), String::new()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Task;

    fn make_task(
        kind: &str,
        db_type_source: &str,
        db_type_target: &str,
        extractor_config: &str,
        sinker_config: &str,
    ) -> Task {
        Task {
            id: "test-id".into(),
            task_id: format!("{kind}_{db_type_source}_{db_type_target}_abcd1234"),
            name: "test task".into(),
            kind: kind.into(),
            db_type_source: db_type_source.into(),
            db_type_target: db_type_target.into(),
            source_endpoint: r#"{"url":"mysql://src:3306/db"}"#.into(),
            target_endpoint: r#"{"url":"mysql://dst:3306/db"}"#.into(),
            extractor_config: extractor_config.into(),
            sinker_config: sinker_config.into(),
            filter_config: r#"{"do_dbs":"","ignore_dbs":"","do_tbs":"test_db.*","ignore_tbs":""}"#
                .into(),
            router_config: r#"{"db_map":"","tb_map":"","col_map":""}"#.into(),
            parallelizer_config: r#"{"parallel_type":"snapshot","parallel_size":2}"#.into(),
            pipeline_config: r#"{"buffer_size":4,"checkpoint_interval_secs":1}"#.into(),
            resumer_config: "{}".into(),
            processor_config: "{}".into(),
            runtime_config:
                r#"{"log_level":"info","log_dir":"./logs","log4rs_file":"./log4rs.yaml"}"#.into(),
            metrics_config: "{}".into(),
            resource_group_id: "default-rg".into(),
            owner_user_id: Some("admin".into()),
            status: "draft".into(),
            created_at: "2025-01-01T00:00:00.000Z".into(),
            updated_at: "2025-01-01T00:00:00.000Z".into(),
        }
    }

    #[test]
    fn test_snapshot_mysql_to_mysql_has_required_sections() {
        let task = make_task(
            "snapshot",
            "mysql",
            "mysql",
            r#"{"extractType":"snapshot","url":"mysql://src:3306/db","username":"root","password":"pass"}"#,
            r#"{"sinkType":"write","url":"mysql://dst:3306/db","username":"root","password":"pass","batch_size":2,"replace":true,"disable_foreign_key_checks":true,"transaction_isolation":"read_committed"}"#,
        );
        let ini = render(&task);
        assert!(ini.contains("[global]"));
        assert!(ini.contains("[extractor]"));
        assert!(ini.contains("[sinker]"));
        assert!(ini.contains("[filter]"));
        assert!(ini.contains("[router]"));
        assert!(ini.contains("[parallelizer]"));
        assert!(ini.contains("[pipeline]"));
        assert!(ini.contains("[runtime]"));
        assert!(
            !ini.contains("[processor]"),
            "empty processor should be omitted"
        );
        assert!(
            !ini.contains("[data_marker]"),
            "empty data_marker should be omitted"
        );
    }

    #[test]
    fn test_deterministic_rendering() {
        let task = make_task(
            "snapshot",
            "mysql",
            "mysql",
            r#"{"extractType":"snapshot","url":"mysql://src:3306/db"}"#,
            r#"{"sinkType":"write","url":"mysql://dst:3306/db"}"#,
        );
        let first = render(&task);
        let second = render(&task);
        assert_eq!(first, second, "rendering must be deterministic");
        assert_eq!(first.as_bytes(), second.as_bytes(), "byte-identical output");
    }

    #[test]
    fn test_empty_optional_sections_omitted() {
        let task = make_task(
            "snapshot",
            "mysql",
            "mysql",
            r#"{"extractType":"snapshot","url":"mysql://src:3306/db"}"#,
            r#"{"sinkType":"write","url":"mysql://dst:3306/db"}"#,
        );
        let ini = render(&task);
        assert!(
            !ini.contains("[processor]"),
            "empty processor section must be omitted"
        );
        assert!(
            !ini.contains("[data_marker]"),
            "empty data_marker section must be omitted"
        );
        assert!(
            !ini.contains("[metacenter]"),
            "empty metacenter section must be omitted"
        );
    }

    #[test]
    fn test_processor_section_emitted_when_configured() {
        let mut task = make_task(
            "snapshot",
            "mysql",
            "mysql",
            r#"{"extractType":"snapshot","url":"mysql://src:3306/db"}"#,
            r#"{"sinkType":"write","url":"mysql://dst:3306/db"}"#,
        );
        task.processor_config = r#"{"lua_code_file":"./transform.lua"}"#.into();
        let ini = render(&task);
        assert!(ini.contains("[processor]"));
        assert!(ini.contains("lua_code_file=./transform.lua"));
    }

    #[test]
    fn test_extractor_snapshot_fields() {
        let task = make_task(
            "snapshot",
            "mysql",
            "mysql",
            r#"{"extractType":"snapshot","url":"mysql://src:3306/db","batch_size":100,"parallel_size":2,"sample_interval":1}"#,
            r#"{"sinkType":"write","url":"mysql://dst:3306/db","batch_size":2}"#,
        );
        let ini = render(&task);
        assert!(ini.contains("db_type=mysql"));
        assert!(ini.contains("extract_type=snapshot"));
        assert!(ini.contains("url=mysql://src:3306/db"));
    }

    #[test]
    fn test_extractor_cdc_mysql_fields() {
        let task = make_task(
            "cdc",
            "mysql",
            "mysql",
            r#"{"extractType":"cdc","url":"mysql://src:3306/db","server_id":2000,"binlog_filename":"","binlog_position":0,"heartbeat_interval_secs":1,"heartbeat_tb":"heartbeat_db.ape_dts_heartbeat"}"#,
            r#"{"sinkType":"write","url":"mysql://dst:3306/db","batch_size":2}"#,
        );
        let ini = render(&task);
        assert!(ini.contains("extract_type=cdc"));
        assert!(ini.contains("server_id=2000"));
        assert!(ini.contains("binlog_position=0"));
        assert!(ini.contains("heartbeat_interval_secs=1"));
        assert!(ini.contains("heartbeat_tb=heartbeat_db.ape_dts_heartbeat"));
    }

    #[test]
    fn test_extractor_cdc_pg_fields() {
        let task = make_task(
            "cdc",
            "pg",
            "pg",
            r#"{"extractType":"cdc","url":"postgres://src:5432/db","slot_name":"ape_test","pub_name":"","start_lsn":"","recreate_slot_if_exists":true,"heartbeat_interval_secs":10}"#,
            r#"{"sinkType":"write","url":"postgres://dst:5432/db","batch_size":2}"#,
        );
        let ini = render(&task);
        assert!(ini.contains("db_type=pg"));
        assert!(ini.contains("slot_name=ape_test"));
        assert!(ini.contains("recreate_slot_if_exists=true"));
    }

    #[test]
    fn test_extractor_cdc_oracle_logminer_fields() {
        let task = make_task(
            "cdc",
            "oracle",
            "oracle",
            r#"{"extractType":"cdc","url":"oracle://src:1521/db","cdc_mode":"logminer","poll_interval_millis":200,"poll_batch_size":200,"start_scn":0,"start_time_utc":"","end_time_utc":""}"#,
            r#"{"sinkType":"write","url":"oracle://dst:1521/db","batch_size":2}"#,
        );
        let ini = render(&task);
        assert!(ini.contains("db_type=oracle"));
        assert!(ini.contains("cdc_mode=logminer"));
        assert!(ini.contains("poll_interval_millis=200"));
        assert!(ini.contains("start_scn=0"));
    }

    #[test]
    fn test_check_mysql_to_mysql() {
        let task = make_task(
            "check",
            "mysql",
            "mysql",
            r#"{"extractType":"snapshot","url":"mysql://src:3306/db"}"#,
            r#"{"sinkType":"check","url":"mysql://dst:3306/db","batch_size":2,"check_log_dir":"./check"}"#,
        );
        let ini = render(&task);
        assert!(ini.contains("sink_type=check"));
        assert!(ini.contains("check_log_dir=./check"));
    }

    #[test]
    fn test_struct_mysql_to_mysql() {
        let task = make_task(
            "struct",
            "mysql",
            "mysql",
            r#"{"extractType":"struct","url":"mysql://src:3306/db"}"#,
            r#"{"sinkType":"struct","url":"mysql://dst:3306/db","conflict_policy":"interrupt"}"#,
        );
        let ini = render(&task);
        assert!(ini.contains("extract_type=struct"));
        assert!(ini.contains("sink_type=struct"));
        assert!(ini.contains("conflict_policy=interrupt"));
    }

    #[test]
    fn test_snapshot_gaussdb_pg_to_mysql() {
        let task = make_task(
            "snapshot",
            "gaussdb_pg",
            "mysql",
            r#"{"extractType":"snapshot","url":"postgres://gaussdb:8000/db","username":"root","password":"pass"}"#,
            r#"{"sinkType":"write","url":"mysql://dst:3306/db","username":"root","password":"pass","batch_size":2,"replace":true,"disable_foreign_key_checks":true}"#,
        );
        let ini = render(&task);
        assert!(ini.contains("db_type=gaussdb_pg"));
        assert!(ini.contains("extract_type=snapshot"));
    }

    #[test]
    fn test_cdc_gaussdb_mysql_to_mysql() {
        let task = make_task(
            "cdc",
            "gaussdb_mysql",
            "mysql",
            r#"{"extractType":"cdc","url":"mysql://gaussdb:3311/db","server_id":2000,"heartbeat_interval_secs":1}"#,
            r#"{"sinkType":"write","url":"mysql://dst:3306/db","batch_size":2,"replace":true,"disable_foreign_key_checks":true}"#,
        );
        let ini = render(&task);
        assert!(ini.contains("db_type=gaussdb_mysql"));
        assert!(ini.contains("extract_type=cdc"));
        assert!(ini.contains("server_id=2000"));
    }

    #[test]
    fn test_cdc_gaussdb_oracle_to_oracle() {
        let task = make_task(
            "cdc",
            "gaussdb_oracle",
            "oracle",
            r#"{"extractType":"cdc","url":"postgres://gaussdb:8000/db","slot_name":"ape_test_gaussdb_oracle","start_lsn":"","recreate_slot_if_exists":false,"keepalive_interval_secs":10,"heartbeat_interval_secs":0,"heartbeat_tb":"heartbeat_db.ape_dts_heartbeat","start_time_utc":"","end_time_utc":""}"#,
            r#"{"sinkType":"write","url":"oracle://dst:1521/db","username":"system","password":"pass","batch_size":2}"#,
        );
        let ini = render(&task);
        assert!(ini.contains("db_type=gaussdb_oracle"));
        assert!(ini.contains("slot_name=ape_test_gaussdb_oracle"));
        assert!(ini.contains("keepalive_interval_secs=10"));
    }

    #[test]
    fn test_snapshot_mysql_to_kafka() {
        let mut task = make_task(
            "snapshot",
            "mysql",
            "kafka",
            r#"{"extractType":"snapshot","url":"mysql://src:3306/db"}"#,
            r#"{"sinkType":"write","url":"kafka://kafka:9092","batch_size":2}"#,
        );
        task.db_type_target = "kafka".into();
        task.filter_config =
            r#"{"do_dbs":"","ignore_dbs":"","do_tbs":"test_db_1.*,test_db_2.*","ignore_tbs":"","do_events":"insert,update,delete"}"#
                .into();
        task.router_config =
            r#"{"db_map":"","tb_map":"","col_map":"","topic_map":"*.*:test,test_db_1.*:test2"}"#
                .into();
        let ini = render(&task);
        assert!(ini.contains("db_type=kafka"));
        assert!(ini.contains("sink_type=write"));
        assert!(ini.contains("topic_map="));
    }

    #[test]
    fn test_global_task_id_rendered() {
        let task = make_task(
            "snapshot",
            "mysql",
            "mysql",
            r#"{"extractType":"snapshot","url":"mysql://src:3306/db"}"#,
            r#"{"sinkType":"write","url":"mysql://dst:3306/db"}"#,
        );
        let ini = render(&task);
        assert!(ini.contains("[global]"));
        assert!(ini.contains(&format!("task_id={}", task.task_id)));
    }

    #[test]
    fn test_snapshot_pg_to_pg() {
        let task = make_task(
            "snapshot",
            "pg",
            "pg",
            r#"{"extractType":"snapshot","url":"postgres://src:5432/db","username":"root","password":"pass"}"#,
            r#"{"sinkType":"write","url":"postgres://dst:5432/db","username":"root","password":"pass","batch_size":2}"#,
        );
        let ini = render(&task);
        assert!(ini.contains("db_type=pg"));
        assert!(ini.contains("extract_type=snapshot"));
        assert!(ini.contains("url=postgres://src:5432/db"));
    }

    #[test]
    fn test_filter_do_structures_for_struct() {
        let mut task = make_task(
            "struct",
            "mysql",
            "mysql",
            r#"{"extractType":"struct","url":"mysql://src:3306/db"}"#,
            r#"{"sinkType":"struct","url":"mysql://dst:3306/db","conflict_policy":"interrupt"}"#,
        );
        task.filter_config =
            r#"{"do_dbs":"struct_it_mysql2mysql_1","ignore_dbs":"","do_tbs":"","ignore_tbs":"","do_events":"","do_structures":"table,index,constraint"}"#
                .into();
        let ini = render(&task);
        assert!(ini.contains("do_structures=table,index,constraint"));
    }

    #[test]
    fn test_resumer_from_log_section() {
        let mut task = make_task(
            "snapshot",
            "mysql",
            "mysql",
            r#"{"extractType":"snapshot","url":"mysql://src:3306/db"}"#,
            r#"{"sinkType":"write","url":"mysql://dst:3306/db"}"#,
        );
        task.resumer_config =
            r#"{"resume_type":"from_log","log_dir":"./logs","config_file":""}"#.into();
        let ini = render(&task);
        assert!(ini.contains("[resumer]"));
        assert!(ini.contains("resume_type=from_log"));
    }

    #[test]
    fn test_resumer_dummy_omitted() {
        let task = make_task(
            "snapshot",
            "mysql",
            "mysql",
            r#"{"extractType":"snapshot","url":"mysql://src:3306/db"}"#,
            r#"{"sinkType":"write","url":"mysql://dst:3306/db"}"#,
        );
        let ini = render(&task);
        assert!(
            !ini.contains("[resumer]"),
            "dummy resumer should be omitted"
        );
    }
}
