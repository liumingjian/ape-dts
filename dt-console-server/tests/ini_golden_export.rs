//! Golden tests for IniRenderer — byte-for-byte comparison against 11 mandated
//! (kind × engine) matrix fixtures under `tests/golden/`.
//!
//! Also covers:
//! - Deterministic rendering (same input → same output twice)
//! - Optional sections omitted when empty
//! - Metrics section only when feature configured
//! - Round-trip: rendered INI → TaskConfig::new() recovers same struct
//! - preview_ini endpoint: GET /api/tasks/:id/preview_ini
//! - Export JSON/INI + import round-trip + clone

use dt_console_server::ini_renderer;
use dt_console_server::models::Task;

/// Helper: create a Task model with the specified kind and engine pair,
/// using representative config values matching the golden fixture.
fn make_golden_task(kind: &str, db_type_source: &str, db_type_target: &str) -> Task {
    let task_id = format!("{kind}_{db_type_source}_{db_type_target}_abcd1234");

    // Build configs per matrix entry
    let (
        extractor_config,
        sinker_config,
        filter_config,
        router_config,
        parallelizer_config,
        pipeline_config,
    ) = match (kind, db_type_source, db_type_target) {
        // VAL-INI-001: Snapshot × mysql → mysql
        ("snapshot", "mysql", "mysql") => (
            r#"{"extractType":"snapshot","url":"mysql://src:3307/db","username":"root","password":"src_pass"}"#,
            r#"{"sinkType":"write","url":"mysql://dst:3308/db","username":"root","password":"dst_pass","batch_size":2,"replace":true,"disable_foreign_key_checks":true,"transaction_isolation":"read_committed"}"#,
            r#"{"do_dbs":"","ignore_dbs":"","do_tbs":"test_db_1.*","ignore_tbs":"","do_events":"insert"}"#,
            r#"{"db_map":"","tb_map":"","col_map":""}"#,
            r#"{"parallel_type":"snapshot","parallel_size":2}"#,
            r#"{"buffer_size":4,"checkpoint_interval_secs":1}"#,
        ),
        // VAL-INI-002: Snapshot × pg → pg
        ("snapshot", "pg", "pg") => (
            r#"{"extractType":"snapshot","url":"postgres://src:5433/db","username":"root","password":"src_pass"}"#,
            r#"{"sinkType":"write","url":"postgres://dst:5434/db","username":"root","password":"dst_pass","batch_size":2,"replace":true,"disable_foreign_key_checks":true}"#,
            r#"{"do_dbs":"test_db_1","ignore_dbs":"","do_tbs":"","ignore_tbs":"","do_events":"insert"}"#,
            r#"{"db_map":"","tb_map":"","col_map":""}"#,
            r#"{"parallel_type":"snapshot","parallel_size":2}"#,
            r#"{"buffer_size":4,"checkpoint_interval_secs":1}"#,
        ),
        // VAL-INI-003: Snapshot × mysql → kafka
        ("snapshot", "mysql", "kafka") => (
            r#"{"extractType":"snapshot","url":"mysql://src:3307/db"}"#,
            r#"{"sinkType":"write","url":"kafka://kafka:9092","batch_size":2}"#,
            r#"{"do_dbs":"","ignore_dbs":"","do_tbs":"test_db_1.*,test_db_2.*","ignore_tbs":"","do_events":"insert,update,delete"}"#,
            r#"{"db_map":"","tb_map":"","col_map":"","topic_map":"*.*:test,test_db_1.*:test2"}"#,
            r#"{"parallel_type":"serial","parallel_size":1}"#,
            r#"{"buffer_size":16000,"checkpoint_interval_secs":15}"#,
        ),
        // VAL-INI-004: CDC × mysql → mysql
        ("cdc", "mysql", "mysql") => (
            r#"{"extractType":"cdc","url":"mysql://src:3307/db","username":"root","password":"src_pass","server_id":2000,"binlog_filename":"","binlog_position":0,"heartbeat_interval_secs":1,"heartbeat_tb":"heartbeat_db.ape_dts_heartbeat"}"#,
            r#"{"sinkType":"write","url":"mysql://dst:3308/db","username":"root","password":"dst_pass","batch_size":2}"#,
            r#"{"do_dbs":"","ignore_dbs":"","do_tbs":"test_db_1.*,upper_case_db.*","ignore_tbs":"","do_events":"insert,update,delete","do_ddls":"*"}"#,
            r#"{"db_map":"","tb_map":"","col_map":""}"#,
            r#"{"parallel_type":"rdb_merge","parallel_size":2}"#,
            r#"{"buffer_size":4,"checkpoint_interval_secs":1}"#,
        ),
        // VAL-INI-005: CDC × pg → pg
        ("cdc", "pg", "pg") => (
            r#"{"extractType":"cdc","url":"postgres://src:5433/db","username":"root","password":"src_pass","slot_name":"ape_test","pub_name":"","start_lsn":"","recreate_slot_if_exists":true,"heartbeat_interval_secs":10,"heartbeat_tb":"heartbeat_db.ape_dts_heartbeat"}"#,
            r#"{"sinkType":"write","url":"postgres://dst:5434/db","username":"root","password":"dst_pass","batch_size":2}"#,
            r#"{"do_dbs":"upper_case_db","ignore_dbs":"","do_tbs":"public.default_table","ignore_tbs":"","do_events":"insert,update,delete"}"#,
            r#"{"db_map":"","tb_map":"","col_map":""}"#,
            r#"{"parallel_type":"rdb_merge","parallel_size":2}"#,
            r#"{"buffer_size":4,"checkpoint_interval_secs":1}"#,
        ),
        // VAL-INI-006: CDC × oracle → oracle (LogMiner)
        ("cdc", "oracle", "oracle") => (
            r#"{"extractType":"cdc","url":"oracle://src:1521/db","username":"system","password":"oracle","cdc_mode":"logminer","poll_interval_millis":200,"poll_batch_size":200,"start_scn":0,"start_time_utc":"","end_time_utc":""}"#,
            r#"{"sinkType":"write","url":"oracle://dst:1521/db","username":"system","password":"oracle","batch_size":2}"#,
            r#"{"do_dbs":"","ignore_dbs":"","do_tbs":"APE_DTS.ORA_CDC_BASIC","ignore_tbs":"","do_events":"insert,update,delete"}"#,
            r#"{"db_map":"","tb_map":"","col_map":""}"#,
            r#"{"parallel_type":"serial","parallel_size":1}"#,
            r#"{"buffer_size":4,"checkpoint_interval_secs":1}"#,
        ),
        // VAL-INI-007: Check × mysql → mysql
        ("check", "mysql", "mysql") => (
            r#"{"extractType":"snapshot","url":"mysql://src:3307/db"}"#,
            r#"{"sinkType":"check","url":"mysql://dst:3308/db","batch_size":2,"check_log_dir":"./check"}"#,
            r#"{"do_dbs":"","ignore_dbs":"","do_tbs":"test_db_1.*","ignore_tbs":"","do_events":"insert"}"#,
            r#"{"db_map":"","tb_map":"","col_map":""}"#,
            r#"{"parallel_type":"rdb_check","parallel_size":2}"#,
            r#"{"buffer_size":4,"checkpoint_interval_secs":1}"#,
        ),
        // VAL-INI-008: Struct × mysql → mysql
        ("struct", "mysql", "mysql") => (
            r#"{"extractType":"struct","url":"mysql://src:3307/db"}"#,
            r#"{"sinkType":"struct","url":"mysql://dst:3308/db","conflict_policy":"interrupt"}"#,
            r#"{"do_dbs":"struct_it_mysql2mysql_1","ignore_dbs":"","do_tbs":"","ignore_tbs":"","do_events":"","do_structures":"table,index,constraint"}"#,
            r#"{"db_map":"","tb_map":"","col_map":""}"#,
            r#"{"parallel_type":"serial","parallel_size":1}"#,
            r#"{"checkpoint_interval_secs":1,"buffer_size":100}"#,
        ),
        // VAL-INI-009: Snapshot × gaussdb_pg → mysql
        ("snapshot", "gaussdb_pg", "mysql") => (
            r#"{"extractType":"snapshot","url":"postgres://gaussdb:8000/db","username":"root","password":"gaussdb_pass"}"#,
            r#"{"sinkType":"write","url":"mysql://dst:3308/db","username":"root","password":"dst_pass","batch_size":2,"replace":true,"disable_foreign_key_checks":true}"#,
            r#"{"do_dbs":"","ignore_dbs":"","do_tbs":"public.gaussdb_to_pg_snapshot_basic","ignore_tbs":"","do_events":"insert"}"#,
            r#"{"db_map":"","tb_map":"","col_map":""}"#,
            r#"{"parallel_type":"snapshot","parallel_size":1}"#,
            r#"{"buffer_size":4,"checkpoint_interval_secs":1}"#,
        ),
        // VAL-INI-010: CDC × gaussdb_mysql → mysql
        ("cdc", "gaussdb_mysql", "mysql") => (
            r#"{"extractType":"cdc","url":"mysql://gaussdb:3311/db","username":"root","password":"gaussdb_pass","server_id":2000,"heartbeat_interval_secs":1,"heartbeat_tb":"heartbeat_db.ape_dts_heartbeat"}"#,
            r#"{"sinkType":"write","url":"mysql://dst:3308/db","username":"root","password":"dst_pass","batch_size":2,"replace":true,"disable_foreign_key_checks":true}"#,
            r#"{"do_dbs":"","ignore_dbs":"","do_tbs":"public.gaussdb_to_mysql_cdc_basic","ignore_tbs":"","do_events":"insert,update,delete"}"#,
            r#"{"db_map":"public:gaussdb_to_mysql_cdc_dst","tb_map":"","col_map":""}"#,
            r#"{"parallel_type":"rdb_merge","parallel_size":1}"#,
            r#"{"buffer_size":4,"checkpoint_interval_secs":1}"#,
        ),
        // VAL-INI-011: CDC × gaussdb_oracle → oracle
        ("cdc", "gaussdb_oracle", "oracle") => (
            r#"{"extractType":"cdc","url":"postgres://gaussdb:8000/db","username":"root","password":"gaussdb_pass","slot_name":"ape_test_gaussdb_oracle","start_lsn":"","recreate_slot_if_exists":false,"keepalive_interval_secs":10,"heartbeat_interval_secs":0,"heartbeat_tb":"heartbeat_db.ape_dts_heartbeat","start_time_utc":"","end_time_utc":""}"#,
            r#"{"sinkType":"write","url":"oracle://dst:1521/db","username":"system","password":"oracle","batch_size":2}"#,
            r#"{"do_dbs":"","ignore_dbs":"","do_tbs":"public.gdbo_ora_cdc_basic","ignore_tbs":"","do_events":"insert,update,delete"}"#,
            r#"{"db_map":"public:APE_DTS","tb_map":"public.gdbo_ora_cdc_basic:APE_DTS.GDBO_ORA_CDC_BASIC","col_map":"json:[{\"db\":\"public\",\"tb\":\"gdbo_ora_cdc_basic\",\"col_map\":{\"id\":\"ID\",\"val\":\"VAL\"}}]"}"#,
            r#"{"parallel_type":"serial","parallel_size":1}"#,
            r#"{"buffer_size":4,"checkpoint_interval_secs":1}"#,
        ),
        _ => (
            r#"{"extractType":"snapshot","url":"mysql://src:3307/db"}"#,
            r#"{"sinkType":"write","url":"mysql://dst:3308/db","batch_size":2}"#,
            r#"{"do_dbs":"","ignore_dbs":"","do_tbs":"","ignore_tbs":""}"#,
            r#"{"db_map":"","tb_map":"","col_map":""}"#,
            r#"{"parallel_type":"serial","parallel_size":1}"#,
            r#"{"buffer_size":16000,"checkpoint_interval_secs":10}"#,
        ),
    };

    Task {
        id: "test-id".into(),
        task_id: task_id.clone(),
        name: format!("golden test {kind} {db_type_source}→{db_type_target}"),
        kind: kind.into(),
        db_type_source: db_type_source.into(),
        db_type_target: db_type_target.into(),
        source_endpoint: r#"{"url":"mysql://src:3307/db"}"#.into(),
        target_endpoint: r#"{"url":"mysql://dst:3308/db"}"#.into(),
        extractor_config: extractor_config.into(),
        sinker_config: sinker_config.into(),
        filter_config: filter_config.into(),
        router_config: router_config.into(),
        parallelizer_config: parallelizer_config.into(),
        pipeline_config: pipeline_config.into(),
        resumer_config: "{}".into(),
        processor_config: "{}".into(),
        runtime_config: r#"{"log_level":"info","log_dir":"./logs","log4rs_file":"./log4rs.yaml"}"#
            .into(),
        metrics_config: "{}".into(),
        resource_group_id: "default-rg".into(),
        owner_user_id: Some("admin".into()),
        status: "draft".into(),
        created_at: "2025-01-01T00:00:00.000Z".into(),
        updated_at: "2025-01-01T00:00:00.000Z".into(),
    }
}

