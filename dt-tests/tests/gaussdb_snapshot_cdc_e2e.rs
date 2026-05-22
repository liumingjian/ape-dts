#[cfg(test)]
mod test {
    use std::{
        env, fs,
        path::{Path, PathBuf},
        time::Duration,
    };

    use anyhow::{bail, Context};
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use chrono::{Duration as ChronoDuration, SecondsFormat, Utc};
    use dt_common::config::connection_auth_config::ConnectionAuthConfig;
    use dt_connector::oracle::OracleSqlPlusClient;
    use dt_task::task_util::TaskUtil;
    use openssl::ssl::{SslConnector, SslMethod, SslVerifyMode};
    use postgres_openssl::MakeTlsConnector;
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};
    use sqlx::{MySql, Pool, Postgres, Row};
    use tokio::time::{sleep, timeout};
    use tokio_postgres::{Client, NoTls, SimpleQueryMessage};
    use url::Url;

    use crate::test_config_util::TestConfigUtil;

    const DEFAULT_WEB_URL: &str = "http://127.0.0.1:5174";
    const RAW_DIR: &str = ".codex-tasks/20260516-gaussdb-bidirectional-snapshot-cdc/raw";
    const RUNS_DIR: &str = ".codex-tasks/20260516-gaussdb-bidirectional-snapshot-cdc/runs";
    const LICENSE_SECRET: &str = "ape-dts-console-license-secret-2025";
    const GAUSSDB_RW_PROBE_ATTEMPTS: usize = 2;
    const GAUSSDB_RW_PROBE_DELAY: Duration = Duration::from_secs(1);
    const GAUSSDB_CONNECT_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(6);
    const GAUSSDB_WRITE_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
    const GAUSSDB_SQL_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(20);

    #[tokio::test]
    async fn gaussdb_bidirectional_snapshot_cdc_via_console_and_playwright() {
        let result = run_matrix(MatrixScope::Full).await;
        if let Err(e) = result {
            panic!("{:#}", e);
        }
    }

    #[tokio::test]
    async fn gaussdb_pg_mysql_snapshot_cdc_via_console_and_playwright() {
        let result = run_matrix(MatrixScope::GaussDbPgMysql).await;
        if let Err(e) = result {
            panic!("{:#}", e);
        }
    }

    #[tokio::test]
    async fn gaussdb_oracle_snapshot_cdc_via_console_and_playwright() {
        let result = run_matrix(MatrixScope::GaussDbOracle).await;
        if let Err(e) = result {
            panic!("{:#}", e);
        }
    }

    #[tokio::test]
    async fn gaussdb_oracle_self_check_prepare_normal() {
        let result = self_check_prepare_normal().await;
        if let Err(e) = result {
            panic!("{:#}", e);
        }
    }

    #[tokio::test]
    async fn gaussdb_oracle_self_check_mutate_normal() {
        let result = self_check_mutate_normal().await;
        if let Err(e) = result {
            panic!("{:#}", e);
        }
    }

    #[tokio::test]
    async fn gaussdb_oracle_self_check_verify_normal() {
        let result = self_check_verify_normal().await;
        if let Err(e) = result {
            panic!("{:#}", e);
        }
    }

    async fn run_matrix(scope: MatrixScope) -> anyhow::Result<()> {
        load_env();
        fs::create_dir_all(raw_dir())?;

        let auth = BrowserAuth::login().await?;
        auth.activate_license().await?;
        let pg_src = PgDb::new(
            "pg_src",
            env_url("pg_extractor_without_auth_url")?,
            auth_cfg("pg_extractor")?,
        )
        .await?;
        let pg_dst = PgDb::new(
            "pg_dst",
            env_url("pg_sinker_without_auth_url")?,
            auth_cfg("pg_sinker")?,
        )
        .await?;
        let my_src = MyDb::new(
            "mysql_src",
            env_url("mysql_extractor_without_auth_url")?,
            auth_cfg("mysql_extractor")?,
        )
        .await?;
        let my_dst = MyDb::new(
            "mysql_dst",
            env_url("mysql_sinker_without_auth_url")?,
            auth_cfg("mysql_sinker")?,
        )
        .await?;
        let gd_pg = PgDb::new_gaussdb(
            "gaussdb_pg",
            env_url("gaussdb_pg_extractor_without_auth_url")?,
            auth_cfg("gaussdb_pg_extractor")?,
        )
        .await?;
        let gd_my = GaussMyDb::new(
            "gaussdb_mysql",
            env_url("gaussdb_mysql_sinker_without_auth_url")?,
            auth_cfg("gaussdb_mysql_sinker")?,
        )
        .await?;
        let gd_ora = PgDb::new_gaussdb(
            "gaussdb_oracle",
            env_url("gaussdb_oracle_sinker_without_auth_url")?,
            auth_cfg("gaussdb_oracle_sinker")?,
        )
        .await?;
        let oracle = OracleDb::new(
            "oracle",
            env_url("oracle_sinker_without_auth_url")?,
            auth_cfg("oracle_sinker")?,
        );

        let dbs = MatrixDbs {
            pg_src: &pg_src,
            pg_dst: &pg_dst,
            my_src: &my_src,
            my_dst: &my_dst,
            gd_pg: &gd_pg,
            gd_my: &gd_my,
            gd_ora: &gd_ora,
            oracle: &oracle,
        };
        let cases = match scope {
            MatrixScope::Full => full_matrix_cases(&dbs),
            MatrixScope::GaussDbPgMysql => gaussdb_pg_mysql_cases(&dbs),
            MatrixScope::GaussDbOracle => gaussdb_oracle_cases(&dbs),
        };

        let mut summaries = Vec::new();
        for case in cases {
            let case_name = case.name;
            let summary = match run_case(&auth, case).await {
                Ok(summary) => summary,
                Err(e) => json!({
                    "case": case_name,
                    "status": "failed",
                    "error": format!("{:#}", e),
                }),
            };
            summaries.push(summary);
        }

        fs::write(
            raw_dir().join("matrix-summary.json"),
            serde_json::to_string_pretty(&summaries)?,
        )?;
        let failures: Vec<String> = summaries
            .iter()
            .filter(|summary| summary["status"].as_str() == Some("failed"))
            .map(|summary| {
                format!(
                    "{}: {}",
                    summary["case"].as_str().unwrap_or("unknown"),
                    summary["error"].as_str().unwrap_or("unknown error")
                )
            })
            .collect();
        if !failures.is_empty() {
            bail!("matrix failures:\n{}", failures.join("\n"));
        }
        Ok(())
    }

    async fn self_check_prepare_normal() -> anyhow::Result<()> {
        let cases = gaussdb_oracle_self_check_cases().await?;
        for case in cases {
            case.source.reset(case.name).await?;
            case.target.reset(case.name).await?;
            case.source.seed(case.name).await?;
            print_rows(
                "prepared source",
                case.name,
                case.source.rows(case.name).await?,
            );
            print_rows(
                "prepared target",
                case.name,
                case.target.rows(case.name).await?,
            );
        }
        println!("PASS: normal scenario prepared. Create and start the Console tasks manually.");
        Ok(())
    }

    async fn self_check_mutate_normal() -> anyhow::Result<()> {
        let cases = gaussdb_oracle_self_check_cases().await?;
        for case in cases {
            case.source.apply_cdc(case.name).await?;
            print_rows(
                "mutated source",
                case.name,
                case.source.rows(case.name).await?,
            );
        }
        println!("PASS: CDC changes were applied to both source sides.");
        Ok(())
    }

    async fn self_check_verify_normal() -> anyhow::Result<()> {
        let cases = gaussdb_oracle_self_check_cases().await?;
        let mut failures = Vec::new();
        for case in cases {
            let source_rows = case.source.rows(case.name).await?;
            let target_rows = case.target.rows(case.name).await?;
            print_rows("source", case.name, source_rows.clone());
            print_rows("target", case.name, target_rows.clone());
            if source_rows == target_rows && target_rows == expected_rows() {
                println!("PASS: {} source rows match target rows", case.name);
            } else {
                failures.push(format!(
                    "{} mismatch\nsource={source_rows:?}\ntarget={target_rows:?}\nexpected={:?}",
                    case.name,
                    expected_rows()
                ));
            }
        }
        if !failures.is_empty() {
            bail!("self-check verification failed:\n{}", failures.join("\n"));
        }
        println!("PASS: GaussDBOracle <-> Oracle snapshot+cdc data is consistent.");
        Ok(())
    }

    async fn gaussdb_oracle_self_check_cases<'a>() -> anyhow::Result<Vec<Case<'a>>> {
        load_env();
        let gd_ora = Box::leak(Box::new(
            PgDb::new_gaussdb(
                "gaussdb_oracle",
                env_url("gaussdb_oracle_sinker_without_auth_url")?,
                auth_cfg("gaussdb_oracle_sinker")?,
            )
            .await?,
        ));
        let oracle = Box::leak(Box::new(OracleDb::new(
            "oracle",
            env_url("oracle_sinker_without_auth_url")?,
            auth_cfg("oracle_sinker")?,
        )));
        Ok(vec![
            Case::new(
                "gaussdb_oracle_to_oracle",
                DbKind::GaussOracle,
                DbKind::Oracle,
                gd_ora,
                oracle,
            ),
            Case::new(
                "oracle_to_gaussdb_oracle",
                DbKind::Oracle,
                DbKind::GaussOracle,
                oracle,
                gd_ora,
            ),
        ])
    }

    fn print_rows(label: &str, case: &str, rows: Vec<RowOut>) {
        println!("{label}: {case}");
        for row in rows {
            println!(
                "  id={} tracer={} payload={}",
                row.id, row.tracer, row.payload
            );
        }
    }

    #[derive(Clone, Copy)]
    enum MatrixScope {
        Full,
        GaussDbPgMysql,
        GaussDbOracle,
    }

    struct MatrixDbs<'a> {
        pg_src: &'a PgDb,
        pg_dst: &'a PgDb,
        my_src: &'a MyDb,
        my_dst: &'a MyDb,
        gd_pg: &'a PgDb,
        gd_my: &'a GaussMyDb,
        gd_ora: &'a PgDb,
        oracle: &'a OracleDb,
    }

    fn full_matrix_cases<'a>(dbs: &MatrixDbs<'a>) -> Vec<Case<'a>> {
        vec![
            Case::new(
                "gaussdb_pg_to_postgres",
                DbKind::GaussPg,
                DbKind::Pg,
                dbs.gd_pg,
                dbs.pg_dst,
            ),
            Case::new(
                "postgres_to_gaussdb_pg",
                DbKind::Pg,
                DbKind::GaussPg,
                dbs.pg_src,
                dbs.gd_pg,
            ),
            Case::new(
                "gaussdb_mysql_to_mysql",
                DbKind::GaussMy,
                DbKind::Mysql,
                dbs.gd_my,
                dbs.my_dst,
            ),
            Case::new(
                "mysql_to_gaussdb_mysql",
                DbKind::Mysql,
                DbKind::GaussMy,
                dbs.my_src,
                dbs.gd_my,
            ),
            Case::new(
                "gaussdb_oracle_to_oracle",
                DbKind::GaussOracle,
                DbKind::Oracle,
                dbs.gd_ora,
                dbs.oracle,
            ),
            Case::new(
                "oracle_to_gaussdb_oracle",
                DbKind::Oracle,
                DbKind::GaussOracle,
                dbs.oracle,
                dbs.gd_ora,
            ),
        ]
    }

    fn gaussdb_pg_mysql_cases<'a>(dbs: &MatrixDbs<'a>) -> Vec<Case<'a>> {
        vec![
            Case::new(
                "gaussdb_pg_to_mysql",
                DbKind::GaussPg,
                DbKind::Mysql,
                dbs.gd_pg,
                dbs.my_dst,
            ),
            Case::new(
                "mysql_to_gaussdb_pg",
                DbKind::Mysql,
                DbKind::GaussPg,
                dbs.my_src,
                dbs.gd_pg,
            ),
        ]
    }

    fn gaussdb_oracle_cases<'a>(dbs: &MatrixDbs<'a>) -> Vec<Case<'a>> {
        vec![
            Case::new(
                "gaussdb_oracle_to_oracle",
                DbKind::GaussOracle,
                DbKind::Oracle,
                dbs.gd_ora,
                dbs.oracle,
            ),
            Case::new(
                "oracle_to_gaussdb_oracle",
                DbKind::Oracle,
                DbKind::GaussOracle,
                dbs.oracle,
                dbs.gd_ora,
            ),
        ]
    }

    async fn run_case(auth: &BrowserAuth, case: Case<'_>) -> anyhow::Result<Value> {
        case.source.reset(case.name).await?;
        case.target.reset(case.name).await?;
        case.source.seed(case.name).await?;

        let task = auth.create_task(&case).await?;
        let task_id = task["id"].as_str().context("task id missing")?.to_string();
        auth.start_task(&task_id).await?;

        let run = auth.wait_run(&task_id).await?;
        let run_id = run["id"].as_str().context("run id missing")?.to_string();
        wait_phase2(&run_id, case.source_kind).await?;

        case.source.apply_cdc(case.name).await?;
        let rows = wait_target(&case).await?;
        auth.stop_task(&task_id).await?;

        let out = json!({
            "case": case.name,
            "status": "passed",
            "task_id": task_id,
            "run_id": run_id,
            "rows": rows,
            "phase2_ini": phase2_ini_path(&run_id).to_string_lossy(),
        });
        fs::write(
            raw_dir().join(format!("{}.json", case.name)),
            serde_json::to_string_pretty(&out)?,
        )?;
        Ok(out)
    }

    async fn wait_phase2(run_id: &str, source_kind: DbKind) -> anyhow::Result<()> {
        let state_path = run_dir(run_id).join("phase_state.json");
        for _ in 0..180 {
            if let Ok(raw) = fs::read_to_string(&state_path) {
                let v: Value = serde_json::from_str(&raw)?;
                if v["current_phase"].as_u64() == Some(2) {
                    let phase2 = fs::read_to_string(phase2_ini_path(run_id))?;
                    if phase2.contains("extract_type=cdc") {
                        return wait_cdc_ready(run_id, source_kind, &phase2).await;
                    }
                }
            }
            sleep(Duration::from_secs(1)).await;
        }
        bail!("phase2 did not start for run_id={run_id}");
    }

    async fn wait_cdc_ready(run_id: &str, source_kind: DbKind, phase2: &str) -> anyhow::Result<()> {
        let log_path = run_dir(run_id).join("logs/default.log");
        for _ in 0..180 {
            if let Ok(log) = fs::read_to_string(&log_path) {
                if cdc_ready(&log, source_kind, phase2) {
                    return Ok(());
                }
                fail_on_gaussdb_cdc_connect_exhausted(&log)?;
            }
            sleep(Duration::from_secs(1)).await;
        }
        bail!("GaussDB CDC did not become ready for run_id={run_id}");
    }

    fn cdc_ready(log: &str, source_kind: DbKind, phase2: &str) -> bool {
        match source_kind {
            DbKind::GaussPg | DbKind::GaussOracle => {
                log.contains("gaussdb cdc replication streaming started")
            }
            DbKind::Pg => log.contains("execute: START_REPLICATION SLOT"),
            DbKind::Mysql | DbKind::GaussMy => {
                log.contains("MysqlCdcExtractor starts")
                    && (phase2.contains("server_id=") || phase2.contains("gtid_enabled=true"))
            }
            DbKind::Oracle => log.contains("OracleLogMinerCdcExtractor starts"),
        }
    }

    fn fail_on_gaussdb_cdc_connect_exhausted(log: &str) -> anyhow::Result<()> {
        let blockers = [
            "GaussDB logical decoding requires wal_level=logical",
            "GaussDB is in recovery/standby mode",
        ];
        if let Some(line) = log
            .lines()
            .rev()
            .find(|line| line.contains("gaussdb cdc connect failed"))
        {
            if blockers.iter().any(|b| line.contains(b)) {
                bail!("GaussDB CDC connect failed: {line}");
            }
        }
        Ok(())
    }

    async fn wait_target(case: &Case<'_>) -> anyhow::Result<Vec<RowOut>> {
        for poll in 1..=120 {
            let rows = case
                .target
                .rows(case.name)
                .await
                .with_context(|| format!("{} poll {poll} target rows failed", case.name))?;
            if rows == expected_rows() {
                return Ok(rows);
            }
            fs::write(
                raw_dir().join(format!("{}-poll-{poll}.json", case.name)),
                serde_json::to_string_pretty(&rows)?,
            )?;
            sleep(Duration::from_secs(2)).await;
        }
        bail!("target rows did not converge for {}", case.name);
    }

    fn expected_rows() -> Vec<RowOut> {
        vec![
            RowOut::new(1, "cdc_update", "after_update"),
            RowOut::new(3, "cdc_insert", "after_insert"),
        ]
    }

    fn raw_dir() -> PathBuf {
        PathBuf::from(TestConfigUtil::get_project_root()).join(RAW_DIR)
    }

    fn run_dir(run_id: &str) -> PathBuf {
        runs_dir().join(run_id)
    }

    fn runs_dir() -> PathBuf {
        match env::var("APE_DTS_CONSOLE_E2E_RUNS_DIR") {
            Ok(path) => PathBuf::from(path),
            Err(_) => PathBuf::from(TestConfigUtil::get_project_root()).join(RUNS_DIR),
        }
    }

    fn phase2_ini_path(run_id: &str) -> PathBuf {
        run_dir(run_id).join("phase2.ini")
    }

    fn load_env() {
        let env_local = TestConfigUtil::get_absolute_path(".env.local");
        let env_default = TestConfigUtil::get_absolute_path(".env");
        let _ = load_env_override(&env_default);
        let _ = load_env_override(&env_local);
        env::set_var("ORACLE_SQLPLUS_DOCKER_CONTAINER", "oracle-xe-local");
    }

    fn load_env_override(path: &str) -> anyhow::Result<()> {
        let path = Path::new(path);
        if !path.exists() {
            return Ok(());
        }
        for line in fs::read_to_string(path)?.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            env::set_var(key.trim(), value.trim());
        }
        Ok(())
    }

    fn env_url(key: &str) -> anyhow::Result<String> {
        env::var(key).with_context(|| format!("missing env {key}"))
    }

    fn auth_cfg(prefix: &str) -> anyhow::Result<ConnectionAuthConfig> {
        Ok(ConnectionAuthConfig::Basic {
            username: env::var(format!("{prefix}_username"))?,
            password: Some(env::var(format!("{prefix}_password"))?),
        })
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum DbKind {
        Pg,
        Mysql,
        Oracle,
        GaussPg,
        GaussMy,
        GaussOracle,
    }

    impl DbKind {
        fn engine(self) -> &'static str {
            match self {
                DbKind::Pg => "postgres",
                DbKind::Mysql => "mysql",
                DbKind::Oracle => "oracle",
                DbKind::GaussPg | DbKind::GaussMy | DbKind::GaussOracle => "gaussdb",
            }
        }

        fn db_type(self) -> &'static str {
            match self {
                DbKind::Pg => "pg",
                DbKind::Mysql => "mysql",
                DbKind::Oracle => "oracle",
                DbKind::GaussPg => "gaussdb_pg",
                DbKind::GaussMy => "gaussdb_mysql",
                DbKind::GaussOracle => "gaussdb_oracle",
            }
        }

        fn sub_mode(self) -> Option<&'static str> {
            match self {
                DbKind::GaussPg => Some("pg-mode"),
                DbKind::GaussMy => Some("mysql-mode"),
                DbKind::GaussOracle => Some("oracle-mode"),
                _ => None,
            }
        }
    }

    trait DbOps: Send + Sync {
        fn endpoint(&self) -> Value;
        fn reset<'a>(&'a self, case: &'a str) -> PinBox<'a, ()>;
        fn seed<'a>(&'a self, case: &'a str) -> PinBox<'a, ()>;
        fn apply_cdc<'a>(&'a self, case: &'a str) -> PinBox<'a, ()>;
        fn rows<'a>(&'a self, case: &'a str) -> PinBox<'a, Vec<RowOut>>;
    }

    type PinBox<'a, T> =
        std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<T>> + Send + 'a>>;

    struct Case<'a> {
        name: &'static str,
        source_kind: DbKind,
        target_kind: DbKind,
        source: &'a dyn DbOps,
        target: &'a dyn DbOps,
    }

    impl<'a> Case<'a> {
        fn new(
            name: &'static str,
            source_kind: DbKind,
            target_kind: DbKind,
            source: &'a dyn DbOps,
            target: &'a dyn DbOps,
        ) -> Self {
            Self {
                name,
                source_kind,
                target_kind,
                source,
                target,
            }
        }
    }

    #[derive(Clone, Debug)]
    struct PgDb {
        label: &'static str,
        url: String,
        auth: ConnectionAuthConfig,
        pool: Pool<Postgres>,
        gaussdb: bool,
    }

    impl PgDb {
        async fn new(
            label: &'static str,
            url: String,
            auth: ConnectionAuthConfig,
        ) -> anyhow::Result<Self> {
            let pool = TaskUtil::create_pg_conn_pool(&url, &auth, 1, false, false).await?;
            Ok(Self {
                label,
                url,
                auth,
                pool,
                gaussdb: false,
            })
        }

        async fn new_gaussdb(
            label: &'static str,
            url: String,
            auth: ConnectionAuthConfig,
        ) -> anyhow::Result<Self> {
            let (url, pool) = resolve_gaussdb_rw_pool(&url, &auth).await?;
            Ok(Self {
                label,
                url,
                auth,
                pool,
                gaussdb: true,
            })
        }

        async fn exec(&self, sqls: &[String]) -> anyhow::Result<()> {
            if self.gaussdb {
                return self.exec_gaussdb(sqls).await;
            }
            execute_pg_sqls(self.label, &self.pool, sqls).await
        }

        async fn rows(&self, case: &str) -> anyhow::Result<Vec<RowOut>> {
            let sql = format!(
                "SELECT id, tracer, payload FROM public.{} ORDER BY id",
                table(case)
            );
            if self.gaussdb {
                return self.rows_gaussdb(&sql).await;
            }
            query_pg_rows(self.label, &self.pool, &sql).await
        }

        async fn exec_gaussdb(&self, sqls: &[String]) -> anyhow::Result<()> {
            let mut failures = Vec::new();
            match timeout(
                GAUSSDB_SQL_ATTEMPT_TIMEOUT,
                execute_pg_sqls(self.label, &self.pool, sqls),
            )
            .await
            {
                Ok(Ok(())) => {
                    println!(
                        "{} executed on existing GaussDB RW pool: {}",
                        self.label,
                        redact_url(&self.url)
                    );
                    return Ok(());
                }
                Ok(Err(e)) => failures.push(format!(
                    "{} => existing pool execute failed: {e:#}",
                    redact_url(&self.url)
                )),
                Err(_) => failures.push(format!(
                    "{} => existing pool execute timed out after {:?}",
                    redact_url(&self.url),
                    GAUSSDB_SQL_ATTEMPT_TIMEOUT
                )),
            }
            for url in gaussdb_candidate_urls(&self.url)? {
                let Ok(pool) = self.gaussdb_writable_pool(&url, &mut failures).await else {
                    continue;
                };
                let result = timeout(
                    GAUSSDB_SQL_ATTEMPT_TIMEOUT,
                    execute_pg_sqls(self.label, &pool, sqls),
                )
                .await;
                close_pool(pool).await;
                match result {
                    Ok(Ok(())) => {
                        println!(
                            "{} executed on GaussDB RW URL: {}",
                            self.label,
                            redact_url(&url)
                        );
                        return Ok(());
                    }
                    Ok(Err(e)) => failures.push(format!("{} => {:#}", redact_url(&url), e)),
                    Err(_) => failures.push(format!(
                        "{} => execute timed out after {:?}",
                        redact_url(&url),
                        GAUSSDB_SQL_ATTEMPT_TIMEOUT
                    )),
                }
            }
            bail!(
                "{} execute failed on all GaussDB RW candidates:\n{}",
                self.label,
                failures.join("\n")
            );
        }

        async fn rows_gaussdb(&self, sql: &str) -> anyhow::Result<Vec<RowOut>> {
            let mut failures = Vec::new();
            match timeout(
                GAUSSDB_SQL_ATTEMPT_TIMEOUT,
                query_pg_rows(self.label, &self.pool, sql),
            )
            .await
            {
                Ok(Ok(rows)) => {
                    println!(
                        "{} queried on existing GaussDB RW pool: {}",
                        self.label,
                        redact_url(&self.url)
                    );
                    return Ok(rows);
                }
                Ok(Err(e)) => failures.push(format!(
                    "{} => existing pool query failed: {e:#}",
                    redact_url(&self.url)
                )),
                Err(_) => failures.push(format!(
                    "{} => existing pool query timed out after {:?}",
                    redact_url(&self.url),
                    GAUSSDB_SQL_ATTEMPT_TIMEOUT
                )),
            }
            for url in gaussdb_candidate_urls(&self.url)? {
                let Ok(pool) = self.gaussdb_writable_pool(&url, &mut failures).await else {
                    continue;
                };
                let result = timeout(
                    GAUSSDB_SQL_ATTEMPT_TIMEOUT,
                    query_pg_rows(self.label, &pool, sql),
                )
                .await;
                close_pool(pool).await;
                match result {
                    Ok(Ok(rows)) => {
                        println!(
                            "{} queried on GaussDB RW URL: {}",
                            self.label,
                            redact_url(&url)
                        );
                        return Ok(rows);
                    }
                    Ok(Err(e)) => failures.push(format!("{} => {:#}", redact_url(&url), e)),
                    Err(_) => failures.push(format!(
                        "{} => query timed out after {:?}",
                        redact_url(&url),
                        GAUSSDB_SQL_ATTEMPT_TIMEOUT
                    )),
                }
            }
            bail!(
                "{} query failed on all GaussDB RW candidates:\n{}",
                self.label,
                failures.join("\n")
            );
        }

        async fn gaussdb_writable_pool(
            &self,
            url: &str,
            failures: &mut Vec<String>,
        ) -> anyhow::Result<Pool<Postgres>> {
            let pool = match timeout(
                GAUSSDB_CONNECT_ATTEMPT_TIMEOUT,
                TaskUtil::create_pg_conn_pool(url, &self.auth, 1, false, false),
            )
            .await
            {
                Ok(Ok(pool)) => pool,
                Ok(Err(e)) => {
                    failures.push(format!("{} => connect failed: {e:#}", redact_url(url)));
                    bail!("connect failed");
                }
                Err(_) => {
                    failures.push(format!(
                        "{} => connect timed out after {:?}",
                        redact_url(url),
                        GAUSSDB_CONNECT_ATTEMPT_TIMEOUT
                    ));
                    bail!("connect timed out");
                }
            };
            let probe_result = timeout(GAUSSDB_WRITE_PROBE_TIMEOUT, probe_write(&pool)).await;
            match probe_result {
                Ok(Ok(())) => Ok(pool),
                Ok(Err(e)) => {
                    close_pool(pool).await;
                    failures.push(format!("{} => write probe failed: {e:#}", redact_url(url)));
                    bail!("write probe failed");
                }
                Err(_) => {
                    close_pool(pool).await;
                    failures.push(format!(
                        "{} => write probe timed out after {:?}",
                        redact_url(url),
                        GAUSSDB_WRITE_PROBE_TIMEOUT
                    ));
                    bail!("write probe timed out");
                }
            }
        }
    }

    async fn close_pool(pool: Pool<Postgres>) {
        let _ = timeout(Duration::from_secs(1), pool.close()).await;
    }

    async fn resolve_gaussdb_rw_pool(
        base_url: &str,
        auth: &ConnectionAuthConfig,
    ) -> anyhow::Result<(String, Pool<Postgres>)> {
        let candidates = env::var("gaussdb_pg_candidate_hosts")
            .context("gaussdb_pg_candidate_hosts is required for GaussDB E2E")?;
        let urls = gaussdb_candidate_urls_from(base_url, &candidates)?;
        let mut last_failures = Vec::new();
        for attempt in 1..=GAUSSDB_RW_PROBE_ATTEMPTS {
            let mut failures = Vec::new();
            for url in &urls {
                match create_writable_gaussdb_pool(url, auth).await {
                    Ok(pool) => {
                        println!("selected GaussDB RW URL for E2E: {}", redact_url(url));
                        return Ok((url.clone(), pool));
                    }
                    Err(e) => failures.push(format!("{} => {:#}", redact_url(url), e)),
                }
            }
            last_failures = failures;
            if attempt < GAUSSDB_RW_PROBE_ATTEMPTS {
                sleep(GAUSSDB_RW_PROBE_DELAY).await;
            }
        }
        bail!(
            "cannot resolve writable GaussDB URL from gaussdb_pg_candidate_hosts={candidates}; failures:\n{}",
            last_failures.join("\n")
        );
    }

    async fn create_writable_gaussdb_pool(
        url: &str,
        auth: &ConnectionAuthConfig,
    ) -> anyhow::Result<Pool<Postgres>> {
        let pool = create_gaussdb_pool(url, auth).await?;
        let result = timeout(GAUSSDB_WRITE_PROBE_TIMEOUT, probe_write(&pool)).await;
        match result {
            Ok(Ok(())) => Ok(pool),
            Ok(Err(e)) => {
                close_pool(pool).await;
                Err(e).context("write probe failed")
            }
            Err(_) => {
                close_pool(pool).await;
                bail!(
                    "write probe timed out after {:?}",
                    GAUSSDB_WRITE_PROBE_TIMEOUT
                )
            }
        }
    }

    async fn create_gaussdb_pool(
        url: &str,
        auth: &ConnectionAuthConfig,
    ) -> anyhow::Result<Pool<Postgres>> {
        match timeout(
            GAUSSDB_CONNECT_ATTEMPT_TIMEOUT,
            TaskUtil::create_pg_conn_pool(url, auth, 1, false, false),
        )
        .await
        {
            Ok(Ok(pool)) => Ok(pool),
            Ok(Err(e)) => Err(e).with_context(|| format!("connect failed: {}", redact_url(url))),
            Err(_) => bail!(
                "connect timed out after {:?}: {}",
                GAUSSDB_CONNECT_ATTEMPT_TIMEOUT,
                redact_url(url)
            ),
        }
    }

    async fn execute_pg_sqls(
        label: &str,
        pool: &Pool<Postgres>,
        sqls: &[String],
    ) -> anyhow::Result<()> {
        for sql in sqls {
            sqlx::query(sql)
                .execute(pool)
                .await
                .with_context(|| format!("{label} execute failed: {sql}"))?;
        }
        Ok(())
    }

    async fn query_pg_rows(
        label: &str,
        pool: &Pool<Postgres>,
        sql: &str,
    ) -> anyhow::Result<Vec<RowOut>> {
        let rows = sqlx::query(sql)
            .fetch_all(pool)
            .await
            .with_context(|| format!("{label} query failed: {sql}"))?;
        Ok(rows
            .into_iter()
            .map(|r| {
                RowOut::new(
                    r.get::<i32, _>("id"),
                    r.get::<String, _>("tracer"),
                    r.get::<String, _>("payload"),
                )
            })
            .collect())
    }

    impl DbOps for PgDb {
        fn endpoint(&self) -> Value {
            endpoint_json(&self.url, &self.auth)
        }

        fn reset<'a>(&'a self, case: &'a str) -> PinBox<'a, ()> {
            Box::pin(async move { self.exec(&pg_reset_sql(case)).await })
        }

        fn seed<'a>(&'a self, case: &'a str) -> PinBox<'a, ()> {
            Box::pin(async move { self.exec(&pg_seed_sql(case)).await })
        }

        fn apply_cdc<'a>(&'a self, case: &'a str) -> PinBox<'a, ()> {
            Box::pin(async move { self.exec(&pg_cdc_sql(case)).await })
        }

        fn rows<'a>(&'a self, case: &'a str) -> PinBox<'a, Vec<RowOut>> {
            Box::pin(async move { self.rows(case).await })
        }
    }

    #[derive(Clone, Debug)]
    struct GaussMyDb {
        label: &'static str,
        url: String,
        auth: ConnectionAuthConfig,
    }

    impl GaussMyDb {
        async fn new(
            label: &'static str,
            url: String,
            auth: ConnectionAuthConfig,
        ) -> anyhow::Result<Self> {
            let url = resolve_gaussdb_rw_url(&url, &auth).await?;
            Ok(Self { label, url, auth })
        }

        async fn exec(&self, sqls: &[String]) -> anyhow::Result<()> {
            let client = self.gaussdb_client().await?;
            for sql in sqls {
                client
                    .simple_query(sql)
                    .await
                    .with_context(|| format!("{} execute failed: {sql}", self.label))?;
            }
            Ok(())
        }

        async fn rows(&self, case: &str) -> anyhow::Result<Vec<RowOut>> {
            let sql = format!(
                "SELECT id, tracer, payload FROM `{}`.`{}` ORDER BY id",
                schema(case),
                table(case)
            );
            let messages = self
                .gaussdb_client()
                .await?
                .simple_query(&sql)
                .await
                .with_context(|| format!("{} query failed: {sql}", self.label))?;
            Ok(simple_query_rows(messages))
        }

        async fn gaussdb_client(&self) -> anyhow::Result<Client> {
            let url = resolve_gaussdb_rw_url(&self.url, &self.auth).await?;
            connect_pg_wire_client(&url, &self.auth).await
        }
    }

    impl DbOps for GaussMyDb {
        fn endpoint(&self) -> Value {
            endpoint_json(&self.url, &self.auth)
        }

        fn reset<'a>(&'a self, case: &'a str) -> PinBox<'a, ()> {
            Box::pin(async move { self.exec(&mysql_reset_sql(case)).await })
        }

        fn seed<'a>(&'a self, case: &'a str) -> PinBox<'a, ()> {
            Box::pin(async move { self.exec(&mysql_seed_sql(case)).await })
        }

        fn apply_cdc<'a>(&'a self, case: &'a str) -> PinBox<'a, ()> {
            Box::pin(async move { self.exec(&mysql_cdc_sql(case)).await })
        }

        fn rows<'a>(&'a self, case: &'a str) -> PinBox<'a, Vec<RowOut>> {
            Box::pin(async move { self.rows(case).await })
        }
    }

    fn simple_query_rows(messages: Vec<SimpleQueryMessage>) -> Vec<RowOut> {
        messages
            .into_iter()
            .filter_map(|message| {
                let SimpleQueryMessage::Row(row) = message else {
                    return None;
                };
                let id = row.get("id")?.parse().ok()?;
                Some(RowOut::new(
                    id,
                    row.get("tracer")?.to_string(),
                    row.get("payload")?.to_string(),
                ))
            })
            .collect()
    }

    #[derive(Clone, Debug)]
    struct MyDb {
        url: String,
        auth: ConnectionAuthConfig,
        pool: Pool<MySql>,
    }

    impl MyDb {
        async fn new(
            _label: &'static str,
            url: String,
            auth: ConnectionAuthConfig,
        ) -> anyhow::Result<Self> {
            let pool = TaskUtil::create_mysql_conn_pool(&url, &auth, 1, false, None).await?;
            Ok(Self { url, auth, pool })
        }

        async fn exec(&self, sqls: &[String]) -> anyhow::Result<()> {
            for sql in sqls {
                sqlx::query(sql).execute(&self.pool).await?;
            }
            Ok(())
        }
    }

    impl DbOps for MyDb {
        fn endpoint(&self) -> Value {
            endpoint_json(&self.url, &self.auth)
        }

        fn reset<'a>(&'a self, case: &'a str) -> PinBox<'a, ()> {
            Box::pin(async move { self.exec(&mysql_reset_sql(case)).await })
        }

        fn seed<'a>(&'a self, case: &'a str) -> PinBox<'a, ()> {
            Box::pin(async move { self.exec(&mysql_seed_sql(case)).await })
        }

        fn apply_cdc<'a>(&'a self, case: &'a str) -> PinBox<'a, ()> {
            Box::pin(async move { self.exec(&mysql_cdc_sql(case)).await })
        }

        fn rows<'a>(&'a self, case: &'a str) -> PinBox<'a, Vec<RowOut>> {
            Box::pin(async move {
                let sql = format!(
                    "SELECT id, tracer, payload FROM `{}`.`{}` ORDER BY id",
                    schema(case),
                    table(case)
                );
                let rows = sqlx::query(&sql).fetch_all(&self.pool).await?;
                Ok(rows
                    .into_iter()
                    .map(|r| {
                        RowOut::new(
                            r.get::<i32, _>("id"),
                            r.get::<String, _>("tracer"),
                            r.get::<String, _>("payload"),
                        )
                    })
                    .collect())
            })
        }
    }

    #[derive(Clone, Debug)]
    struct OracleDb {
        url: String,
        auth: ConnectionAuthConfig,
        client: OracleSqlPlusClient,
    }

    impl OracleDb {
        fn new(_label: &'static str, url: String, auth: ConnectionAuthConfig) -> Self {
            let client = OracleSqlPlusClient::new(url.clone(), auth.clone());
            Self { url, auth, client }
        }

        async fn exec(&self, sqls: &[String]) -> anyhow::Result<()> {
            for sql in sqls {
                self.client.exec(sql).await?;
            }
            Ok(())
        }
    }

    impl DbOps for OracleDb {
        fn endpoint(&self) -> Value {
            endpoint_json(&self.url, &self.auth)
        }

        fn reset<'a>(&'a self, case: &'a str) -> PinBox<'a, ()> {
            Box::pin(async move { self.exec(&oracle_reset_sql(case)).await })
        }

        fn seed<'a>(&'a self, case: &'a str) -> PinBox<'a, ()> {
            Box::pin(async move { self.exec(&oracle_seed_sql(case)).await })
        }

        fn apply_cdc<'a>(&'a self, case: &'a str) -> PinBox<'a, ()> {
            Box::pin(async move { self.exec(&oracle_cdc_sql(case)).await })
        }

        fn rows<'a>(&'a self, case: &'a str) -> PinBox<'a, Vec<RowOut>> {
            Box::pin(async move {
                let lines = self
                    .client
                    .query_lines(&format!(
                        "SELECT ID, TRACER, PAYLOAD FROM APE_DTS.{} ORDER BY ID",
                        table(case).to_uppercase()
                    ))
                    .await?;
                Ok(lines
                    .into_iter()
                    .filter_map(|line| {
                        let parts: Vec<_> = line.split('|').map(|s| s.trim().to_string()).collect();
                        if parts.len() == 3 {
                            let id: i32 = parts[0].parse().ok()?;
                            Some(RowOut::new(id, parts[1].clone(), parts[2].clone()))
                        } else {
                            None
                        }
                    })
                    .collect())
            })
        }
    }

    fn endpoint_json(url: &str, auth: &ConnectionAuthConfig) -> Value {
        match auth {
            ConnectionAuthConfig::Basic { username, password } => {
                json!({"url": url, "username": username, "password": password.clone().unwrap_or_default()})
            }
            ConnectionAuthConfig::NoAuth => json!({"url": url}),
        }
    }

    fn schema(case: &str) -> String {
        format!("e2e_{}", case.replace('-', "_"))
    }

    fn table(case: &str) -> String {
        format!("t_{}", case.replace('-', "_"))
    }

    fn pg_reset_sql(case: &str) -> Vec<String> {
        vec![
            format!("DROP TABLE IF EXISTS public.{}", table(case)),
            format!(
                "CREATE TABLE public.{} (id INTEGER PRIMARY KEY, tracer TEXT, payload TEXT)",
                table(case)
            ),
        ]
    }

    fn pg_seed_sql(case: &str) -> Vec<String> {
        vec![format!(
            "INSERT INTO public.{} (id, tracer, payload) VALUES (1, 'snapshot', 'before')",
            table(case)
        )]
    }

    fn pg_cdc_sql(case: &str) -> Vec<String> {
        vec![
            format!("DELETE FROM public.{} WHERE id >= 2", table(case)),
            format!("INSERT INTO public.{} (id, tracer, payload) VALUES (2, 'cdc_insert_delete', 'to_delete')", table(case)),
            format!("UPDATE public.{} SET tracer='cdc_update', payload='after_update' WHERE id=1", table(case)),
            format!("DELETE FROM public.{} WHERE id=2", table(case)),
            format!("INSERT INTO public.{} (id, tracer, payload) VALUES (3, 'cdc_insert', 'after_insert')", table(case)),
        ]
    }

    fn mysql_reset_sql(case: &str) -> Vec<String> {
        vec![
            format!("DROP DATABASE IF EXISTS `{}`", schema(case)),
            format!("CREATE DATABASE `{}`", schema(case)),
            format!("CREATE TABLE `{}`.`{}` (id INT PRIMARY KEY, tracer VARCHAR(64), payload VARCHAR(128))", schema(case), table(case)),
        ]
    }

    fn mysql_seed_sql(case: &str) -> Vec<String> {
        vec![format!(
            "INSERT INTO `{}`.`{}` (id, tracer, payload) VALUES (1, 'snapshot', 'before')",
            schema(case),
            table(case)
        )]
    }

    fn mysql_cdc_sql(case: &str) -> Vec<String> {
        vec![
            format!("DELETE FROM `{}`.`{}` WHERE id >= 2", schema(case), table(case)),
            format!("INSERT INTO `{}`.`{}` (id, tracer, payload) VALUES (2, 'cdc_insert_delete', 'to_delete')", schema(case), table(case)),
            format!("UPDATE `{}`.`{}` SET tracer='cdc_update', payload='after_update' WHERE id=1", schema(case), table(case)),
            format!("DELETE FROM `{}`.`{}` WHERE id=2", schema(case), table(case)),
            format!("INSERT INTO `{}`.`{}` (id, tracer, payload) VALUES (3, 'cdc_insert', 'after_insert')", schema(case), table(case)),
        ]
    }

    fn oracle_reset_sql(case: &str) -> Vec<String> {
        let tb = table(case).to_uppercase();
        vec![
            format!("BEGIN EXECUTE IMMEDIATE 'DROP TABLE APE_DTS.{tb} PURGE'; EXCEPTION WHEN OTHERS THEN IF SQLCODE != -942 THEN RAISE; END IF; END;\n/"),
            format!("CREATE TABLE APE_DTS.{tb} (ID NUMBER(10) PRIMARY KEY, TRACER VARCHAR2(64), PAYLOAD VARCHAR2(128))"),
        ]
    }

    fn oracle_seed_sql(case: &str) -> Vec<String> {
        vec![format!(
            "INSERT INTO APE_DTS.{} (ID, TRACER, PAYLOAD) VALUES (1, 'snapshot', 'before')",
            table(case).to_uppercase()
        )]
    }

    fn oracle_cdc_sql(case: &str) -> Vec<String> {
        let tb = table(case).to_uppercase();
        vec![
            format!("DELETE FROM APE_DTS.{tb} WHERE ID >= 2"),
            format!("INSERT INTO APE_DTS.{tb} (ID, TRACER, PAYLOAD) VALUES (2, 'cdc_insert_delete', 'to_delete')"),
            format!("UPDATE APE_DTS.{tb} SET TRACER='cdc_update', PAYLOAD='after_update' WHERE ID=1"),
            format!("DELETE FROM APE_DTS.{tb} WHERE ID=2"),
            format!("INSERT INTO APE_DTS.{tb} (ID, TRACER, PAYLOAD) VALUES (3, 'cdc_insert', 'after_insert')"),
        ]
    }

    #[derive(Debug, Clone)]
    struct BrowserAuth {
        cookie: String,
        xsrf: String,
    }

    impl BrowserAuth {
        async fn login() -> anyhow::Result<Self> {
            let web_url = web_url();
            let script = r#"
                async page => {
	                  await page.goto('__WEB_URL__/login');
	                  await page.evaluate(async () => {
	                    await fetch('/api/healthz');
	                    const xsrf = document.cookie
	                      .split('; ')
	                      .find(v => v.startsWith('XSRF-TOKEN='))
	                      ?.split('=')[1] ?? '';
	                    const res = await fetch('/api/auth/login', {
	                      method: 'POST',
	                      headers: {
	                        'Content-Type': 'application/json',
	                        'X-XSRF-TOKEN': decodeURIComponent(xsrf)
	                      },
	                      body: JSON.stringify({username: 'admin', password: 'admin123'})
	                    });
	                    if (!res.ok) throw new Error(await res.text());
                  });
	                  const cookies = await page.context().cookies('__WEB_URL__');
	                  return {
	                    cookie: cookies.map(c => `${c.name}=${c.value}`).join('; '),
	                    xsrf: cookies.find(c => c.name === 'XSRF-TOKEN')?.value ?? ''
	                  };
                }
            "#
            .replace("__WEB_URL__", &web_url);
            let raw = playwright_json(&script).await?;
            let cookie = raw["cookie"].as_str().unwrap_or("").to_string();
            let xsrf = raw["xsrf"].as_str().unwrap_or("").to_string();
            if cookie.is_empty() || xsrf.is_empty() {
                bail!("playwright login did not return auth cookies: {raw}");
            }
            Ok(Self { cookie, xsrf })
        }

        async fn create_task(&self, case: &Case<'_>) -> anyhow::Result<Value> {
            let payload = task_payload(case);
            self.post("/tasks", payload).await
        }

        async fn start_task(&self, task_id: &str) -> anyhow::Result<Value> {
            self.post(&format!("/tasks/{task_id}/start"), json!({}))
                .await
        }

        async fn stop_task(&self, task_id: &str) -> anyhow::Result<Value> {
            self.post(&format!("/tasks/{task_id}/stop"), json!({}))
                .await
        }

        async fn activate_license(&self) -> anyhow::Result<Value> {
            self.post("/license/activate", json!({"code": activation_code()}))
                .await
        }

        async fn wait_run(&self, task_id: &str) -> anyhow::Result<Value> {
            for _ in 0..120 {
                let v = self.get(&format!("/tasks/{task_id}/runs")).await?;
                if let Some(run) = v["items"].as_array().and_then(|a| a.first()) {
                    return Ok(run.clone());
                }
                sleep(Duration::from_secs(1)).await;
            }
            bail!("run not created for task {task_id}");
        }

        async fn get(&self, path: &str) -> anyhow::Result<Value> {
            api_request("GET", path, None, self).await
        }

        async fn post(&self, path: &str, body: Value) -> anyhow::Result<Value> {
            api_request("POST", path, Some(body), self).await
        }
    }

    fn task_payload(case: &Case<'_>) -> Value {
        let source_type = case.source_kind.db_type();
        let mut extractor = json!({"extract_type": "snapshot_and_cdc"});
        if source_type == "mysql" || source_type == "gaussdb_mysql" {
            extractor["server_id"] = json!(server_id(case.name));
            extractor["heartbeat_interval_secs"] = json!(1);
        }
        if source_type == "pg" || source_type == "gaussdb_pg" || source_type == "gaussdb_oracle" {
            extractor["slot_name"] = json!(cdc_slot_name(source_type, case.name));
            extractor["recreate_slot_if_exists"] = json!(source_type == "pg");
            extractor["keepalive_interval_secs"] = json!(10);
            extractor["heartbeat_interval_secs"] = json!(0);
            extractor["heartbeat_tb"] = json!("heartbeat_db.ape_dts_heartbeat");
        }
        if source_type == "oracle" {
            extractor["cdc_mode"] = json!("logminer");
            extractor["poll_interval_millis"] = json!(200);
            extractor["poll_batch_size"] = json!(200);
        }

        let mut payload = json!({
            "name": format!("e2e-{}", case.name),
            "kind": "snapshot",
            "engineSource": case.source_kind.engine(),
            "engineTarget": case.target_kind.engine(),
            "sourceEndpoint": case.source.endpoint(),
            "targetEndpoint": case.target.endpoint(),
            "extractor": extractor,
            "sinker": {"sink_type": "write", "batch_size": 2},
            "filter": {
                "do_tbs": source_table_filter(case.source_kind, case.name),
                "do_events": "insert,update,delete"
            },
            "router": router_map(case),
            "parallelizer": {"parallel_type": "snapshot", "parallel_size": 1},
            "pipeline": {"buffer_size": 4, "checkpoint_interval_secs": 1, "max_rps": 0},
            "resumer": {"resume_type": "from_log"},
            "runtime": {"sync_schema": false, "sync_index": false}
        });
        if let Some(mode) = case
            .source_kind
            .sub_mode()
            .or_else(|| case.target_kind.sub_mode())
        {
            payload["subMode"] = json!(mode);
        }
        payload
    }

    fn source_table_filter(kind: DbKind, case: &str) -> String {
        match kind {
            DbKind::Mysql | DbKind::GaussMy => format!("{}.{}", schema(case), table(case)),
            DbKind::Oracle => format!("APE_DTS.{}", table(case).to_uppercase()),
            _ => format!("public.{}", table(case)),
        }
    }

    fn router_map(case: &Case<'_>) -> Value {
        let src = source_table_filter(case.source_kind, case.name);
        let dst = source_table_filter(case.target_kind, case.name);
        let mut router = json!({"tb_map": format!("{src}:{dst}")});
        if case.source_kind == DbKind::Oracle
            && matches!(
                case.target_kind,
                DbKind::Pg | DbKind::GaussPg | DbKind::GaussOracle
            )
        {
            router["col_map"] = json!(format!(
                r#"[{{"db":"APE_DTS","tb":"{}","col_map":{{"ID":"id","TRACER":"tracer","PAYLOAD":"payload"}}}}]"#,
                table(case.name).to_uppercase()
            ));
        }
        router
    }

    fn server_id(seed: &str) -> u64 {
        10_000 + seed.bytes().fold(0_u64, |acc, b| acc + b as u64)
    }

    fn cdc_slot_name(source_type: &str, case: &str) -> String {
        let base = format!("ape_e2e_{}", server_id(case));
        if !source_type.starts_with("gaussdb_") {
            return base;
        }
        format!("{}_{}", base, Utc::now().timestamp_millis())
    }

    fn activation_code() -> String {
        let expire_at =
            (Utc::now() + ChronoDuration::days(30)).to_rfc3339_opts(SecondsFormat::Secs, true);
        let granted_to = "gaussdb-e2e";
        let sku = "professional";
        let max_tasks = 100_i64;
        let sig = license_signature(sku, max_tasks, &expire_at, granted_to);
        let payload = json!({
            "sku": sku,
            "maxTasks": max_tasks,
            "expireAt": expire_at,
            "grantedTo": granted_to,
            "sig": sig
        });
        URL_SAFE_NO_PAD.encode(payload.to_string().as_bytes())
    }

    fn license_signature(sku: &str, max_tasks: i64, expire_at: &str, granted_to: &str) -> String {
        let message = format!("{sku}:{max_tasks}:{expire_at}:{granted_to}:{LICENSE_SECRET}");
        let hash = Sha256::digest(message.as_bytes());
        format!("{hash:x}")[..16].to_string()
    }

    async fn resolve_gaussdb_rw_url(
        base_url: &str,
        auth: &ConnectionAuthConfig,
    ) -> anyhow::Result<String> {
        let candidates = env::var("gaussdb_pg_candidate_hosts")
            .context("gaussdb_pg_candidate_hosts is required for GaussDB E2E")?;
        let urls = gaussdb_candidate_urls_from(base_url, &candidates)?;
        let mut last_failures = Vec::new();
        for attempt in 1..=GAUSSDB_RW_PROBE_ATTEMPTS {
            let mut failures = Vec::new();
            for url in &urls {
                match probe_gaussdb_rw_url(url, auth).await {
                    Ok(()) => {
                        println!("selected GaussDB RW URL for E2E: {}", redact_url(url));
                        return Ok(url.clone());
                    }
                    Err(e) => failures.push(format!("{} => {:#}", redact_url(url), e)),
                }
            }
            last_failures = failures;
            if attempt < GAUSSDB_RW_PROBE_ATTEMPTS {
                sleep(GAUSSDB_RW_PROBE_DELAY).await;
            }
        }
        bail!(
            "cannot resolve writable GaussDB URL from gaussdb_pg_candidate_hosts={candidates}; failures:\n{}",
            last_failures.join("\n")
        );
    }

    fn gaussdb_candidate_urls(base_url: &str) -> anyhow::Result<Vec<String>> {
        let candidates = env::var("gaussdb_pg_candidate_hosts")
            .context("gaussdb_pg_candidate_hosts is required for GaussDB E2E")?;
        gaussdb_candidate_urls_from(base_url, &candidates)
    }

    fn gaussdb_candidate_urls_from(
        base_url: &str,
        candidates: &str,
    ) -> anyhow::Result<Vec<String>> {
        let ordered = ordered_gaussdb_candidates(base_url, candidates)?;
        Ok(ordered.iter().flat_map(candidate_urls).collect())
    }

    fn redact_url(url: &str) -> String {
        match Url::parse(url) {
            Ok(mut parsed) => {
                let _ = parsed.set_username("***");
                let _ = parsed.set_password(Some("***"));
                parsed.to_string()
            }
            Err(_) => url.to_string(),
        }
    }

    fn ordered_gaussdb_candidates(base_url: &str, candidates: &str) -> anyhow::Result<Vec<Url>> {
        let base = Url::parse(base_url)?;
        let base_host = base.host_str().unwrap_or_default();
        let mut parsed = Vec::new();
        for prefer_base_host in [true, false] {
            for raw in candidates
                .split(',')
                .map(str::trim)
                .filter(|v| !v.is_empty())
            {
                let url = rewrite_host_port(&base, raw)?;
                if (url.host_str() == Some(base_host)) == prefer_base_host {
                    parsed.push(url);
                }
            }
        }
        Ok(parsed)
    }

    fn rewrite_host_port(base: &Url, raw: &str) -> anyhow::Result<Url> {
        let mut url = base.clone();
        let (host, port) = match raw.rsplit_once(':') {
            Some((host, port)) if !host.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => {
                (host, Some(port.parse::<u16>()?))
            }
            _ => (raw, base.port()),
        };
        url.set_host(Some(host))
            .map_err(|_| anyhow::anyhow!("invalid GaussDB host {host}"))?;
        if let Some(port) = port {
            url.set_port(Some(port))
                .map_err(|_| anyhow::anyhow!("invalid GaussDB port {port}"))?;
        }
        Ok(url)
    }

    fn candidate_urls(url: &Url) -> Vec<String> {
        let mut urls = vec![url.to_string()];
        if url
            .query_pairs()
            .any(|(k, v)| k == "sslmode" && v == "disable")
        {
            return urls;
        }
        let mut no_ssl = url.clone();
        let pairs: Vec<_> = no_ssl
            .query_pairs()
            .into_owned()
            .filter(|(k, _)| k != "sslmode")
            .collect();
        no_ssl.query_pairs_mut().clear();
        for (key, value) in pairs {
            no_ssl.query_pairs_mut().append_pair(&key, &value);
        }
        no_ssl.query_pairs_mut().append_pair("sslmode", "disable");
        urls.push(no_ssl.to_string());
        urls
    }

    async fn probe_gaussdb_rw_url(url: &str, auth: &ConnectionAuthConfig) -> anyhow::Result<()> {
        let pool = match timeout(
            GAUSSDB_CONNECT_ATTEMPT_TIMEOUT,
            TaskUtil::create_pg_conn_pool(url, auth, 1, false, false),
        )
        .await
        {
            Ok(Ok(pool)) => pool,
            Ok(Err(e)) => bail!("connect failed: {e:#}"),
            Err(_) => bail!(
                "connect timed out after {:?}",
                GAUSSDB_CONNECT_ATTEMPT_TIMEOUT
            ),
        };
        let result = timeout(GAUSSDB_WRITE_PROBE_TIMEOUT, probe_write(&pool)).await;
        pool.close().await;
        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e).context("write probe failed"),
            Err(_) => bail!(
                "write probe timed out after {:?}",
                GAUSSDB_WRITE_PROBE_TIMEOUT
            ),
        }
    }

    async fn probe_write(pool: &Pool<Postgres>) -> anyhow::Result<()> {
        let probe_tbl = format!("ape_dts_rw_probe_{}", server_id("gaussdb_e2e"));
        sqlx::query("BEGIN").execute(pool).await?;
        let create_sql = format!("CREATE TABLE public.{probe_tbl} (id int4)");
        let insert_sql = format!("INSERT INTO public.{probe_tbl} (id) VALUES (1)");
        let create_res = sqlx::query(&create_sql).execute(pool).await;
        let insert_res = if create_res.is_ok() {
            Some(sqlx::query(&insert_sql).execute(pool).await)
        } else {
            None
        };
        let rollback_res = sqlx::query("ROLLBACK").execute(pool).await;
        create_res?;
        insert_res.context("insert probe missing")??;
        rollback_res?;
        Ok(())
    }

    async fn connect_pg_wire_client(
        url: &str,
        auth: &ConnectionAuthConfig,
    ) -> anyhow::Result<Client> {
        let parsed = Url::parse(url)?;
        let conn_info = pg_wire_conn_info(&parsed, auth)?;
        let requires_tls = parsed
            .query_pairs()
            .any(|(key, value)| key == "sslmode" && value == "require");
        if requires_tls {
            let mut builder = SslConnector::builder(SslMethod::tls())?;
            builder.set_verify(SslVerifyMode::NONE);
            let (client, connection) = timeout(
                Duration::from_secs(20),
                tokio_postgres::connect(&conn_info, MakeTlsConnector::new(builder.build())),
            )
            .await
            .context("tokio-postgres TLS connect timed out")??;
            tokio::spawn(async move {
                let _ = connection.await;
            });
            return Ok(client);
        } else {
            let (client, connection) = timeout(
                Duration::from_secs(20),
                tokio_postgres::connect(&conn_info, NoTls),
            )
            .await
            .context("tokio-postgres connect timed out")??;
            tokio::spawn(async move {
                let _ = connection.await;
            });
            return Ok(client);
        }
    }

    fn pg_wire_conn_info(url: &Url, auth: &ConnectionAuthConfig) -> anyhow::Result<String> {
        let ConnectionAuthConfig::Basic { username, password } = auth else {
            bail!("GaussDB E2E requires basic auth");
        };
        let sslmode = url
            .query_pairs()
            .find(|(key, _)| key == "sslmode")
            .map(|(_, value)| value.to_string())
            .unwrap_or_else(|| "disable".to_string());
        Ok(format!(
            "host={} port={} dbname={} user={} password={} sslmode={} protocolVersion=351",
            url.host_str().context("missing host")?,
            url.port().unwrap_or(5432),
            url.path().trim_start_matches('/'),
            username,
            password.as_deref().unwrap_or_default(),
            sslmode
        ))
    }
    async fn api_request(
        method: &str,
        path: &str,
        body: Option<Value>,
        auth: &BrowserAuth,
    ) -> anyhow::Result<Value> {
        let client = reqwest::Client::new();
        let api = api_url();
        let mut req = match method {
            "GET" => client.get(format!("{api}{path}")),
            "POST" => client.post(format!("{api}{path}")),
            _ => bail!("unsupported method {method}"),
        }
        .header("Cookie", &auth.cookie)
        .header("X-XSRF-TOKEN", &auth.xsrf)
        .header("Content-Type", "application/json");
        if let Some(body) = body {
            req = req.json(&body);
        }
        let res = req.send().await?;
        let status = res.status();
        let text = res.text().await?;
        if !status.is_success() {
            bail!("{method} {path} failed: {status} {text}");
        }
        Ok(if text.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(&text)?
        })
    }

    fn web_url() -> String {
        env::var("APE_DTS_CONSOLE_E2E_WEB_URL").unwrap_or_else(|_| DEFAULT_WEB_URL.to_string())
    }

    fn api_url() -> String {
        format!("{}/api", web_url().trim_end_matches('/'))
    }

    async fn playwright_json(script: &str) -> anyhow::Result<Value> {
        ensure_playwright_browser().await?;
        let output = timeout(
            Duration::from_secs(30),
            tokio::process::Command::new("playwright-cli")
                .arg("--raw")
                .arg("run-code")
                .arg(script)
                .output(),
        )
        .await??;
        if !output.status.success() {
            bail!(
                "playwright-cli failed: stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let raw = String::from_utf8_lossy(&output.stdout);
        serde_json::from_str(raw.trim()).with_context(|| format!("invalid playwright JSON: {raw}"))
    }

    async fn ensure_playwright_browser() -> anyhow::Result<()> {
        let probe = timeout(
            Duration::from_secs(10),
            tokio::process::Command::new("playwright-cli")
                .arg("--raw")
                .arg("eval")
                .arg("location.href")
                .output(),
        )
        .await??;
        if probe.status.success() {
            return Ok(());
        }

        let open = timeout(
            Duration::from_secs(30),
            tokio::process::Command::new("playwright-cli")
                .arg("open")
                .arg("about:blank")
                .output(),
        )
        .await??;
        if !open.status.success() {
            bail!(
                "playwright-cli open failed: stdout={} stderr={}",
                String::from_utf8_lossy(&open.stdout),
                String::from_utf8_lossy(&open.stderr)
            );
        }
        Ok(())
    }

    #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
    struct RowOut {
        id: i32,
        tracer: String,
        payload: String,
    }

    impl RowOut {
        fn new(id: i32, tracer: impl Into<String>, payload: impl Into<String>) -> Self {
            Self {
                id,
                tracer: tracer.into(),
                payload: payload.into(),
            }
        }
    }
}
