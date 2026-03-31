use std::{
    collections::{HashMap, HashSet},
    env,
    str::FromStr,
};

use chrono::{Duration, Utc};
use dt_common::{
    config::{
        config_enums::DbType,
        config_token_parser::{ConfigTokenParser, TokenEscapePair},
        extractor_config::ExtractorConfig,
        meta_center_config::MetaCenterConfig,
        sinker_config::SinkerConfig,
        task_config::TaskConfig,
    },
    meta::{ddl_meta::ddl_type::DdlType, time::dt_utc_time::DtNaiveTime},
    rdb_filter::RdbFilter,
    utils::{sql_util::SqlUtil, time_util::TimeUtil},
};

use dt_common::meta::{
    col_value::ColValue, ddl_meta::ddl_parser::DdlParser,
    mysql::mysql_meta_manager::MysqlMetaManager, row_data::RowData,
};
use dt_connector::{
    meta_fetcher::mysql::mysql_struct_check_fetcher::MysqlStructCheckFetcher, rdb_router::RdbRouter,
};
use dt_task::{task_runner::TaskRunner, task_util::TaskUtil};

use sqlx::{query, types::BigDecimal, MySql, Pool, Postgres, Row};
use tokio::task::JoinHandle;
use url::Url;

use crate::{
    test_config_util::TestConfigUtil,
    test_runner::mock_utils::{mock_config::MockConfig, mysql_type::MysqlType, pg_type::PgType},
};

use super::{base_test_runner::BaseTestRunner, rdb_util::RdbUtil};

pub struct RdbTestRunner {
    pub base: BaseTestRunner,
    pub src_conn_pool_mysql: Option<Pool<MySql>>,
    pub dst_conn_pool_mysql: Option<Pool<MySql>>,
    pub src_conn_pool_pg: Option<Pool<Postgres>>,
    pub dst_conn_pool_pg: Option<Pool<Postgres>>,
    pub meta_center_pool_mysql: Option<Pool<MySql>>,
    pub config: TaskConfig,
    pub router: RdbRouter,
    pub filter: RdbFilter,
    pub unordered_compare: bool, // whether to compare rows in unordered way
}

pub const SRC: &str = "src";
pub const DST: &str = "dst";
pub const PUBLIC: &str = "public";

const UTC_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

#[allow(dead_code)]
impl RdbTestRunner {
    pub async fn new(relative_test_dir: &str) -> anyhow::Result<Self> {
        let mut base = BaseTestRunner::new(relative_test_dir).await.unwrap();

        // prepare conn pools
        let mut src_conn_pool_mysql = None;
        let mut dst_conn_pool_mysql = None;
        let mut src_conn_pool_pg = None;
        let mut dst_conn_pool_pg = None;

        let mut config = TaskConfig::new(&base.task_config_file).unwrap();
        Self::maybe_rewrite_gaussdb_primary_urls(
            &base.task_config_file,
            &base.struct_task_config_file,
            &config,
        )
        .await?;
        config = TaskConfig::new(&base.task_config_file).unwrap();
        let src_db_type = &config.extractor_basic.db_type;
        let dst_db_type = &config.sinker_basic.db_type;
        let src_url = &config.extractor_basic.url;
        let dst_url = &config.sinker_basic.url;
        let src_connection_auth = &config.extractor_basic.connection_auth;
        let dst_connection_auth = &config.sinker_basic.connection_auth;

        // generate mock sqls
        let mut unordered_compare = false;
        let mock_result: Option<(Vec<String>, Vec<String>)> = match src_db_type {
            DbType::Pg | DbType::GaussDBPg => MockConfig::<PgType>::new(&base.task_config_file)
                .map(|c| (c.mock_ddl_stmts(), c.mock_dml_stmts())),
            DbType::Mysql => MockConfig::<MysqlType>::new(&base.task_config_file)
                .map(|c| (c.mock_ddl_stmts(), c.mock_dml_stmts())),
            _ => None,
        };
        if let Some((mock_ddl_stmts, mock_dml_stmts)) = mock_result {
            base.src_prepare_sqls.extend(mock_ddl_stmts.clone());
            base.dst_prepare_sqls.extend(mock_ddl_stmts);
            base.src_test_sqls.extend(mock_dml_stmts);

            unordered_compare = true;
        }

        let mysql_conn_settings = Some(vec!["SET FOREIGN_KEY_CHECKS=0"]);

        match src_db_type {
            DbType::Mysql => {
                src_conn_pool_mysql = Some(
                    TaskUtil::create_mysql_conn_pool(
                        src_url,
                        src_connection_auth,
                        5,
                        false,
                        mysql_conn_settings.clone(),
                    )
                    .await?,
                );
            }
            DbType::Pg | DbType::GaussDBPg => {
                let disable_fk_checks = matches!(src_db_type, DbType::Pg);
                let max_connections = if matches!(src_db_type, DbType::GaussDBPg) {
                    // GaussDB cluster endpoints may be behind VIP/LB; using multiple pooled
                    // connections can intermittently hit standby nodes (read-only transaction)
                    // or trigger unstable behavior during tests. Keep a single connection for
                    // deterministic DDL/DML and comparison queries.
                    1
                } else {
                    5
                };
                let pool_timeout = if matches!(src_db_type, DbType::GaussDBPg) {
                    std::time::Duration::from_secs(90)
                } else {
                    std::time::Duration::from_secs(30)
                };
                src_conn_pool_pg = Some(
                    tokio::time::timeout(
                        pool_timeout,
                        TaskUtil::create_pg_conn_pool(
                            src_url,
                            src_connection_auth,
                            max_connections,
                            false,
                            disable_fk_checks,
                        ),
                    )
                    .await
                    .map_err(|_| {
                        anyhow::anyhow!(
                            "operation timed out: create_pg_conn_pool(src) url={}",
                            src_url
                        )
                    })??,
                );
            }
            _ => {}
        }

        if !dst_url.is_empty() {
            match dst_db_type {
                DbType::Mysql
                | DbType::Foxlake
                | DbType::StarRocks
                | DbType::Doris
                | DbType::Tidb => {
                    dst_conn_pool_mysql = Some(
                        TaskUtil::create_mysql_conn_pool(
                            dst_url,
                            dst_connection_auth,
                            5,
                            false,
                            mysql_conn_settings.clone(),
                        )
                        .await?,
                    );
                }
                DbType::Pg | DbType::GaussDBPg => {
                    let disable_fk_checks = matches!(dst_db_type, DbType::Pg);
                    let max_connections = if matches!(dst_db_type, DbType::GaussDBPg) {
                        1
                    } else {
                        5
                    };
                    let pool_timeout = if matches!(dst_db_type, DbType::GaussDBPg) {
                        std::time::Duration::from_secs(90)
                    } else {
                        std::time::Duration::from_secs(30)
                    };
                    dst_conn_pool_pg = Some(
                        tokio::time::timeout(
                            pool_timeout,
                            TaskUtil::create_pg_conn_pool(
                                dst_url,
                                dst_connection_auth,
                                max_connections,
                                false,
                                disable_fk_checks,
                            ),
                        )
                        .await
                        .map_err(|_| {
                            anyhow::anyhow!(
                                "operation timed out: create_pg_conn_pool(dst) url={}",
                                dst_url
                            )
                        })??,
                    );
                }
                _ => {}
            }
        }

        let config = TaskConfig::new(&base.task_config_file).unwrap();
        let router = RdbRouter::from_config(&config.router, dst_db_type).unwrap();
        let filter = RdbFilter::from_config(&config.filter, dst_db_type).unwrap();
        let meta_center_pool_mysql = match &config.meta_center {
            Some(MetaCenterConfig::MySqlDbEngine {
                url,
                connection_auth,
                ..
            }) => Some(
                TaskUtil::create_mysql_conn_pool(
                    url,
                    connection_auth,
                    1,
                    false,
                    mysql_conn_settings.clone(),
                )
                .await
                .unwrap(),
            ),
            _ => None,
        };

        Ok(Self {
            src_conn_pool_mysql,
            dst_conn_pool_mysql,
            src_conn_pool_pg,
            dst_conn_pool_pg,
            meta_center_pool_mysql,
            config,
            router,
            filter,
            base,
            unordered_compare,
        })
    }