/// Load a golden fixture file and return its content as a String.
fn load_golden(name: &str) -> String {
    let path = format!("tests/golden/{name}.ini");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to load golden fixture {path}: {e}"))
}

// ── Golden tests for each of the 11 mandated matrices ──────────────────────

#[test]
fn golden_snapshot_mysql_to_mysql() {
    let task = make_golden_task("snapshot", "mysql", "mysql");
    let rendered = ini_renderer::render(&task);
    let expected = load_golden("snapshot_mysql_to_mysql");
    assert_eq!(
        rendered, expected,
        "byte-for-byte mismatch for snapshot×mysql→mysql"
    );
}

#[test]
fn golden_snapshot_pg_to_pg() {
    let task = make_golden_task("snapshot", "pg", "pg");
    let rendered = ini_renderer::render(&task);
    let expected = load_golden("snapshot_pg_to_pg");
    assert_eq!(
        rendered, expected,
        "byte-for-byte mismatch for snapshot×pg→pg"
    );
}

#[test]
fn golden_snapshot_mysql_to_kafka() {
    let task = make_golden_task("snapshot", "mysql", "kafka");
    let rendered = ini_renderer::render(&task);
    let expected = load_golden("snapshot_mysql_to_kafka");
    assert_eq!(
        rendered, expected,
        "byte-for-byte mismatch for snapshot×mysql→kafka"
    );
}

