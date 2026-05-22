use super::*;

fn make_task(db_type_source: &str, extractor_json: &str, kind: &str) -> Task {
    Task {
        id: "t".into(),
        task_id: "t".into(),
        name: "n".into(),
        kind: kind.into(),
        db_type_source: db_type_source.into(),
        db_type_target: "mysql".into(),
        source_endpoint: r#"{"url":"mysql://src:3306/db"}"#.into(),
        target_endpoint: r#"{"url":"mysql://tgt:3306/db"}"#.into(),
        extractor_config: extractor_json.into(),
        sinker_config: r#"{"sink_type":"write"}"#.into(),
        filter_config: r#"{"do_dbs":"db","do_tbs":"db.t1","do_events":"insert,update,delete"}"#
            .into(),
        router_config: "{}".into(),
        parallelizer_config: "{}".into(),
        pipeline_config: "{}".into(),
        resumer_config: "{}".into(),
        processor_config: "{}".into(),
        runtime_config: "{}".into(),
        metrics_config: "{}".into(),
        resource_group_id: "default".into(),
        owner_user_id: None,
        status: "draft".into(),
        created_at: "now".into(),
        updated_at: "now".into(),
    }
}

fn make_oracle_task(extractor_json: &str) -> Task {
    let mut task = make_task("oracle", extractor_json, "snapshot");
    task.db_type_target = "oracle".into();
    task.source_endpoint =
        r#"{"url":"oracle://127.0.0.1:15211/XE","username":"APE_SRC","password":"ape_dts"}"#.into();
    task.target_endpoint =
        r#"{"url":"oracle://127.0.0.1:15211/XE","username":"APE_DST","password":"ape_dts"}"#.into();
    task.filter_config =
        r#"{"do_dbs":"APE_SRC","do_tbs":"APE_SRC.CDC_SMOKE","do_events":"insert,update,delete"}"#
            .into();
    task.router_config =
        r#"{"db_map":"APE_SRC:APE_DST","tb_map":"APE_SRC.CDC_SMOKE:APE_DST.CDC_SMOKE"}"#.into();
    task
}

#[test]
fn detects_mysql_snapshot_and_cdc() {
    let task = make_task(
        "mysql",
        r#"{"extract_type":"snapshot_and_cdc","server_id":"1234","url":"mysql://src:3306/db"}"#,
        "snapshot",
    );
    assert!(is_two_phase_task(&task));
}

#[test]
fn detects_gaussdb_mysql_snapshot_and_cdc() {
    let task = make_task(
        "gaussdb_mysql",
        r#"{"extract_type":"snapshot_and_cdc","server_id":"1234"}"#,
        "snapshot",
    );
    assert!(is_two_phase_task(&task));
}

#[test]
fn detects_gaussdb_pg_snapshot_and_cdc() {
    let task = make_task(
        "gaussdb_pg",
        r#"{"extract_type":"snapshot_and_cdc","slot_name":"ape_test_gaussdb_pg"}"#,
        "snapshot",
    );
    assert!(is_two_phase_task(&task));
}

#[test]
fn detects_gaussdb_oracle_snapshot_and_cdc() {
    let task = make_task(
        "gaussdb_oracle",
        r#"{"extract_type":"snapshot_and_cdc","slot_name":"ape_test_gaussdb_oracle"}"#,
        "snapshot",
    );
    assert!(is_two_phase_task(&task));
}