    async fn maybe_rewrite_gaussdb_primary_urls(
        task_config_file: &str,
        struct_task_config_file: &str,
        config: &TaskConfig,
    ) -> anyhow::Result<()> {
        let candidates = match env::var("gaussdb_pg_candidate_hosts") {
            Ok(v) => v,
            Err(_) => return Ok(()),
        };

        let (base_url, auth) = if config.extractor_basic.db_type == DbType::GaussDBPg {
            (
                config.extractor_basic.url.clone(),
                config.extractor_basic.connection_auth.clone(),
            )
        } else if config.sinker_basic.db_type == DbType::GaussDBPg {
            (
                config.sinker_basic.url.clone(),
                config.sinker_basic.connection_auth.clone(),
            )
        } else {
            return Ok(());
        };

        let primary_url = match Self::resolve_gaussdb_rw_url(&base_url, &auth, &candidates).await? {
            Some(v) => v,
            None => return Ok(()),
        };

        let mut updates = Vec::new();
        if config.extractor_basic.db_type == DbType::GaussDBPg {
            updates.push((
                "extractor".to_string(),
                "url".to_string(),
                primary_url.clone(),
            ));
        }
        if config.sinker_basic.db_type == DbType::GaussDBPg {
            updates.push(("sinker".to_string(), "url".to_string(), primary_url.clone()));
        }
        if !updates.is_empty() {
            TestConfigUtil::update_task_config(task_config_file, task_config_file, &updates);
        }

        if !struct_task_config_file.is_empty() {
            let struct_config = TaskConfig::new(struct_task_config_file)?;
            let mut struct_updates = Vec::new();
            if struct_config.extractor_basic.db_type == DbType::GaussDBPg {
                struct_updates.push((
                    "extractor".to_string(),
                    "url".to_string(),
                    primary_url.clone(),
                ));
            }
            if struct_config.sinker_basic.db_type == DbType::GaussDBPg {
                struct_updates.push(("sinker".to_string(), "url".to_string(), primary_url.clone()));
            }
            if !struct_updates.is_empty() {
                TestConfigUtil::update_task_config(
                    struct_task_config_file,
                    struct_task_config_file,
                    &struct_updates,
                );
            }
        }

        Ok(())
    }

    async fn resolve_gaussdb_rw_url(
        base_url: &str,
        auth: &dt_common::config::connection_auth_config::ConnectionAuthConfig,
        candidates: &str,
    ) -> anyhow::Result<Option<String>> {
        let base = Url::parse(base_url)?;
        let base_host = base.host_str().map(|s| s.to_string()).unwrap_or_default();
        let base_port = base.port();

        // Prefer direct node hosts over VIP/LB host in base_url, because VIPs may route
        // different connections to primary/standby and cause intermittent "read-only transaction"
        // errors during tests.
        let mut ordered_candidates: Vec<&str> = Vec::new();
        for candidate in candidates.split(',').map(|s| s.trim()) {
            if candidate.is_empty() {
                continue;
            }
            let host = candidate
                .rsplit_once(':')
                .map(|(h, _)| h)
                .unwrap_or(candidate)
                .trim();
            if host != base_host {
                ordered_candidates.push(candidate);
            }
        }
        for candidate in candidates.split(',').map(|s| s.trim()) {
            if candidate.is_empty() {
                continue;
            }
            let host = candidate
                .rsplit_once(':')
                .map(|(h, _)| h)
                .unwrap_or(candidate)
                .trim();
            if host == base_host {
                ordered_candidates.push(candidate);
            }
        }

        for candidate in ordered_candidates {
            if candidate.is_empty() {
                continue;
            }

            let (host, port) = match candidate.rsplit_once(':') {
                Some((h, p)) if !h.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => {
                    (h.to_string(), Some(p.parse::<u16>()?))
                }
                _ => (candidate.to_string(), base_port),
            };

            let mut u = base.clone();
            u.set_host(Some(&host))
                .map_err(|_| anyhow::anyhow!("failed to set host in url: {}", base_url))?;
            if let Some(port) = port {
                u.set_port(Some(port))
                    .map_err(|_| anyhow::anyhow!("failed to set port in url: {}", base_url))?;
            }

            let mut candidate_urls = vec![u.to_string()];
            if !candidate_urls[0].to_lowercase().contains("sslmode=disable") {
                let mut no_ssl = u.clone();
                let other_pairs: Vec<(String, String)> = no_ssl
                    .query_pairs()
                    .into_owned()
                    .filter(|(k, _)| k != "sslmode")
                    .collect();
                no_ssl.query_pairs_mut().clear();
                {
                    let mut qp = no_ssl.query_pairs_mut();
                    for (k, v) in other_pairs {
                        qp.append_pair(&k, &v);
                    }
                    qp.append_pair("sslmode", "disable");
                }
                candidate_urls.push(no_ssl.to_string());
            }

            for url in candidate_urls {
                // Probe multiple fresh connections to avoid VIP/LB flakiness.
                let mut all_rw = true;
                let mut server_addr_seen: Option<String> = None;
                for probe_idx in 1..=2 {
                    let pool = match tokio::time::timeout(
                        std::time::Duration::from_secs(20),
                        TaskUtil::create_pg_conn_pool(&url, auth, 1, false, false),
                    )
                    .await
                    {
                        Ok(Ok(pool)) => pool,
                        Ok(Err(e)) => {
                            println!(
                                "skip gaussdb candidate (connect failed): {}, error: {}",
                                url, e
                            );
                            all_rw = false;
                            break;
                        }
                        Err(_) => {
                            println!(
                                "skip gaussdb candidate (connect timeout#{}) : {}",
                                probe_idx, url
                            );
                            all_rw = false;
                            break;
                        }
                    };

                    // GaussDB HA promotion can temporarily expose nodes that are not in recovery
                    // but still reject writes with "read-only transaction". Filter those out so
                    // DDL/DML in tests runs on a truly writable coordinator.
                    //
                    // NOTE: Some GaussDB distributions don't implement `current_setting(text, boolean)`,
                    // so we probe via `pg_settings` instead.
                    let probe_res: Result<(bool, String, Option<String>, Option<String>), sqlx::Error> =
                        match tokio::time::timeout(
                            std::time::Duration::from_secs(10),
                            sqlx::query_as(
                                "SELECT \
                                    pg_is_in_recovery()::bool, \
                                    inet_server_addr()::text, \
                                    (SELECT setting FROM pg_settings WHERE name='transaction_read_only')::text, \
                                    (SELECT setting FROM pg_settings WHERE name='default_transaction_read_only')::text",
                            )
                            .fetch_one(&pool),
                        )
                        .await
                        {
                            Ok(v) => v,
                            Err(_) => Err(sqlx::Error::PoolTimedOut),
                        };
                    let (in_recovery, server_addr, transaction_ro, default_transaction_ro) =
                        match probe_res {
                            Ok(v) => v,
                            Err(e) => {
                                println!(
                                    "skip gaussdb candidate (probe failed#{}) : {}, error: {}",
                                    probe_idx, url, e
                                );
                                pool.close().await;
                                all_rw = false;
                                break;
                            }
                        };
                    // `pg_is_in_recovery()` is not a reliable indicator for GaussDB coordinators in
                    // all deployments (it can be `true` while the node is still writable, or vice
                    // versa). We still log it for visibility, but rely on the DDL+DML probe below
                    // as the final truth for write capability.
                    if in_recovery {
                        println!(
                            "gaussdb candidate reports in_recovery=true, will still probe write: {}",
                            url
                        );
                    }
                    let is_on = |v: &Option<String>| {
                        v.as_deref().is_some_and(|s| s.eq_ignore_ascii_case("on"))
                    };
                    if is_on(&transaction_ro) || is_on(&default_transaction_ro) {
                        println!(
                            "skip gaussdb candidate (read-only): {} (transaction_read_only={:?}, default_transaction_read_only={:?})",
                            url, transaction_ro, default_transaction_ro
                        );
                        pool.close().await;
                        all_rw = false;
                        break;
                    }

                    // Final guard: even if the node claims it's not in recovery, it can still be
                    // temporarily read-only during HA promotion. Verify we can run a small DDL in
                    // a transaction and roll it back, which is a stronger indicator of true
                    // write-capability than settings (and more reliable than temp tables).
                    let probe_tbl = format!(
                        "ape_dts_rw_probe_{}_{}_{}",
                        std::process::id(),
                        probe_idx,
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_else(|_| std::time::Duration::from_secs(0))
                            .as_nanos()
                    );
                    let create_sql = format!("CREATE TABLE public.{} (id int4)", probe_tbl);
                    let insert_sql = format!("INSERT INTO public.{} (id) VALUES (1)", probe_tbl);

                    let ddl_probe = async {
                        sqlx::query("BEGIN").execute(&pool).await?;
                        let create_res = sqlx::query(&create_sql).execute(&pool).await;
                        let insert_res = if create_res.is_ok() {
                            Some(sqlx::query(&insert_sql).execute(&pool).await)
                        } else {
                            None
                        };
                        let rollback_res = sqlx::query("ROLLBACK").execute(&pool).await;
                        match create_res {
                            Ok(_) => match insert_res
                                .expect("insert_res must exist when create_res is Ok")
                            {
                                Ok(_) => rollback_res.map(|_| ()),
                                Err(e) => {
                                    let _ = rollback_res;
                                    Err(e)
                                }
                            },
                            Err(e) => {
                                let _ = rollback_res;
                                Err(e)
                            }
                        }
                    };
                    match tokio::time::timeout(std::time::Duration::from_secs(10), ddl_probe).await
                    {
                        Ok(Ok(())) => {}
                        Ok(Err(e)) => {
                            println!(
                                "skip gaussdb candidate (ddl probe failed#{}) : {}, error: {}",
                                probe_idx, url, e
                            );
                            pool.close().await;
                            all_rw = false;
                            break;
                        }
                        Err(_) => {
                            println!(
                                "skip gaussdb candidate (ddl probe timeout#{}) : {}",
                                probe_idx, url
                            );
                            pool.close().await;
                            all_rw = false;
                            break;
                        }
                    }

                    pool.close().await;
                    if let Some(prev) = &server_addr_seen {
                        if prev != &server_addr {
                            println!(
                                "skip gaussdb candidate (routes to multiple server_addr): {} ({} -> {})",
                                url, prev, server_addr
                            );
                            all_rw = false;
                            break;
                        }
                    } else {
                        server_addr_seen = Some(server_addr);
                    }
                }

                if all_rw {
                    if let Some(addr) = &server_addr_seen {
                        println!("selected gaussdb rw url: {} (server_addr={})", url, addr);
                    } else {
                        println!("selected gaussdb rw url: {}", url);
                    }
                    return Ok(Some(url));
                }
            }
        }

        Ok(None)
    }

