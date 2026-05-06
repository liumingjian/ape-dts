//! Utility to regenerate golden fixtures from the renderer.
//! Run: cargo test -p dt-console-server --test regen_golden -- --nocapture

use dt_console_server::ini_renderer;
use dt_console_server::models::Task;

fn make_golden_task(kind: &str, db_type_source: &str, db_type_target: &str) -> Task {
    let task_id = format!("{kind}_{db_type_source}_{db_type_target}_abcd1234");

    let (
        extractor_config,
        sinker_config,
        filter_config,
        router_config,
        parallelizer_config,
        pipeline_config,
    ) = match (kind, db_type_source, db_type_target) {
        ("snapshot", "mysql", "mysql") => (
            r#"{"extractType":"snapshot","url":"mysql://src:3307/db","username":"root","password":"src_pass"}"#,
            r#"{"sinkType":"write","url":"mysql://dst:3308/db","username":"root","password":"dst_pass","batch_size":2,"replace":true,"disable_foreign_key_checks":true,"transaction_isolation":"read_committed"}"#,
            r#"{"do_dbs":"","ignore_dbs":"","do_tbs":"test_db_1.*","ignore_tbs":"","do_events":"insert"}"#,
            r#"{"db_map":"","tb_map":"","col_map":""}"#,
            r#"{"parallel_type":"snapshot","parallel_size":2}"#,
            r#"{"buffer_size":4,"checkpoint_interval_secs":1}"#,
        ),
        ("snapshot", "pg", "pg") => (
            r#"{"extractType":"snapshot","url":"postgres://src:5433/db","username":"root","password":"src_pass"}"#,
            r#"{"sinkType":"write","url":"postgres://dst:5434/db","username":"root","password":"dst_pass","batch_size":2,"replace":true,"disable_foreign_key_checks":true}"#,
            r#"{"do_dbs":"test_db_1","ignore_dbs":"","do_tbs":"","ignore_tbs":"","do_events":"insert"}"#,
            r#"{"db_map":"","tb_map":"","col_map":""}"#,
            r#"{"parallel_type":"snapshot","parallel_size":2}"#,
            r#"{"buffer_size":4,"checkpoint_interval_secs":1}"#,
        ),
        ("snapshot", "mysql", "kafka") => (
            r#"{"extractType":"snapshot","url":"mysql://src:3307/db"}"#,
            r#"{"sinkType":"write","url":"kafka://kafka:9092","batch_size":2}"#,
            r#"{"do_dbs":"","ignore_dbs":"","do_tbs":"test_db_1.*,test_db_2.*","ignore_tbs":"","do_events":"insert,update,delete"}"#,
            r#"{"db_map":"","tb_map":"","col_map":"","topic_map":"*.*:test,test_db_1.*:test2"}"#,
            r#"{"parallel_type":"serial","parallel_size":1}"#,
            r#"{"buffer_size":16000,"checkpoint_interval_secs":15}"#,
        ),
        ("cdc", "mysql", "mysql") => (
            r#"{"extractType":"cdc","url":"mysql://src:3307/db","username":"root","password":"src_pass","server_id":2000,"binlog_filename":"","binlog_position":0,"heartbeat_interval_secs":1,"heartbeat_tb":"heartbeat_db.ape_dts_heartbeat"}"#,
            r#"{"sinkType":"write","url":"mysql://dst:3308/db","username":"root","password":"dst_pass","batch_size":2}"#,
            r#"{"do_dbs":"","ignore_dbs":"","do_tbs":"test_db_1.*,upper_case_db.*","ignore_tbs":"","do_events":"insert,update,delete","do_ddls":"*"}"#,
            r#"{"db_map":"","tb_map":"","col_map":""}"#,
            r#"{"parallel_type":"rdb_merge","parallel_size":2}"#,
            r#"{"buffer_size":4,"checkpoint_interval_secs":1}"#,
        ),
        ("cdc", "pg", "pg") => (
            r#"{"extractType":"cdc","url":"postgres://src:5433/db","username":"root","password":"src_pass","slot_name":"ape_test","pub_name":"","start_lsn":"","recreate_slot_if_exists":true,"heartbeat_interval_secs":10,"heartbeat_tb":"heartbeat_db.ape_dts_heartbeat"}"#,
            r#"{"sinkType":"write","url":"postgres://dst:5434/db","username":"root","password":"dst_pass","batch_size":2}"#,
            r#"{"do_dbs":"upper_case_db","ignore_dbs":"","do_tbs":"public.default_table","ignore_tbs":"","do_events":"insert,update,delete"}"#,
            r#"{"db_map":"","tb_map":"","col_map":""}"#,
            r#"{"parallel_type":"rdb_merge","parallel_size":2}"#,
            r#"{"buffer_size":4,"checkpoint_interval_secs":1}"#,
        ),
        ("cdc", "oracle", "oracle") => (
            r#"{"extractType":"cdc","url":"oracle://src:1521/db","username":"system","password":"oracle","cdc_mode":"logminer","poll_interval_millis":200,"poll_batch_size":200,"start_scn":0,"start_time_utc":"","end_time_utc":""}"#,
            r#"{"sinkType":"write","url":"oracle://dst:1521/db","username":"system","password":"oracle","batch_size":2}"#,
            r#"{"do_dbs":"","ignore_dbs":"","do_tbs":"APE_DTS.ORA_CDC_BASIC","ignore_tbs":"","do_events":"insert,update,delete"}"#,
            r#"{"db_map":"","tb_map":"","col_map":""}"#,
            r#"{"parallel_type":"serial","parallel_size":1}"#,
            r#"{"buffer_size":4,"checkpoint_interval_secs":1}"#,
        ),
        ("check", "mysql", "mysql") => (
            r#"{"extractType":"snapshot","url":"mysql://src:3307/db"}"#,
            r#"{"sinkType":"check","url":"mysql://dst:3308/db","batch_size":2,"check_log_dir":"./check"}"#,
            r#"{"do_dbs":"","ignore_dbs":"","do_tbs":"test_db_1.*","ignore_tbs":"","do_events":"insert"}"#,
            r#"{"db_map":"","tb_map":"","col_map":""}"#,
            r#"{"parallel_type":"rdb_check","parallel_size":2}"#,
            r#"{"buffer_size":4,"checkpoint_interval_secs":1}"#,
        ),
        ("struct", "mysql", "mysql") => (
            r#"{"extractType":"struct","url":"mysql://src:3307/db"}"#,
            r#"{"sinkType":"struct","url":"mysql://dst:3308/db","conflict_policy":"interrupt"}"#,
            r#"{"do_dbs":"struct_it_mysql2mysql_1","ignore_dbs":"","do_tbs":"","ignore_tbs":"","do_events":"","do_structures":"table,index,constraint"}"#,
            r#"{"db_map":"","tb_map":"","col_map":""}"#,
            r#"{"parallel_type":"serial","parallel_size":1}"#,
            r#"{"checkpoint_interval_secs":1,"buffer_size":100}"#,
        ),
        ("snapshot", "gaussdb_pg", "mysql") => (
            r#"{"extractType":"snapshot","url":"postgres://gaussdb:8000/db","username":"root","password":"gaussdb_pass"}"#,
            r#"{"sinkType":"write","url":"mysql://dst:3308/db","username":"root","password":"dst_pass","batch_size":2,"replace":true,"disable_foreign_key_checks":true}"#,
            r#"{"do_dbs":"","ignore_dbs":"","do_tbs":"public.gaussdb_to_pg_snapshot_basic","ignore_tbs":"","do_events":"insert"}"#,
            r#"{"db_map":"","tb_map":"","col_map":""}"#,
            r#"{"parallel_type":"snapshot","parallel_size":1}"#,
            r#"{"buffer_size":4,"checkpoint_interval_secs":1}"#,
        ),
        ("cdc", "gaussdb_mysql", "mysql") => (
            r#"{"extractType":"cdc","url":"mysql://gaussdb:3311/db","username":"root","password":"gaussdb_pass","server_id":2000,"heartbeat_interval_secs":1,"heartbeat_tb":"heartbeat_db.ape_dts_heartbeat"}"#,
            r#"{"sinkType":"write","url":"mysql://dst:3308/db","username":"root","password":"dst_pass","batch_size":2,"replace":true,"disable_foreign_key_checks":true}"#,
            r#"{"do_dbs":"","ignore_dbs":"","do_tbs":"public.gaussdb_to_mysql_cdc_basic","ignore_tbs":"","do_events":"insert,update,delete"}"#,
            r#"{"db_map":"public:gaussdb_to_mysql_cdc_dst","tb_map":"","col_map":""}"#,
            r#"{"parallel_type":"rdb_merge","parallel_size":1}"#,
            r#"{"buffer_size":4,"checkpoint_interval_secs":1}"#,
        ),
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

#[test]
fn regen_all_golden_fixtures() {
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
        let rendered = ini_renderer::render(&task);
        let name = format!("{kind}_{src}_to_{dst}");
        let path = format!("tests/golden/{name}.ini");
        std::fs::write(&path, &rendered).unwrap_or_else(|e| panic!("failed to write {path}: {e}"));
        println!("Wrote {path} ({} bytes)", rendered.len());
    }
}
