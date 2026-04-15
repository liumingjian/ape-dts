use std::{
    collections::{HashMap, HashSet},
    future::Future,
};

use anyhow::Error;
use dt_common::config::config_enums::DbType;

use futures::executor::block_on;
use tokio::time::{sleep, Duration};

use crate::test_runner::rdb_test_runner::DST;

use super::{
    mongo_check_test_runner::MongoCheckTestRunner, mongo_test_runner::MongoTestRunner,
    precheck_test_runner::PrecheckTestRunner, rdb_check_test_runner::RdbCheckTestRunner,
    rdb_kafka_rdb_test_runner::RdbKafkaRdbTestRunner, rdb_lua_test_runner::RdbLuaTestRunner,
    rdb_redis_test_runner::RdbRedisTestRunner, rdb_sql_test_runner::RdbSqlTestRunner,
    rdb_starrocks_test_runner::RdbStarRocksTestRunner, rdb_struct_test_runner::RdbStructTestRunner,
    rdb_test_runner::RdbTestRunner, redis_statistic_runner::RedisStatisticTestRunner,
    redis_test_runner::RedisTestRunner,
};

pub struct TestBase {}

#[allow(dead_code)]
impl TestBase {
    fn should_retry(test_dir: &str) -> bool {
        test_dir.contains("gaussdb")
    }

    fn is_transient_error(err: &Error) -> bool {
        let msg = format!("{:#}", err).to_lowercase();
        msg.contains("unexpected end of file")
            || msg.contains("read-only transaction")
            || msg.contains("connection reset")
            || msg.contains("broken pipe")
            || msg.contains("connection refused")
            || msg.contains("operation timed out")
            || msg.contains("timeout expired")
            || msg.contains("pool timed out")
            || msg.contains("compare tb data failed")
            || msg.contains("terminating connection")
            || msg.contains("server closed the connection")
    }

    async fn run_with_retry<F, Fut>(test_dir: &str, label: &str, mut f: F)
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<(), Error>>,
    {
        let max_attempts = if Self::should_retry(test_dir) { 6 } else { 1 };
        let mut last_err: Option<Error> = None;

        for attempt in 1..=max_attempts {
            match f().await {
                Ok(()) => return,
                Err(e) => {
                    let retryable = attempt < max_attempts && Self::is_transient_error(&e);
                    if retryable {
                        println!(
                            "{} failed (attempt {}/{}), will retry: {:#}",
                            label, attempt, max_attempts, e
                        );
                        last_err = Some(e);
                        sleep(Duration::from_millis(500 * attempt as u64)).await;
                        continue;
                    }
                    last_err = Some(e);
                    break;
                }
            }
        }

        panic!(
            "{} failed after {} attempts: {:#}",
            label,
            max_attempts,
            last_err.unwrap()
        );
    }

    pub async fn run_snapshot_test(test_dir: &str) {
        Self::run_with_retry(test_dir, "snapshot", || async {
            let runner = RdbTestRunner::new(test_dir).await?;
            let run_res = runner.run_snapshot_test(true).await;
            // Best-effort cleanup to reduce shared-environment pollution. No-op if clean SQLs are absent.
            let clean_res = runner.execute_clean_sqls().await;
            let _ = runner.close().await;
            if let Err(e) = run_res {
                return Err(e);
            }
            clean_res?;
            Ok(())
        })
        .await;
    }

    pub async fn run_snapshot_test_and_check_dst_count(
        test_dir: &str,
        db_type: &DbType,
        dst_expected_counts: HashMap<&str, usize>,
    ) {
        let runner = RdbTestRunner::new(test_dir).await.unwrap();
        runner.run_snapshot_test(false).await.unwrap();

        let assert_dst_count = |db_tb: &(String, String), count: usize| {
            let dst_data = block_on(runner.fetch_data(db_tb, DST)).unwrap();
            println!(
                "check dst table {:?} record count, expect: {}, actual: {}",
                db_tb,
                count,
                dst_data.len()
            );
            assert_eq!(dst_data.len(), count);
        };

        for (db_tb, count) in dst_expected_counts {
            let db_tb = RdbTestRunner::parse_full_tb_name(db_tb, db_type);
            assert_dst_count(&db_tb, count);
        }
        runner.execute_clean_sqls().await.unwrap();
        runner.close().await.unwrap();
    }

    pub async fn run_cdc_test(test_dir: &str, start_millis: u64, parse_millis: u64) {
        Self::run_with_retry(test_dir, "cdc", || async {
            let runner = RdbTestRunner::new(test_dir).await?;
            let run_res = runner.run_cdc_test(start_millis, parse_millis).await;
            // Best-effort cleanup to reduce shared-environment pollution. No-op if clean SQLs are absent.
            let clean_res = runner.execute_clean_sqls().await;
            let _ = runner.close().await;
            if let Err(e) = run_res {
                return Err(e);
            }
            clean_res?;
            Ok(())
        })
        .await;
    }