    async fn maybe_create_gaussdb_rw_pg_pool(
        base_url: &str,
        auth: &dt_common::config::connection_auth_config::ConnectionAuthConfig,
    ) -> anyhow::Result<Option<(String, Pool<Postgres>)>> {
        let candidates = match env::var("gaussdb_pg_candidate_hosts") {
            Ok(v) => v,
            Err(_) => return Ok(None),
        };

        let Some(url) = Self::resolve_gaussdb_rw_url(base_url, auth, &candidates).await? else {
            return Ok(None);
        };

        let pool = TaskUtil::create_pg_conn_pool(&url, auth, 1, false, false).await?;
        Ok(Some((url, pool)))
    }

    async fn create_gaussdb_rw_pg_pool_with_wait(
        base_url: &str,
        auth: &dt_common::config::connection_auth_config::ConnectionAuthConfig,
        max_wait_millis: u64,
    ) -> anyhow::Result<Option<(String, Pool<Postgres>)>> {
        let start = std::time::Instant::now();
        while start.elapsed().as_millis() < max_wait_millis as u128 {
            if let Some(v) = Self::maybe_create_gaussdb_rw_pg_pool(base_url, auth).await? {
                return Ok(Some(v));
            }
            TimeUtil::sleep_millis(500).await;
        }
        Ok(None)
    }

    pub async fn close(&self) -> anyhow::Result<()> {
        if let Some(pool) = &self.src_conn_pool_mysql {
            pool.close().await;
        }
        if let Some(pool) = &self.dst_conn_pool_mysql {
            pool.close().await;
        }
        if let Some(pool) = &self.src_conn_pool_pg {
            pool.close().await;
        }
        if let Some(pool) = &self.dst_conn_pool_pg {
            pool.close().await;
        }
        Ok(())
    }

    pub async fn get_dst_mysql_version(&self) -> String {
        if let Some(conn_pool) = &self.dst_conn_pool_mysql {
            let meta_manager = MysqlMetaManager::new(conn_pool.clone()).await.unwrap();
            return meta_manager.meta_fetcher.version;
        }
        String::new()
    }

    pub async fn run_snapshot_test(&self, compare_data: bool) -> anyhow::Result<()> {
        // prepare src and dst tables
        self.execute_prepare_sqls().await?;
        self.execute_test_sqls().await?;

        // start task
        self.base.start_task().await?;

        // compare data
        let (src_db_tbs, dst_db_tbs) = self.get_compare_db_tbs()?;
        if compare_data {
            assert!(self.compare_data_for_tbs(&src_db_tbs, &dst_db_tbs).await?)
        }
        Ok(())
    }

    pub async fn run_ddl_test(&self, start_millis: u64, parse_millis: u64) -> anyhow::Result<()> {
        self.execute_prepare_sqls().await?;

        self.update_cdc_task_config(start_millis, parse_millis)
            .await?;
        let task = self.base.spawn_task().await?;
        TimeUtil::sleep_millis(start_millis).await;

        self.execute_src_sqls(&self.base.src_test_sqls).await?;
        self.base.wait_task_finish(&task).await?;

        let (src_db_tbs, dst_db_tbs) = self.get_compare_db_tbs()?;
        assert!(
            self.compare_data_for_tbs_ignore_filtered(&src_db_tbs, &dst_db_tbs)
                .await?
        );
        Ok(())
    }

    pub async fn run_ddl_meta_center_test(
        &mut self,
        start_millis: u64,
        parse_millis: u64,
    ) -> anyhow::Result<()> {
        self.execute_prepare_sqls().await?;
        self.update_cdc_task_config(start_millis, parse_millis)
            .await?;

        self.execute_src_sqls(&self.base.src_test_sqls).await?;

        // run_ddl_test: start cdc task BEFORE src_test_sqls executed
        // run_ddl_meta_center_test: start cdc task AFTER src_test_sqls executed
        let task: JoinHandle<()> = self.base.spawn_task().await?;
        self.base.wait_task_finish(&task).await?;

        // compare table data
        let (src_db_tbs, dst_db_tbs) = self.get_compare_db_tbs()?;
        assert!(
            self.compare_data_for_tbs_ignore_filtered(&src_db_tbs, &dst_db_tbs)
                .await?
        );

        // compare show create table
        let src_fetcher = MysqlStructCheckFetcher {
            conn_pool: self.src_conn_pool_mysql.as_mut().unwrap().clone(),
        };
        let meta_center_fetcher = MysqlStructCheckFetcher {
            conn_pool: self.meta_center_pool_mysql.as_mut().unwrap().clone(),
        };

        let filtered_db_tbs = self.get_filtered_db_tbs();
        for i in 0..src_db_tbs.len() {
            if filtered_db_tbs.contains(&src_db_tbs[i]) {
                continue;
            }
            let src_ddl_sql = src_fetcher
                .fetch_table(&src_db_tbs[i].0, &src_db_tbs[i].1)
                .await;
            let meta_center_ddl_sql = meta_center_fetcher
                .fetch_table(&dst_db_tbs[i].0, &dst_db_tbs[i].1)
                .await;
            assert_eq!(src_ddl_sql, meta_center_ddl_sql);
        }
        Ok(())
    }

    pub async fn run_dcl_test(&self, start_millis: u64, parse_millis: u64) -> anyhow::Result<()> {
        self.execute_prepare_sqls().await?;

        self.update_cdc_task_config(start_millis, parse_millis)
            .await?;
        let task = self.base.spawn_task().await?;
        TimeUtil::sleep_millis(start_millis).await;

        self.execute_src_sqls(&self.base.src_test_sqls).await?;
        self.base.wait_task_finish(&task).await?;

        self.dcl_check_sql_execution().await?;

        Ok(())
    }