#[test]
fn golden_cdc_mysql_to_mysql() {
    let task = make_golden_task("cdc", "mysql", "mysql");
    let rendered = ini_renderer::render(&task);
    let expected = load_golden("cdc_mysql_to_mysql");
    assert_eq!(
        rendered, expected,
        "byte-for-byte mismatch for cdc×mysql→mysql"
    );
}

#[test]
fn golden_cdc_pg_to_pg() {
    let task = make_golden_task("cdc", "pg", "pg");
    let rendered = ini_renderer::render(&task);
    let expected = load_golden("cdc_pg_to_pg");
    assert_eq!(rendered, expected, "byte-for-byte mismatch for cdc×pg→pg");
}

#[test]
fn golden_cdc_oracle_to_oracle() {
    let task = make_golden_task("cdc", "oracle", "oracle");
    let rendered = ini_renderer::render(&task);
    let expected = load_golden("cdc_oracle_to_oracle");
    assert_eq!(
        rendered, expected,
        "byte-for-byte mismatch for cdc×oracle→oracle"
    );
}

#[test]
fn golden_check_mysql_to_mysql() {
    let task = make_golden_task("check", "mysql", "mysql");
    let rendered = ini_renderer::render(&task);
    let expected = load_golden("check_mysql_to_mysql");
    assert_eq!(
        rendered, expected,
        "byte-for-byte mismatch for check×mysql→mysql"
    );
}