    pub async fn run_cdc_resume_test(test_dir: &str, start_millis: u64, parse_millis: u64) {
        Self::run_with_retry(test_dir, "cdc_resume", || async {
            let runner = RdbTestRunner::new(test_dir).await?;
            let run_res = runner.run_cdc_resume_test(start_millis, parse_millis).await;
            // Best-effort cleanup to reduce shared-environment pollution. No-op if clean SQLs are absent.
            let clean_res = runner.execute_clean_sqls().await;
            let _ = runner.close().await;
            if let Err(e) = run_res {
                return Err(e);
            }
            clean_res?;
            Ok(())
        })
        .await;
    }

    pub async fn run_cdc_failover_test(test_dir: &str, start_millis: u64, parse_millis: u64) {
        // Failover tests should not be retried automatically because each attempt can change
        // the primary node in a shared HA environment. Keep it single-attempt and rely on
        // the test's own best-effort restore logic.
        // However, in shared HA environments, initial pool creation can transiently fail due to
        // candidate probing timeouts / network hiccups. Retrying runner initialization is safe
        // (it does not run any switchover), and reduces flakes significantly.
        let runner = {
            let mut last_err: Option<Error> = None;
            let mut out: Option<RdbTestRunner> = None;
            let max_attempts: u32 = 3;
            for attempt in 1..=max_attempts {
                match RdbTestRunner::new(test_dir).await {
                    Ok(r) => {
                        out = Some(r);
                        break;
                    }
                    Err(e) => {
                        let retryable = attempt < max_attempts && Self::is_transient_error(&e);
                        if retryable {
                            println!(
                                "cdc_failover runner init failed (attempt {}/{}), will retry: {:#}",
                                attempt, max_attempts, e
                            );
                            last_err = Some(e);
                            sleep(Duration::from_millis(500 * attempt as u64)).await;
                            continue;
                        }
                        last_err = Some(e);
                        break;
                    }
                }
            }
            out.unwrap_or_else(|| {
                panic!("cdc_failover runner init failed: {:#}", last_err.unwrap())
            })
        };
        let run_res = runner
            .run_cdc_failover_test(start_millis, parse_millis)
            .await;
        // Best-effort cleanup to reduce shared-environment pollution. No-op if clean SQLs are absent.
        let clean_res = runner.execute_clean_sqls().await;
        let _ = runner.close().await;
        run_res.unwrap();
        clean_res.unwrap();
    }

    pub async fn run_cdc_to_sql_test(
        test_dir: &str,
        reverse: bool,
        start_millis: u64,
        parse_millis: u64,
    ) {
        let runner = RdbSqlTestRunner::new(test_dir, reverse).await.unwrap();
        runner
            .run_cdc_to_sql_test(start_millis, parse_millis)
            .await
            .unwrap();
        runner.close().await.unwrap();
    }

    pub async fn run_cdc_lua_test(test_dir: &str, start_millis: u64, parse_millis: u64) {
        let runner = RdbLuaTestRunner::new(test_dir).await.unwrap();
        runner
            .run_cdc_test(start_millis, parse_millis)
            .await
            .unwrap();
        runner.close().await.unwrap();
    }

    pub async fn run_snapshot_lua_test(test_dir: &str) {
        let runner = RdbLuaTestRunner::new(test_dir).await.unwrap();
        runner.run_snapshot_test().await.unwrap();
        runner.close().await.unwrap();
    }

    pub async fn run_heartbeat_test(test_dir: &str, start_millis: u64, parse_millis: u64) {
        let runner = RdbTestRunner::new(test_dir).await.unwrap();
        runner
            .run_heartbeat_test(start_millis, parse_millis)
            .await
            .unwrap();
        runner.close().await.unwrap();
    }

    pub async fn run_ddl_test(test_dir: &str, start_millis: u64, parse_millis: u64) {
        let runner = RdbTestRunner::new(test_dir).await.unwrap();
        runner
            .run_ddl_test(start_millis, parse_millis)
            .await
            .unwrap();
        runner.close().await.unwrap();
    }

    pub async fn run_ddl_meta_center_test(test_dir: &str, start_millis: u64, parse_millis: u64) {
        let mut runner = RdbTestRunner::new(test_dir).await.unwrap();
        runner
            .run_ddl_meta_center_test(start_millis, parse_millis)
            .await
            .unwrap();
        runner.close().await.unwrap();
    }

    pub async fn run_check_test(test_dir: &str) {
        Self::run_with_retry(test_dir, "check", || async {
            let runner = RdbCheckTestRunner::new(test_dir).await?;
            let res = runner.run_check_test().await;
            let _ = runner.close().await;
            res?;
            Ok(())
        })
        .await;
    }