    pub async fn dcl_check_sql_execution(&self) -> anyhow::Result<()> {
        self.check_sql_execution("check_with_succeed.txt", true)
            .await?;
        self.check_sql_execution("check_with_failed.txt", false)
            .await?;
        Ok(())
    }

    async fn check_sql_execution(
        &self,
        filename: &str,
        expect_success: bool,
    ) -> anyhow::Result<()> {
        let file_path = format!("{}/{}", &self.base.test_dir, filename);
        if !BaseTestRunner::check_path_exists(&file_path) {
            return Ok(());
        }

        let lines = BaseTestRunner::load_file(&file_path);
        let mut mysql_pool: Option<Pool<MySql>> = None;
        let mut pg_pool: Option<Pool<Postgres>> = None;
        let is_mysql = self.dst_conn_pool_mysql.is_some();

        for line in lines {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            if line.starts_with("--") {
                let parts: Vec<&str> = line[2..].trim().split('/').collect();
                let current_user = parts[0].trim().to_string();
                let current_pwd = parts[1].trim().to_string();

                if let Some(old_pool) = mysql_pool.take() {
                    old_pool.close().await;
                }
                if let Some(old_pool) = pg_pool.take() {
                    old_pool.close().await;
                }

                let url = &self.config.sinker_basic.url;
                let conn_str = url.replace(
                    &url[url.find("://").unwrap() + 3..url.find('@').unwrap()],
                    &format!("{}:{}", current_user, current_pwd),
                );

                if is_mysql {
                    let pool_connect = Pool::<MySql>::connect(&conn_str).await;
                    match pool_connect {
                        Ok(p) => {
                            mysql_pool = Some(p);
                        }
                        Err(e) => {
                            if expect_success {
                                assert!(
                                    false,
                                    "MySQL pool connect failed: {} with user={}, password={}, but expect success",
                                    e, current_user, current_pwd
                                );
                            }
                        }
                    }
                } else {
                    let pool_connect = Pool::<Postgres>::connect(&conn_str).await;
                    match pool_connect {
                        Ok(p) => {
                            pg_pool = Some(p);
                        }
                        Err(e) => {
                            if expect_success {
                                assert!(
                                    false,
                                    "PostgreSQL pool connect failed: {} with user={}, password={}, but expect success",
                                    e, current_user, current_pwd
                                );
                            }
                        }
                    }
                }
                continue;
            }

            if is_mysql {
                if let Some(ref pool) = mysql_pool {
                    let query = sqlx::query(line);
                    let result = query.execute(pool).await;

                    if expect_success {
                        assert!(
                            result.is_ok(),
                            "Expected success but got error: {:?}",
                            result.err()
                        );
                    } else {
                        assert!(
                            result.is_err(),
                            "Expected error but got success, sql: {}",
                            line
                        );
                    }
                }
            } else {
                if let Some(ref pool) = pg_pool {
                    let query = sqlx::query(line);
                    let result = query.execute(pool).await;

                    if expect_success {
                        assert!(
                            result.is_ok(),
                            "Expected success but got error: {:?}",
                            result.err()
                        );
                    } else {
                        assert!(
                            result.is_err(),
                            "Expected error but got success, sql: {}",
                            line
                        );
                    }
                }
            }
        }

        if let Some(pool) = mysql_pool {
            pool.close().await;
        }
        if let Some(pool) = pg_pool {
            pool.close().await;
        }

        Ok(())
    }

    pub async fn run_cdc_test(&self, start_millis: u64, parse_millis: u64) -> anyhow::Result<()> {
        // prepare src and dst tables
        self.execute_prepare_sqls().await?;

        // start task
        let task = self.spawn_cdc_task(start_millis, parse_millis).await?;

        let res = self.execute_test_sqls_and_compare(parse_millis).await;
        // Always abort the task to avoid leaking long-running CDC tasks across retries.
        // Other runners (mongo/redis) do the same.
        let _ = self.base.abort_task(&task).await;
        res
    }

    pub async fn run_heartbeat_test(
        &self,
        _start_millis: u64,
        parse_millis: u64,
    ) -> anyhow::Result<()> {
        let config = TaskConfig::new(&self.base.task_config_file).unwrap();
        let (heartbeat_tb, db_type) = match config.extractor {
            ExtractorConfig::PgCdc { heartbeat_tb, .. } => (heartbeat_tb.clone(), DbType::Pg),
            ExtractorConfig::GaussDBCdc { heartbeat_tb, .. } => {
                (heartbeat_tb.clone(), DbType::GaussDBPg)
            }
            ExtractorConfig::MysqlCdc { heartbeat_tb, .. } => (heartbeat_tb.clone(), DbType::Mysql),
            _ => (String::new(), DbType::Mysql),
        };

        let tokens = ConfigTokenParser::parse(
            &heartbeat_tb,
            &['.'],
            &TokenEscapePair::from_char_pairs(SqlUtil::get_escape_pairs(&db_type)),
        );
        let db_tb = (tokens[0].clone(), tokens[1].clone());

        self.execute_prepare_sqls().await?;

        // start task
        self.update_cdc_task_config(0, parse_millis).await?;
        let task = self.base.spawn_task().await?;
        self.base.wait_task_finish(&task).await.unwrap();

        let src_data = self.fetch_data(&db_tb, SRC).await?;
        assert_eq!(src_data.len(), 1);
        Ok(())
    }

    pub async fn update_cdc_task_config(
        &self,
        start_millis: u64,
        parse_millis: u64,
    ) -> anyhow::Result<()> {
        let duration = Duration::try_milliseconds((start_millis + parse_millis) as i64).unwrap();
        let end_time_utc = (Utc::now() + duration).format(UTC_FORMAT).to_string();
        let (binlog_file, binlog_position) = self.fetch_mysql_binlog_position().await.unwrap();
        let binlog_position = binlog_position.to_string();
        let update_configs = vec![
            ("extractor", "end_time_utc", end_time_utc.as_str()),
            ("extractor", "binlog_filename", binlog_file.as_str()),
            ("extractor", "binlog_position", binlog_position.as_str()),
        ];
        TestConfigUtil::update_task_config_2(
            &self.base.task_config_file,
            &self.base.task_config_file,
            &update_configs,
        );
        Ok(())
    }

    pub async fn spawn_cdc_task(
        &self,
        start_millis: u64,
        parse_millis: u64,
    ) -> anyhow::Result<JoinHandle<()>> {
        // A previous attempt can leave a long-running GaussDB CDC task behind (replication slot
        // stays active). Starting a new task would then fail with "replication slot is already
        // active" and/or make slot-active readiness checks meaningless.
        let config = TaskConfig::new(&self.base.task_config_file).unwrap();
        if let ExtractorConfig::GaussDBCdc { ref slot_name, .. } = config.extractor {
            self.wait_gaussdb_cdc_slot_inactive(slot_name, start_millis)
                .await?;
        }

        // start task
        let total_parse_millis = self.get_total_parse_millis(parse_millis);
        self.update_cdc_task_config(start_millis, total_parse_millis)
            .await?;
        let task = self.base.spawn_task().await?;
        // For GaussDB CDC, task startup (slot create + START_REPLICATION) can be slow. If we
        // execute DML before the replication slot becomes active, those changes will be missed.
        // Wait (up to start_millis) for the slot to become active, otherwise fall back to a fixed sleep.
        let config = TaskConfig::new(&self.base.task_config_file).unwrap();
        match config.extractor {
            ExtractorConfig::GaussDBCdc { ref slot_name, .. } => {
                if let Err(e) = self
                    .wait_gaussdb_cdc_slot_active(slot_name, start_millis)
                    .await
                {
                    let _ = self.base.abort_task(&task).await;
                    return Err(e);
                }
            }
            _ => TimeUtil::sleep_millis(start_millis).await,
        }
        Ok(task)
    }