#[test]
fn detects_oracle_snapshot_and_cdc() {
    let task = make_oracle_task(r#"{"extract_type":"snapshot_and_cdc","cdc_mode":"logminer"}"#);
    assert!(is_two_phase_task(&task));
}

#[test]
fn detects_pg_snapshot_and_cdc() {
    let task = make_task(
        "pg",
        r#"{"extract_type":"snapshot_and_cdc","slot_name":"ape_test_pg"}"#,
        "snapshot",
    );
    assert!(is_two_phase_task(&task));
}

#[test]
fn rejects_plain_snapshot() {
    let task = make_task("mysql", r#"{"extract_type":"snapshot"}"#, "snapshot");
    assert!(!is_two_phase_task(&task));
}

#[test]
fn oracle_phase2_ini_has_logminer_start_scn() {
    let task = make_oracle_task(
        r#"{"extract_type":"snapshot_and_cdc","cdc_mode":"logminer","poll_interval_millis":200,"poll_batch_size":200}"#,
    );
    let prep = prepare_run_dir(
        &task,
        &unique_tmp_dir("oracle-scn"),
        TwoPhaseStart {
            start_time_utc: String::new(),
            start_scn: Some(12345),
        },
    )
    .unwrap();

    assert!(prep.phase1_ini.contains("extract_type=snapshot"));
    assert!(prep.phase2_ini.contains("extract_type=cdc"));
    assert!(prep.phase2_ini.contains("cdc_mode=logminer"));
    assert!(prep.phase2_ini.contains("start_scn=12345"));
    assert!(prep.phase2_ini.contains("parallel_type=serial"));
    assert_eq!(prep.start_scn, Some(12345));
}

#[test]
fn phase1_ini_has_snapshot_extract_type_and_insert_only_filter() {
    let task = make_task(
        "mysql",
        r#"{"extract_type":"snapshot_and_cdc","server_id":"1234","url":"mysql://src:3306/db"}"#,
        "snapshot",
    );
    let ini = render_phase1_ini(&task);
    assert!(ini.contains("extract_type=snapshot"));
    assert!(!ini.contains("extract_type=snapshot_and_cdc"));
    assert!(ini.contains("do_events=insert"));
    assert!(ini.contains("parallel_type=snapshot"));
}

#[test]
fn phase2_ini_has_cdc_extract_type_and_start_time_utc() {
    let mut task = make_task(
        "mysql",
        r#"{"extract_type":"snapshot_and_cdc","server_id":"1234","url":"mysql://src:3306/db","heartbeat_interval_secs":5}"#,
        "snapshot",
    );
    task.parallelizer_config = r#"{"parallel_type":"snapshot","parallel_size":4}"#.into();
    let ini = render_phase2_ini(&task, "2026-01-01 00:00:00.000");
    assert!(ini.contains("extract_type=cdc"));
    assert!(ini.contains("start_time_utc=2026-01-01 00:00:00.000"));
    assert!(ini.contains("server_id=1234"));
    assert!(ini.contains("heartbeat_interval_secs=5"));
    assert!(ini.contains("do_events=insert,update,delete"));
    assert!(ini.contains("parallel_type=rdb_merge"));
    assert!(ini.contains("parallel_size=4"));
}

fn unique_tmp_dir(label: &str) -> std::path::PathBuf {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("ape-dts-two-phase-{label}-{pid}-{nanos}"))
}

#[test]
fn prepare_run_dir_writes_phase2_files() {
    let dir = unique_tmp_dir("prep");
    let task = make_task(
        "mysql",
        r#"{"extract_type":"snapshot_and_cdc","server_id":"1234","url":"mysql://src:3306/db"}"#,
        "snapshot",
    );
    let prep = prepare_run_dir(
        &task,
        &dir,
        TwoPhaseStart {
            start_time_utc: format_start_time_utc(chrono::Utc::now()),
            start_scn: None,
        },
    )
    .unwrap();
    assert!(prep.phase1_ini.contains("extract_type=snapshot"));
    assert!(prep.phase2_ini.contains("extract_type=cdc"));
    assert!(!prep.start_time_utc.is_empty());
    assert!(dir.join(PHASE2_INI_FILE).exists());
    assert!(dir.join(PHASE_STATE_FILE).exists());

    let state = read_phase_state(&dir).unwrap();
    assert_eq!(state.current_phase, 1);
    assert_eq!(state.start_time_utc, prep.start_time_utc);
    assert_eq!(state.start_scn, None);

    mark_phase_advanced(&dir).unwrap();
    let state = read_phase_state(&dir).unwrap();
    assert_eq!(state.current_phase, 2);

    let _ = std::fs::remove_dir_all(&dir);
}