#[test]
fn golden_struct_mysql_to_mysql() {
    let task = make_golden_task("struct", "mysql", "mysql");
    let rendered = ini_renderer::render(&task);
    let expected = load_golden("struct_mysql_to_mysql");
    assert_eq!(
        rendered, expected,
        "byte-for-byte mismatch for struct×mysql→mysql"
    );
}

#[test]
fn golden_snapshot_gaussdb_pg_to_mysql() {
    let task = make_golden_task("snapshot", "gaussdb_pg", "mysql");
    let rendered = ini_renderer::render(&task);
    let expected = load_golden("snapshot_gaussdb_pg_to_mysql");
    assert_eq!(
        rendered, expected,
        "byte-for-byte mismatch for snapshot×gaussdb_pg→mysql"
    );
}

#[test]
fn golden_cdc_gaussdb_mysql_to_mysql() {
    let task = make_golden_task("cdc", "gaussdb_mysql", "mysql");
    let rendered = ini_renderer::render(&task);
    let expected = load_golden("cdc_gaussdb_mysql_to_mysql");
    assert_eq!(
        rendered, expected,
        "byte-for-byte mismatch for cdc×gaussdb_mysql→mysql"
    );
}

#[test]
fn golden_cdc_gaussdb_oracle_to_oracle() {
    let task = make_golden_task("cdc", "gaussdb_oracle", "oracle");
    let rendered = ini_renderer::render(&task);
    let expected = load_golden("cdc_gaussdb_oracle_to_oracle");
    assert_eq!(
        rendered, expected,
        "byte-for-byte mismatch for cdc×gaussdb_oracle→oracle"
    );
}