    async fn wait_gaussdb_cdc_slot_active(
        &self,
        slot_name: &str,
        max_wait_millis: u64,
    ) -> anyhow::Result<()> {
        if env::var("gaussdb_pg_candidate_hosts").is_err() {
            let Some(pool) = &self.src_conn_pool_pg else {
                return Ok(());
            };

            let start = std::time::Instant::now();
            while start.elapsed().as_millis() < max_wait_millis as u128 {
                match sqlx::query_scalar::<_, bool>(
                    "SELECT active FROM pg_catalog.pg_replication_slots WHERE slot_name = $1",
                )
                .bind(slot_name)
                .fetch_optional(pool)
                .await
                {
                    Ok(Some(true)) => return Ok(()),
                    Ok(_) => {}
                    Err(_) => {}
                }
                TimeUtil::sleep_millis(500).await;
            }

            anyhow::bail!(
                "operation timed out: gaussdb cdc slot '{}' did not become active within {} ms",
                slot_name,
                max_wait_millis
            )
        }

        let base_url = &self.config.extractor_basic.url;
        let auth = &self.config.extractor_basic.connection_auth;

        let start = std::time::Instant::now();
        let mut last_url: Option<String> = None;
        let mut pool: Option<Pool<Postgres>> = None;
        let mut last_resolve_at = std::time::Instant::now();

        while start.elapsed().as_millis() < max_wait_millis as u128 {
            if pool.is_none() {
                match Self::maybe_create_gaussdb_rw_pg_pool(base_url, auth).await? {
                    Some((url, p)) => {
                        last_url = Some(url);
                        pool = Some(p);
                        last_resolve_at = std::time::Instant::now();
                    }
                    None => {
                        TimeUtil::sleep_millis(500).await;
                        continue;
                    }
                }
            }

            let p = pool.as_ref().unwrap();
            match sqlx::query_scalar::<_, bool>(
                "SELECT active FROM pg_catalog.pg_replication_slots WHERE slot_name = $1",
            )
            .bind(slot_name)
            .fetch_optional(p)
            .await
            {
                Ok(Some(true)) => {
                    if let Some(tmp) = pool.take() {
                        tmp.close().await;
                    }
                    return Ok(());
                }
                Ok(_) => {}
                Err(_) => {
                    if let Some(tmp) = pool.take() {
                        tmp.close().await;
                    }
                }
            }

            if last_resolve_at.elapsed().as_secs() >= 5 {
                if let Some(tmp) = pool.take() {
                    tmp.close().await;
                }
            }

            TimeUtil::sleep_millis(500).await;
        }

        if let Some(tmp) = pool {
            tmp.close().await;
        }

        anyhow::bail!(
            "operation timed out: gaussdb cdc slot '{}' did not become active within {} ms (last_url={:?})",
            slot_name,
            max_wait_millis,
            last_url
        )
    }

    async fn wait_gaussdb_cdc_slot_inactive(
        &self,
        slot_name: &str,
        max_wait_millis: u64,
    ) -> anyhow::Result<()> {
        if env::var("gaussdb_pg_candidate_hosts").is_err() {
            let Some(pool) = &self.src_conn_pool_pg else {
                return Ok(());
            };

            let start = std::time::Instant::now();
            while start.elapsed().as_millis() < max_wait_millis as u128 {
                match sqlx::query_scalar::<_, bool>(
                    "SELECT active FROM pg_catalog.pg_replication_slots WHERE slot_name = $1",
                )
                .bind(slot_name)
                .fetch_optional(pool)
                .await
                {
                    Ok(Some(true)) => {}
                    Ok(_) => return Ok(()),
                    Err(_) => {}
                }
                TimeUtil::sleep_millis(500).await;
            }

            anyhow::bail!(
                "operation timed out: gaussdb cdc slot '{}' did not become inactive within {} ms",
                slot_name,
                max_wait_millis
            )
        }

        let base_url = &self.config.extractor_basic.url;
        let auth = &self.config.extractor_basic.connection_auth;

        let start = std::time::Instant::now();
        let mut last_url: Option<String> = None;
        let mut pool: Option<Pool<Postgres>> = None;
        let mut last_resolve_at = std::time::Instant::now();

        while start.elapsed().as_millis() < max_wait_millis as u128 {
            if pool.is_none() {
                match Self::maybe_create_gaussdb_rw_pg_pool(base_url, auth).await? {
                    Some((url, p)) => {
                        last_url = Some(url);
                        pool = Some(p);
                        last_resolve_at = std::time::Instant::now();
                    }
                    None => {
                        TimeUtil::sleep_millis(500).await;
                        continue;
                    }
                }
            }

            let p = pool.as_ref().unwrap();
            match sqlx::query_scalar::<_, bool>(
                "SELECT active FROM pg_catalog.pg_replication_slots WHERE slot_name = $1",
            )
            .bind(slot_name)
            .fetch_optional(p)
            .await
            {
                Ok(Some(true)) => {}
                Ok(_) => {
                    if let Some(tmp) = pool.take() {
                        tmp.close().await;
                    }
                    return Ok(());
                }
                Err(_) => {
                    if let Some(tmp) = pool.take() {
                        tmp.close().await;
                    }
                }
            }

            if last_resolve_at.elapsed().as_secs() >= 5 {
                if let Some(tmp) = pool.take() {
                    tmp.close().await;
                }
            }

            TimeUtil::sleep_millis(500).await;
        }

        if let Some(tmp) = pool {
            tmp.close().await;
        }

        anyhow::bail!(
            "operation timed out: gaussdb cdc slot '{}' did not become inactive within {} ms (last_url={:?})",
            slot_name,
            max_wait_millis,
            last_url
        )
    }

    fn get_total_parse_millis(&self, parse_millis: u64) -> u64 {
        let (src_insert_sqls, src_update_sqls, src_delete_sqls) =
            Self::split_dml_sqls(&self.base.src_test_sqls);
        // parse_millis * 2 for: time to parse binlog + time to compare data
        (!src_insert_sqls.is_empty() as u64
            + !src_update_sqls.is_empty() as u64
            + !src_delete_sqls.is_empty() as u64)
            * parse_millis
            * 2
    }

    async fn compare_data_for_tbs_with_retry(
        &self,
        stage: &str,
        src_db_tbs: &[(String, String)],
        dst_db_tbs: &[(String, String)],
        max_wait_millis: u64,
    ) -> anyhow::Result<()> {
        let started = std::time::Instant::now();
        let mut backoff_millis: u64 = 500;

        // First compare attempt (gives us an error to report if we time out).
        let mut last_err = match self.compare_data_for_tbs(src_db_tbs, dst_db_tbs).await {
            Ok(_) => return Ok(()),
            Err(e) => e,
        };

        while started.elapsed().as_millis() < max_wait_millis as u128 {
            TimeUtil::sleep_millis(backoff_millis).await;
            backoff_millis = std::cmp::min(backoff_millis.saturating_mul(2), 2000);

            match self.compare_data_for_tbs(src_db_tbs, dst_db_tbs).await {
                Ok(_) => return Ok(()),
                Err(e) => last_err = e,
            }
        }

        anyhow::bail!(
            "compare tb data failed after {} ms (stage={}): {:#}",
            max_wait_millis,
            stage,
            last_err
        );
    }

    pub async fn execute_test_sqls_and_compare(&self, parse_millis: u64) -> anyhow::Result<()> {
        // load dml sqls
        let (src_insert_sqls, src_update_sqls, src_delete_sqls) =
            Self::split_dml_sqls(&self.base.src_test_sqls);

        let (src_db_tbs, dst_db_tbs) = self.get_compare_db_tbs()?;

        // Execute DML as quickly as possible, then do a single final compare. This reduces the
        // time window between stages (insert -> update -> delete), which helps on flappy GaussDB
        // HA environments where the primary can switch mid-test.
        if !src_insert_sqls.is_empty() {
            self.execute_src_sqls(&src_insert_sqls).await?;
        }

        if !src_update_sqls.is_empty() {
            self.execute_src_sqls(&src_update_sqls).await?;
        }

        if !src_delete_sqls.is_empty() {
            self.execute_src_sqls(&src_delete_sqls).await?;
        }

        let stage_count = !src_insert_sqls.is_empty() as u64
            + !src_update_sqls.is_empty() as u64
            + !src_delete_sqls.is_empty() as u64;
        if stage_count == 0 {
            return Ok(());
        }

        self.compare_data_for_tbs_with_retry(
            "dml",
            &src_db_tbs,
            &dst_db_tbs,
            stage_count * parse_millis,
        )
        .await?;
        Ok(())
    }