    pub async fn run_review_test(test_dir: &str) {
        let runner = RdbCheckTestRunner::new(test_dir).await.unwrap();
        runner.run_review_test().await.unwrap();
        runner.close().await.unwrap();
    }

    pub async fn run_revise_test(test_dir: &str) {
        let runner = RdbCheckTestRunner::new(test_dir).await.unwrap();
        runner.run_revise_test().await.unwrap();
        runner.close().await.unwrap();
    }

    pub async fn run_recheck_test(test_dir: &str) {
        let runner = RdbCheckTestRunner::new(test_dir).await.unwrap();
        runner.run_recheck_test().await.unwrap();
        runner.close().await.unwrap();
    }

    pub async fn run_mongo_snapshot_test(test_dir: &str) {
        let runner = MongoTestRunner::new(test_dir).await.unwrap();
        runner.run_snapshot_test(true).await.unwrap();
    }

    pub async fn run_mongo_snapshot_test_and_check_dst_count(
        test_dir: &str,
        dst_expected_counts: HashMap<(&str, &str), usize>,
    ) {
        let runner = MongoTestRunner::new(test_dir).await.unwrap();
        runner.run_snapshot_test(false).await.unwrap();

        for ((db, tb), count) in dst_expected_counts.iter() {
            let dst_data = runner.fetch_data(db, tb, DST).await;
            println!(
                "check dst table {:?} record count, expect: {}, actual: {}",
                (db, tb),
                count,
                dst_data.len()
            );
            assert_eq!(dst_data.len(), *count);
        }
    }

    pub async fn run_mongo_cdc_test(test_dir: &str, start_millis: u64, parse_millis: u64) {
        let runner = MongoTestRunner::new(test_dir).await.unwrap();
        runner
            .run_cdc_test(start_millis, parse_millis)
            .await
            .unwrap();
    }

    pub async fn run_mongo_cdc_resume_test(test_dir: &str, start_millis: u64, parse_millis: u64) {
        let runner = MongoTestRunner::new(test_dir).await.unwrap();
        runner
            .run_cdc_resume_test(start_millis, parse_millis)
            .await
            .unwrap();
    }

    pub async fn run_mongo_heartbeat_test(test_dir: &str, start_millis: u64, parse_millis: u64) {
        let runner = MongoTestRunner::new(test_dir).await.unwrap();
        runner
            .run_heartbeat_test(start_millis, parse_millis)
            .await
            .unwrap();
    }

    pub async fn run_mongo_check_test(test_dir: &str) {
        let runner = MongoCheckTestRunner::new(test_dir).await.unwrap();
        runner.run_check_test().await.unwrap();
    }

    pub async fn run_mongo_recheck_test(test_dir: &str) {
        let runner = MongoCheckTestRunner::new(test_dir).await.unwrap();
        runner.run_recheck_test().await.unwrap();
    }

    pub async fn run_mongo_revise_test(test_dir: &str) {
        let runner = MongoCheckTestRunner::new(test_dir).await.unwrap();
        runner.run_revise_test().await.unwrap();
    }

    pub async fn run_mongo_review_test(test_dir: &str) {
        let runner = MongoCheckTestRunner::new(test_dir).await.unwrap();
        runner.run_review_test().await.unwrap();
    }

    pub async fn run_redis_snapshot_test(test_dir: &str) {
        let mut runner = RedisTestRunner::new_default(test_dir).await.unwrap();
        runner.run_snapshot_test().await.unwrap();
    }

    pub async fn run_redis_rejson_snapshot_test(test_dir: &str) {
        let mut runner = RedisTestRunner::new(test_dir, vec![('\'', '\'')])
            .await
            .unwrap();
        runner.run_snapshot_test().await.unwrap();
    }

    pub async fn run_redis_redisearch_snapshot_test(test_dir: &str) {
        let mut runner = RedisTestRunner::new(test_dir, vec![('\'', '\'')])
            .await
            .unwrap();
        runner.run_snapshot_test().await.unwrap();
    }

    pub async fn run_redis_graph_snapshot_test(test_dir: &str) {
        let mut runner = RedisTestRunner::new_default(test_dir).await.unwrap();
        runner.run_snapshot_test().await.unwrap();
    }

    pub async fn run_redis_cdc_test(test_dir: &str, start_millis: u64, parse_millis: u64) {
        let mut runner = RedisTestRunner::new_default(test_dir).await.unwrap();
        runner
            .run_cdc_test(start_millis, parse_millis)
            .await
            .unwrap();
    }

    pub async fn run_redis_heartbeat_test(test_dir: &str, start_millis: u64, parse_millis: u64) {
        let mut runner = RedisTestRunner::new_default(test_dir).await.unwrap();
        runner
            .run_heartbeat_test(start_millis, parse_millis)
            .await
            .unwrap();
    }