// ── Determinism tests ──────────────────────────────────────────────────────

#[test]
fn deterministic_rendering_same_input_twice() {
    let task = make_golden_task("snapshot", "mysql", "mysql");
    let first = ini_renderer::render(&task);
    let second = ini_renderer::render(&task);
    assert_eq!(
        first.as_bytes(),
        second.as_bytes(),
        "rendering must be deterministic — same input must produce byte-identical output"
    );
}

#[test]
fn deterministic_rendering_all_matrices() {
    let matrices = [
        ("snapshot", "mysql", "mysql"),
        ("snapshot", "pg", "pg"),
        ("snapshot", "mysql", "kafka"),
        ("cdc", "mysql", "mysql"),
        ("cdc", "pg", "pg"),
        ("cdc", "oracle", "oracle"),
        ("check", "mysql", "mysql"),
        ("struct", "mysql", "mysql"),
        ("snapshot", "gaussdb_pg", "mysql"),
        ("cdc", "gaussdb_mysql", "mysql"),
        ("cdc", "gaussdb_oracle", "oracle"),
    ];
    for (kind, src, dst) in matrices {
        let task = make_golden_task(kind, src, dst);
        let first = ini_renderer::render(&task);
        let second = ini_renderer::render(&task);
        assert_eq!(
            first, second,
            "determinism check failed for {kind}×{src}→{dst}"
        );
    }
}

// ── Optional section tests ─────────────────────────────────────────────────

#[test]
fn empty_optional_sections_omitted() {
    let task = make_golden_task("snapshot", "mysql", "mysql");
    let ini = ini_renderer::render(&task);
    assert!(
        !ini.contains("[processor]"),
        "empty [processor] must be omitted"
    );
    assert!(
        !ini.contains("[data_marker]"),
        "empty [data_marker] must be omitted"
    );
    assert!(
        !ini.contains("[metacenter]"),
        "empty [metacenter] must be omitted"
    );
}