    pub fn split_dml_sqls(dml_sqls: &[String]) -> (Vec<String>, Vec<String>, Vec<String>) {
        let mut insert_sqls = Vec::new();
        let mut update_sqls = Vec::new();
        let mut delete_sqls = Vec::new();

        for sql in dml_sqls.iter() {
            if sql.to_lowercase().starts_with("insert") {
                insert_sqls.push(sql.clone());
            } else if sql.to_lowercase().starts_with("update") {
                update_sqls.push(sql.clone());
            } else {
                delete_sqls.push(sql.clone());
            }
        }
        (insert_sqls, update_sqls, delete_sqls)
    }

    pub async fn execute_test_sqls(&self) -> anyhow::Result<()> {
        self.execute_src_sqls(&self.base.src_test_sqls).await?;
        self.execute_dst_sqls(&self.base.dst_test_sqls).await
    }

    pub async fn execute_prepare_sqls(&self) -> anyhow::Result<()> {
        self.execute_src_sqls(&self.base.src_prepare_sqls).await?;
        self.execute_dst_sqls(&self.base.dst_prepare_sqls).await?;
        self.execute_meta_center_prepare_sqls(&self.base.meta_center_prepare_sqls)
            .await?;

        // migrate database/table structures to target if needed
        if !self.base.struct_task_config_file.is_empty() {
            TaskRunner::new(&self.base.struct_task_config_file)?
                .start_task()
                .await?;
        }
        Ok(())
    }

    pub async fn execute_meta_center_prepare_sqls(&self, sqls: &Vec<String>) -> anyhow::Result<()> {
        if let Some(pool) = &self.meta_center_pool_mysql {
            RdbUtil::execute_sqls_mysql(pool, sqls).await?
        }
        Ok(())
    }

    pub async fn execute_clean_sqls(&self) -> anyhow::Result<()> {
        self.execute_src_sqls(&self.base.src_clean_sqls).await?;
        self.execute_dst_sqls(&self.base.dst_clean_sqls).await
    }

    pub async fn execute_src_sqls(&self, sqls: &Vec<String>) -> anyhow::Result<()> {
        if let Some(pool) = &self.src_conn_pool_mysql {
            RdbUtil::execute_sqls_mysql(pool, sqls).await?;
        }

        if let Some(pool) = &self.src_conn_pool_pg {
            RdbUtil::execute_sqls_pg(pool, sqls).await?;
        }
        Ok(())
    }

    pub async fn execute_dst_sqls(&self, sqls: &Vec<String>) -> anyhow::Result<()> {
        if let Some(pool) = &self.dst_conn_pool_mysql {
            RdbUtil::execute_sqls_mysql(pool, sqls).await?;
        }

        if let Some(pool) = &self.dst_conn_pool_pg {
            RdbUtil::execute_sqls_pg(pool, sqls).await?;
        }
        Ok(())
    }

    pub async fn compare_data_for_tbs_ignore_filtered(
        &self,
        src_db_tbs: &[(String, String)],
        dst_db_tbs: &[(String, String)],
    ) -> anyhow::Result<bool> {
        let filtered_db_tbs = self.get_filtered_db_tbs();
        for i in 0..src_db_tbs.len() {
            if filtered_db_tbs.contains(&src_db_tbs[i]) {
                continue;
            }
            if !self.compare_tb_data(&src_db_tbs[i], &dst_db_tbs[i]).await? {
                anyhow::bail!(
                    "compare tb data failed, src_tb: {:?}, dst_tb: {:?}",
                    src_db_tbs[i],
                    dst_db_tbs[i]
                );
            }
        }
        Ok(true)
    }

    pub async fn compare_data_for_tbs(
        &self,
        src_db_tbs: &[(String, String)],
        dst_db_tbs: &[(String, String)],
    ) -> anyhow::Result<bool> {
        let filtered_db_tbs = self.get_filtered_db_tbs();
        for i in 0..src_db_tbs.len() {
            if filtered_db_tbs.contains(&src_db_tbs[i]) {
                let dst_data = self.fetch_data(&dst_db_tbs[i], DST).await?;
                if !dst_data.is_empty() {
                    println!("tb: {:?} is filtered but dst is not empty", dst_db_tbs[i]);
                    anyhow::bail!("filtered tb has dst rows, dst_tb: {:?}", dst_db_tbs[i]);
                }
            } else {
                if !self.compare_tb_data(&src_db_tbs[i], &dst_db_tbs[i]).await? {
                    anyhow::bail!(
                        "compare tb data failed, src_tb: {:?}, dst_tb: {:?}",
                        src_db_tbs[i],
                        dst_db_tbs[i]
                    );
                }
            }
        }
        Ok(true)
    }

    async fn compare_tb_data(
        &self,
        src_db_tb: &(String, String),
        dst_db_tb: &(String, String),
    ) -> anyhow::Result<bool> {
        let src_data = self.fetch_data(src_db_tb, SRC).await?;
        let dst_data = self.fetch_data(dst_db_tb, DST).await?;
        println!(
            "comparing row data for src_tb: {:?}, dst_tb: {:?}, src_data count: {}, dst_data count: {}",
            src_db_tb,
            dst_db_tb,
            src_data.len(),
            dst_data.len(),
        );

        if !self.compare_row_data(&src_data, &dst_data, src_db_tb) {
            println!(
                "compare tb data failed, src_tb: {:?}, dst_tb: {:?}",
                src_db_tb, dst_db_tb,
            );
            return Ok(false);
        }
        Ok(true)
    }

    fn compare_row_data(
        &self,
        src_data: &[RowData],
        dst_data: &[RowData],
        src_db_tb: &(String, String),
    ) -> bool {
        if src_data.len() != dst_data.len() {
            println!(
                "row count mismatch: src={}, dst={}",
                src_data.len(),
                dst_data.len()
            );
            return false;
        }

        let src_db_type = self.get_db_type(SRC);
        let dst_db_type = self.get_db_type(DST);

        // router: col_map
        let col_map = self.router.get_col_map(&src_db_tb.0, &src_db_tb.1);
        // filter: ignore_cols
        let ignore_cols = self.filter.get_ignore_cols(&src_db_tb.0, &src_db_tb.1);

        if self.unordered_compare {
            // Unordered comparison: use multiset matching for tables without primary key
            self.compare_row_data_unordered(
                src_data,
                dst_data,
                col_map,
                ignore_cols,
                &src_db_type,
                &dst_db_type,
            )
        } else {
            // Ordered comparison: compare rows by index
            self.compare_row_data_ordered(
                src_data,
                dst_data,
                col_map,
                ignore_cols,
                &src_db_type,
                &dst_db_type,
            )
        }
    }

    fn compare_row_data_ordered(
        &self,
        src_data: &[RowData],
        dst_data: &[RowData],
        col_map: Option<&HashMap<String, String>>,
        ignore_cols: Option<&HashSet<String>>,
        src_db_type: &DbType,
        dst_db_type: &DbType,
    ) -> bool {
        for i in 0..src_data.len() {
            let (src_col_values, dst_col_values) =
                match (src_data[i].require_after(), dst_data[i].require_after()) {
                    (Ok(src), Ok(dst)) => (src, dst),
                    _ => return false,
                };

            for (src_col, src_col_value) in src_col_values {
                let dst_col = if let Some(col_map) = col_map {
                    col_map.get(src_col).unwrap_or(src_col)
                } else {
                    src_col
                };

                let dst_col_value = dst_col_values.get(dst_col).unwrap();

                // ignored cols were NOT synced to target
                if ignore_cols.is_some_and(|cols| cols.contains(src_col)) {
                    assert_eq!(*dst_col_value, ColValue::None);
                    continue;
                }

                // TODO
                // issue: https://github.com/apecloud/foxlake/issues/2108
                // sqlx will execute: "SET time_zone='+00:00',NAMES utf8mb4 COLLATE utf8mb4_unicode_ci;"
                // to initialize each connection.
                // but it doesn't work on Foxlake
                if matches!(self.base.get_config().sinker, SinkerConfig::Foxlake { .. })
                    && matches!(dst_col_value, ColValue::Timestamp(..))
                {
                    continue;
                }

                println!(
                    "row index: {}, col: {}, src_col_value: {:?}, dst_col_value: {:?}",
                    i, src_col, src_col_value, dst_col_value
                );

                if Self::compare_col_value(src_col_value, dst_col_value, src_db_type, dst_db_type) {
                    continue;
                }
                return false;
            }
        }
        true
    }