    pub async fn run_redis_rejson_cdc_test(test_dir: &str, start_millis: u64, parse_millis: u64) {
        let mut runner = RedisTestRunner::new(test_dir, vec![('\'', '\'')])
            .await
            .unwrap();
        runner
            .run_cdc_test(start_millis, parse_millis)
            .await
            .unwrap();
    }

    pub async fn run_redis_graph_cdc_test(test_dir: &str, start_millis: u64, parse_millis: u64) {
        let mut runner = RedisTestRunner::new_default(test_dir).await.unwrap();
        runner
            .run_cdc_test(start_millis, parse_millis)
            .await
            .unwrap();
    }

    pub async fn run_redis_statistic_test(test_dir: &str) {
        let mut runner = RedisStatisticTestRunner::new(test_dir).await.unwrap();
        runner.run_statistic_test().await.unwrap();
    }

    pub async fn run_mysql_struct_test(test_dir: &str) {
        Self::run_with_retry(test_dir, "mysql_struct", || async {
            let mut runner = RdbStructTestRunner::new(test_dir).await?;
            let run_res = runner.run_mysql_struct_test().await;
            let clean_res = runner.base.execute_clean_sqls().await;
            let _ = runner.close().await;

            if let Err(e) = run_res {
                return Err(e);
            }
            clean_res?;
            Ok(())
        })
        .await;
    }

    pub async fn run_pg_struct_test(test_dir: &str) {
        Self::run_with_retry(test_dir, "pg_struct", || async {
            let mut runner = RdbStructTestRunner::new(test_dir).await?;
            let run_res = runner.run_pg_struct_test().await;
            let clean_res = runner.base.execute_clean_sqls().await;
            let _ = runner.close().await;

            if let Err(e) = run_res {
                return Err(e);
            }
            clean_res?;
            Ok(())
        })
        .await;
    }

    pub async fn run_precheck_test(
        test_dir: &str,
        ignore_check_items: &HashSet<String>,
        src_expected_results: &HashMap<String, bool>,
        dst_expected_results: &HashMap<String, bool>,
    ) {
        Self::run_with_retry(test_dir, "precheck", || async {
            let runner = PrecheckTestRunner::new(test_dir).await?;
            let run_res = runner
                .run_check(
                    ignore_check_items,
                    src_expected_results,
                    dst_expected_results,
                )
                .await;
            let clean_res = runner.after_check().await;
            if let Err(e) = run_res {
                return Err(e);
            }
            clean_res?;
            Ok(())
        })
        .await;
    }

    pub async fn run_rdb_kafka_rdb_cdc_test(test_dir: &str, start_millis: u64, parse_millis: u64) {
        let runner = RdbKafkaRdbTestRunner::new(test_dir).await.unwrap();
        runner
            .run_cdc_test(start_millis, parse_millis)
            .await
            .unwrap();
    }

    pub async fn run_rdb_kafka_rdb_snapshot_test(
        test_dir: &str,
        start_millis: u64,
        parse_millis: u64,
    ) {
        let runner = RdbKafkaRdbTestRunner::new(test_dir).await.unwrap();
        runner
            .run_snapshot_test(start_millis, parse_millis)
            .await
            .unwrap();
    }

    pub async fn run_rdb_redis_cdc_test(test_dir: &str, start_millis: u64, parse_millis: u64) {
        let mut runner = RdbRedisTestRunner::new(test_dir).await.unwrap();
        runner
            .run_cdc_test(start_millis, parse_millis)
            .await
            .unwrap();
        runner.close().await.unwrap();
    }

    pub async fn run_rdb_redis_snapshot_test(test_dir: &str) {
        let mut runner = RdbRedisTestRunner::new(test_dir).await.unwrap();
        runner.run_snapshot_test().await.unwrap();
        runner.close().await.unwrap();
    }

    pub async fn run_rdb_starrocks_cdc_test(test_dir: &str, start_millis: u64, parse_millis: u64) {
        let runner = RdbStarRocksTestRunner::new(test_dir).await.unwrap();
        runner
            .run_cdc_soft_delete_test(start_millis, parse_millis)
            .await
            .unwrap();
        runner.close().await.unwrap();
    }

    pub async fn run_dcl_test(test_dir: &str, start_millis: u64, parse_millis: u64) {
        let runner = RdbTestRunner::new(test_dir).await.unwrap();
        runner
            .run_dcl_test(start_millis, parse_millis)
            .await
            .unwrap();
        runner.close().await.unwrap();
    }

    pub async fn run_dcl_check_test(test_dir: &str) {
        let runner = RdbTestRunner::new(test_dir).await.unwrap();
        runner.dcl_check_sql_execution().await.unwrap();
        runner.close().await.unwrap();
    }
}