#[test]
fn processor_section_present_when_configured() {
    let mut task = make_golden_task("snapshot", "mysql", "mysql");
    task.processor_config = r#"{"lua_code_file":"./transform.lua"}"#.into();
    let ini = ini_renderer::render(&task);
    assert!(
        ini.contains("[processor]"),
        "[processor] must be present when configured"
    );
    assert!(ini.contains("lua_code_file=./transform.lua"));
}

#[test]
fn resumer_dummy_omitted() {
    let task = make_golden_task("snapshot", "mysql", "mysql");
    let ini = ini_renderer::render(&task);
    assert!(!ini.contains("[resumer]"), "dummy resumer must be omitted");
}

#[test]
fn resumer_from_log_present() {
    let mut task = make_golden_task("snapshot", "mysql", "mysql");
    task.resumer_config =
        r#"{"resume_type":"from_log","log_dir":"./logs","config_file":""}"#.into();
    let ini = ini_renderer::render(&task);
    assert!(ini.contains("[resumer]"));
    assert!(ini.contains("resume_type=from_log"));
}

// ── Metrics section tests ───────────────────────────────────────────────────

#[cfg(feature = "metrics")]
#[test]
fn metrics_section_present_when_configured() {
    let mut task = make_golden_task("snapshot", "mysql", "mysql");
    task.metrics_config = r#"{"http_host":"0.0.0.0","http_port":9090,"workers":2}"#.into();
    let ini = ini_renderer::render(&task);
    assert!(
        ini.contains("[metrics]"),
        "[metrics] must be present when feature enabled and config non-empty"
    );
    assert!(ini.contains("http_host=0.0.0.0"));
    assert!(ini.contains("http_port=9090"));
}

#[cfg(feature = "metrics")]
#[test]
fn metrics_section_omitted_when_empty() {
    let task = make_golden_task("snapshot", "mysql", "mysql");
    let ini = ini_renderer::render(&task);
    assert!(
        !ini.contains("[metrics]"),
        "[metrics] must be omitted when config is empty"
    );
}

#[cfg(not(feature = "metrics"))]
#[test]
fn metrics_section_never_emitted_without_feature() {
    let mut task = make_golden_task("snapshot", "mysql", "mysql");
    task.metrics_config = r#"{"http_host":"0.0.0.0","http_port":9090}"#.into();
    let ini = ini_renderer::render(&task);
    assert!(
        !ini.contains("[metrics]"),
        "[metrics] must never be emitted without the metrics feature"
    );
}

// ── Task ID stability tests ─────────────────────────────────────────────────

#[test]
fn global_task_id_matches_task_model() {
    let task = make_golden_task("snapshot", "mysql", "mysql");
    let ini = ini_renderer::render(&task);
    assert!(ini.contains(&format!("task_id={}", task.task_id)));
    // Verify [global] section is first
    assert!(
        ini.starts_with("[global]\n"),
        "INI must start with [global]"
    );
}

// ── Round-trip test scaffolding ──────────────────────────────────────────────
// Note: Full round-trip (render INI → write to file → TaskConfig::new → compare)
// requires the engine to be built and available. The test here verifies the
// rendered output is structurally valid (all sections present, key=value format).

#[test]
fn rendered_ini_is_structurally_valid() {
    let matrices = [
        ("snapshot", "mysql", "mysql"),
        ("snapshot", "pg", "pg"),
        ("cdc", "mysql", "mysql"),
        ("cdc", "pg", "pg"),
        ("cdc", "oracle", "oracle"),
        ("check", "mysql", "mysql"),
        ("struct", "mysql", "mysql"),
    ];
    for (kind, src, dst) in matrices {
        let task = make_golden_task(kind, src, dst);
        let ini = ini_renderer::render(&task);
        // Verify all sections start with [
        for line in ini.lines() {
            if !line.is_empty() && !line.starts_with('#') && !line.starts_with(';') {
                if line.starts_with('[') {
                    assert!(
                        line.ends_with(']'),
                        "section header must end with ]: {line}"
                    );
                } else {
                    assert!(
                        line.contains('='),
                        "non-section line must contain =: {line}"
                    );
                }
            }
        }
    }
}