    // Unordered comparison: use multiset matching for tables without primary key
    // or with NULL values in unique keys
    fn compare_row_data_unordered(
        &self,
        src_data: &[RowData],
        dst_data: &[RowData],
        col_map: Option<&HashMap<String, String>>,
        ignore_cols: Option<&HashSet<String>>,
        src_db_type: &DbType,
        dst_db_type: &DbType,
    ) -> bool {
        // Normalize rows to comparable strings and count occurrences
        let mut dst_row_counts: HashMap<String, usize> = HashMap::new();

        // Build destination row counts
        for (i, row) in dst_data.iter().enumerate() {
            let dst_col_values = match row.require_after() {
                Ok(v) => v,
                Err(_) => {
                    println!("failed to get dst row {} after values", i);
                    return false;
                }
            };

            let row_key =
                self.normalize_row(dst_col_values, col_map, ignore_cols, dst_db_type, false);
            *dst_row_counts.entry(row_key).or_insert(0) += 1;
        }

        // Compare multisets using flexible column value comparison
        // Try to match each src row to a dst row
        let mut matched_dst_rows: HashMap<String, usize> = HashMap::new();

        for (i, row) in src_data.iter().enumerate() {
            let src_col_values = match row.require_after() {
                Ok(v) => v,
                Err(_) => return false,
            };

            // Try to find a matching row in dst_data
            let mut found_match = false;
            for dst_row in dst_data.iter() {
                let dst_col_values = match dst_row.require_after() {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                // Check how many times this dst row has been matched
                let dst_key =
                    self.normalize_row(dst_col_values, col_map, ignore_cols, dst_db_type, false);
                let already_matched = *matched_dst_rows.get(&dst_key).unwrap_or(&0);
                let dst_available = *dst_row_counts.get(&dst_key).unwrap_or(&0);

                if already_matched >= dst_available {
                    continue;
                }

                if self.compare_single_row(
                    src_col_values,
                    dst_col_values,
                    col_map,
                    ignore_cols,
                    src_db_type,
                    dst_db_type,
                ) {
                    *matched_dst_rows.entry(dst_key).or_insert(0) += 1;
                    found_match = true;
                    break;
                }
            }

            if !found_match {
                println!("no matching dst row found for src row {}", i);
                // Print the src row for debugging
                for (col, val) in src_col_values {
                    println!("  src col: {}, value: {:?}", col, val);
                }
                return false;
            }
        }

        true
    }

    /// Normalize a row to a comparable string key
    fn normalize_row(
        &self,
        col_values: &HashMap<String, ColValue>,
        col_map: Option<&HashMap<String, String>>,
        ignore_cols: Option<&HashSet<String>>,
        _db_type: &DbType,
        is_src: bool,
    ) -> String {
        let mut parts: Vec<String> = Vec::new();

        // Get sorted column names for consistent ordering
        let mut cols: Vec<&String> = col_values.keys().collect();
        cols.sort();

        for col in cols {
            // Map column name if needed
            let effective_col = if is_src {
                if let Some(map) = col_map {
                    map.get(col).unwrap_or(col)
                } else {
                    col
                }
            } else {
                col
            };

            // Skip ignored columns
            if is_src && ignore_cols.is_some_and(|cols| cols.contains(col)) {
                continue;
            }
            if !is_src && ignore_cols.is_some_and(|cols| cols.contains(effective_col)) {
                continue;
            }

            let val = col_values.get(col).unwrap();
            parts.push(format!("{}={:?}", effective_col, val));
        }

        parts.join("|")
    }

    // Compare a single source row with a destination row
    fn compare_single_row(
        &self,
        src_col_values: &HashMap<String, ColValue>,
        dst_col_values: &HashMap<String, ColValue>,
        col_map: Option<&HashMap<String, String>>,
        ignore_cols: Option<&HashSet<String>>,
        src_db_type: &DbType,
        dst_db_type: &DbType,
    ) -> bool {
        for (src_col, src_col_value) in src_col_values {
            let dst_col = if let Some(map) = col_map {
                map.get(src_col).unwrap_or(src_col)
            } else {
                src_col
            };

            let dst_col_value = match dst_col_values.get(dst_col) {
                Some(v) => v,
                None => return false,
            };

            // ignored cols were NOT synced to target
            if ignore_cols.is_some_and(|cols| cols.contains(src_col)) {
                if *dst_col_value != ColValue::None {
                    return false;
                }
                continue;
            }

            // TODO
            // issue: https://github.com/apecloud/foxlake/issues/2108
            // sqlx will execute: "SET time_zone='+00:00',NAMES utf8mb4 COLLATE utf8mb4_unicode_ci;"
            // to initialize each connection.
            // but it doesn't work on Foxlake
            if matches!(self.base.get_config().sinker, SinkerConfig::Foxlake { .. })
                && matches!(dst_col_value, ColValue::Timestamp(..))
            {
                continue;
            }

            if !Self::compare_col_value(src_col_value, dst_col_value, src_db_type, dst_db_type) {
                return false;
            }
        }
        true
    }

    fn compare_col_value(
        src_col_value: &ColValue,
        dst_col_value: &ColValue,
        src_db_type: &DbType,
        dst_db_type: &DbType,
    ) -> bool {
        if src_col_value == dst_col_value {
            return true;
        }

        if src_col_value.is_nan() && dst_col_value.is_nan() {
            return true;
        }

        if src_db_type == dst_db_type {
            return false;
        }

        if src_col_value.to_option_string() == dst_col_value.to_option_string() {
            return true;
        }

        // different databases support different column types,
        // for example: we use Year in mysql, but INT in StarRocks,
        // so try to compare after both converted to string.
        match src_col_value {
            // mysql 00:00:00 == foxlake 00:00:00.000000
            ColValue::Time(_) => {
                DtNaiveTime::from_str(&src_col_value.to_string()).unwrap()
                    == DtNaiveTime::from_str(&dst_col_value.to_string()).unwrap()
            }
            ColValue::Date(_) => {
                TimeUtil::date_from_str(&src_col_value.to_string()).unwrap()
                    == TimeUtil::date_from_str(&dst_col_value.to_string()).unwrap()
            }
            ColValue::DateTime(_) => {
                TimeUtil::datetime_from_utc_str(&src_col_value.to_string()).unwrap()
                    == TimeUtil::datetime_from_utc_str(&dst_col_value.to_string()).unwrap()
            }
            ColValue::Timestamp(_) => {
                TimeUtil::datetime_from_utc_str(&src_col_value.to_string()).unwrap()
                    == TimeUtil::datetime_from_utc_str(&dst_col_value.to_string()).unwrap()
            }
            ColValue::Json2(src_v) => match dst_col_value {
                // for snapshot/cdc task: mysql -> starrocks
                // in src mysql, json_test.f_1 type == JSON -> ColValue::Json2
                // in dst starrocks, json_test.f_1 type == STRING/VARCHAR/CHAR -> ColValue::String
                ColValue::Json2(dst_v) | ColValue::String(dst_v) => {
                    serde_json::Value::from_str(src_v).unwrap()
                        == serde_json::Value::from_str(dst_v).unwrap()
                }
                // for snapshot task: mysql -> starrocks
                // in src mysql, json_test.f_1 type == JSON
                // INSERT INTO json_test VALUES (11, NULL),(12, 'NULL');
                // +-----+------+
                // | f_0 | f_1  |
                // +-----+------+
                // |  11 | NULL |  ->  ColValue::None  ->  serde_json::Value::Null
                // |  12 | null |  ->  ColValue::Json2("null")  ->  serde_json::Value::Null
                // +-----+------+
                // in dst starrocks, json_test.f_1 type == JSON,
                // +-----+------+
                // | f_0 | f_1  |
                // +-----+------+
                // |  11 | NULL |  ->  ColValue::None  ->  serde_json::Value::Null
                // |  12 | NULL |  ->  ColValue::None  ->  serde_json::Value::Null
                ColValue::None => serde_json::Value::from_str(src_v).unwrap().is_null(),
                _ => false,
            },
            ColValue::Bit(src_v) => match dst_col_value {
                // in dst starrocks, BIT is stored as BIGINT
                ColValue::LongLong(dst_v) => *src_v == *dst_v as u64,
                _ => false,
            },
            ColValue::Decimal(src_v) => match dst_col_value {
                // src_v = 30.00000, dst_v = 30.000000000
                ColValue::Decimal(dst_v) => {
                    BigDecimal::from_str(src_v) == BigDecimal::from_str(dst_v)
                }
                _ => false,
            },
            ColValue::Bool(src_v) => match dst_col_value {
                // src_v, postgres, boolean
                // dst_v, starrocks, boolean == tiny(1)
                ColValue::Tiny(dst_v) => *src_v == (*dst_v == 1),
                _ => false,
            },

            _ => src_col_value.to_option_string() == dst_col_value.to_option_string(),
        }
    }

    pub async fn fetch_data(
        &self,
        db_tb: &(String, String),
        from: &str,
    ) -> anyhow::Result<Vec<RowData>> {
        self.fetch_data_with_condition(db_tb, from, "").await
    }

    pub async fn fetch_data_with_condition(
        &self,
        db_tb: &(String, String),
        from: &str,
        condition: &str,
    ) -> anyhow::Result<Vec<RowData>> {
        let where_sql = self.get_where_sql(&db_tb.0, &db_tb.1, condition);
        let db_type = self.get_db_type(from);

        let (conn_pool_mysql, conn_pool_pg) = self.get_conn_pool(from);
        let data = if let Some(pool) = conn_pool_mysql {
            RdbUtil::fetch_data_mysql_compatible(pool, None, db_tb, &db_type, &where_sql).await?
        } else if let Some(pool) = conn_pool_pg {
            RdbUtil::fetch_data_pg(pool, None, db_tb, &where_sql).await?
        } else {
            Vec::new()
        };
        Ok(data)
    }

    pub fn parse_full_tb_name(full_tb_name: &str, db_type: &DbType) -> (String, String) {
        let escape_pairs = SqlUtil::get_escape_pairs(db_type);
        let tokens = ConfigTokenParser::parse(
            full_tb_name,
            &['.'],
            &TokenEscapePair::from_char_pairs(escape_pairs.clone()),
        );
        let (db, tb) = if tokens.len() > 1 {
            (tokens[0].to_string(), tokens[1].to_string())
        } else {
            (String::new(), full_tb_name.to_string())
        };

        (
            SqlUtil::unescape(&db, &escape_pairs[0]),
            SqlUtil::unescape(&tb, &escape_pairs[0]),
        )
    }

    /// get compare tbs
    #[allow(clippy::type_complexity)]
    pub fn get_compare_db_tbs(
        &self,
    ) -> anyhow::Result<(Vec<(String, String)>, Vec<(String, String)>)> {
        let db_type = self.get_db_type(SRC);
        let mut src_db_tbs =
            Self::get_compare_db_tbs_from_sqls(&db_type, &self.base.src_prepare_sqls)?;
        // since tables may be created/dropped in src_test.sql for ddl tests,
        // we also need to parse src_test.sql.
        src_db_tbs.extend_from_slice(&Self::get_compare_db_tbs_from_sqls(
            &db_type,
            &self.base.src_test_sqls,
        )?);

        let mut dst_db_tbs = vec![];
        for (db, tb) in src_db_tbs.iter() {
            let (dst_db, dst_tb) = self.router.get_tb_map(db, tb);
            dst_db_tbs.push((dst_db.into(), dst_tb.into()));
        }

        Ok((src_db_tbs, dst_db_tbs))
    }

    pub fn get_compare_db_tbs_from_sqls(
        db_type: &DbType,
        sqls: &[String],
    ) -> anyhow::Result<Vec<(String, String)>> {
        let mut db_tbs = vec![];
        let parser = DdlParser::new(db_type.to_owned());

        for sql in sqls.iter() {
            let sql = sql.trim().to_string();
            let tokens: Vec<&str> = sql.split(" ").collect();
            if tokens[0].trim().to_lowercase() != "create"
                || tokens[1].trim().to_lowercase() != "table"
            {
                continue;
            }

            let ddl = parser.parse(&sql).unwrap().unwrap();
            if ddl.ddl_type == DdlType::CreateTable {
                let (mut db, tb) = ddl.get_schema_tb();
                if db.is_empty() {
                    db = PUBLIC.to_string();
                }
                db_tbs.push((db, tb));
            }
        }

        Ok(db_tbs)
    }

    fn get_filtered_db_tbs(&self) -> HashSet<(String, String)> {
        let mut filtered_db_tbs = HashSet::new();
        let db_type = &self.get_db_type(SRC);
        let delimiters = vec!['.'];
        let escape_pairs = SqlUtil::get_escape_pairs(db_type);
        let filtered_tbs_file = format!("{}/filtered_tbs.txt", &self.base.test_dir);

        if BaseTestRunner::check_path_exists(&filtered_tbs_file) {
            let lines = BaseTestRunner::load_file(&filtered_tbs_file);
            for line in lines.iter() {
                let db_tb =
                    ConfigTokenParser::parse_config(line, db_type, &delimiters, None).unwrap();
                if db_tb.len() == 2 {
                    let db = SqlUtil::unescape(&db_tb[0], &escape_pairs[0]);
                    let tb = SqlUtil::unescape(&db_tb[1], &escape_pairs[0]);
                    filtered_db_tbs.insert((db, tb));
                }
            }
        }
        filtered_db_tbs
    }

    pub async fn get_tb_cols(
        &self,
        db_tb: &(String, String),
        from: &str,
    ) -> anyhow::Result<Vec<String>> {
        let (conn_pool_mysql, conn_pool_pg) = self.get_conn_pool(from);
        let cols = if let Some(conn_pool) = conn_pool_mysql {
            let tb_meta = RdbUtil::get_tb_meta_mysql(conn_pool, db_tb).await?;
            tb_meta.basic.cols.clone()
        } else if let Some(conn_pool) = conn_pool_pg {
            let tb_meta = RdbUtil::get_tb_meta_pg(conn_pool, db_tb).await?;
            tb_meta.basic.cols.clone()
        } else {
            vec![]
        };
        Ok(cols)
    }

    fn get_conn_pool(&self, from: &str) -> (&Option<Pool<MySql>>, &Option<Pool<Postgres>>) {
        if from == SRC {
            (&self.src_conn_pool_mysql, &self.src_conn_pool_pg)
        } else {
            (&self.dst_conn_pool_mysql, &self.dst_conn_pool_pg)
        }
    }

    pub fn get_db_type(&self, from: &str) -> DbType {
        let config = TaskConfig::new(&self.base.task_config_file).unwrap();
        if from == SRC {
            config.extractor_basic.db_type
        } else {
            config.sinker_basic.db_type
        }
    }

    async fn fetch_mysql_binlog_position(&self) -> anyhow::Result<(String, u32)> {
        if let Some(pool) = &self.src_conn_pool_mysql {
            let row = query("show master status").fetch_one(pool).await?;
            let binlog_file: String = row.try_get(0)?;
            let binlog_position = row.try_get(1)?;
            Ok((binlog_file, binlog_position))
        } else {
            Ok((String::new(), 0))
        }
    }

    pub fn get_where_sql(&self, schema: &str, tb: &str, condition: &str) -> String {
        let mut res: String = String::new();
        if let Some(where_condition) = self.filter.get_where_condition(schema, tb) {
            res = format!("WHERE {}", where_condition);
        }

        if condition.is_empty() {
            return res;
        }

        if res.is_empty() {
            format!("WHERE {}", condition)
        } else {
            format!("{} AND {}", res, condition)
        }
    }
}
