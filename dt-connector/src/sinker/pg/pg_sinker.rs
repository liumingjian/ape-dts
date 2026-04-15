use std::{cmp, str::FromStr, sync::Arc, time::Duration};

use anyhow::Context;
use async_trait::async_trait;
use dt_common::{config::connection_auth_config::ConnectionAuthConfig, log_warn};
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    ConnectOptions, Executor, Pool, Postgres,
};
use tokio::{
    sync::{Mutex, RwLock},
    time::{timeout, Instant},
};
use url::Url;

use crate::{
    call_batch_fn, data_marker::DataMarker, rdb_query_builder::RdbQueryBuilder,
    rdb_router::RdbRouter, sinker::base_sinker::BaseSinker, Sinker,
};
use dt_common::{
    config::config_enums::DbType,
    log_error, log_info,
    meta::{
        ddl_meta::{ddl_data::DdlData, ddl_type::DdlType},
        pg::pg_meta_manager::PgMetaManager,
        row_data::RowData,
        row_type::RowType,
    },
    monitor::monitor::Monitor,
    utils::limit_queue::LimitedQueue,
};

#[derive(Clone)]
pub struct PgSinker {
    pub url: String,
    pub connection_auth: ConnectionAuthConfig,
    pub db_type: DbType,
    pub conn_pool: Arc<RwLock<Pool<Postgres>>>,
    pub meta_manager: PgMetaManager,
    pub router: RdbRouter,
    pub batch_size: usize,
    pub max_connections: u32,
    pub enable_sqlx_log: bool,
    pub disable_foreign_key_checks: bool,
    pub reconnect_lock: Arc<Mutex<()>>,
    pub last_success_endpoint: Arc<RwLock<Option<(String, u16)>>>,
    pub monitor: Arc<Monitor>,
    pub data_marker: Option<Arc<RwLock<DataMarker>>>,
    pub replace: bool,
    pub monitor_interval: u64,
}

#[async_trait]
impl Sinker for PgSinker {
    async fn sink_dml(&mut self, mut data: Vec<RowData>, batch: bool) -> anyhow::Result<()> {
        if data.is_empty() {
            return Ok(());
        }

        // For GaussDB targets without VIP/LB, failover can transiently turn the current endpoint
        // read-only or drop connections. Keep the CDC task alive by reconnecting and retrying.
        let mut last_err: Option<anyhow::Error> = None;
        let max_attempts: u32 = if self.is_gaussdb_target() { 6 } else { 1 };
        for attempt in 1..=max_attempts {
            let exec_res = if !batch {
                self.serial_sink(&data).await
            } else {
                match data[0].row_type {
                    RowType::Insert => {
                        // `call_batch_fn!` uses `?` internally; wrap it so errors become a value
                        // we can handle with the retry/self-heal loop below.
                        async {
                            call_batch_fn!(self, data, Self::batch_insert);
                            Ok(())
                        }
                        .await
                    }
                    RowType::Delete => {
                        async {
                            call_batch_fn!(self, data, Self::batch_delete);
                            Ok(())
                        }
                        .await
                    }
                    _ => self.serial_sink(&data).await,
                }
            };

            match exec_res {
                Ok(()) => return Ok(()),
                Err(e) => {
                    let retryable = attempt < max_attempts && self.is_failover_related_error(&e);
                    if retryable {
                        last_err = Some(e);
                        self.reconnect_gaussdb_target(attempt).await?;
                        continue;
                    }
                    return Err(e);
                }
            }
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("pg sinker failed")))
    }

    async fn sink_ddl(&mut self, data: Vec<DdlData>, _batch: bool) -> anyhow::Result<()> {
        let mut rts = LimitedQueue::new(cmp::min(100, data.len()));
        let monitor_interval = if self.monitor_interval > 0 {
            self.monitor_interval
        } else {
            10
        };
        let mut data_size = 0;
        let mut data_len = 0;
        let mut last_monitor_time = Instant::now();

        for ddl_data in data.iter() {
            let (schema, _tb) = ddl_data.get_schema_tb();
            data_size += ddl_data.get_data_size();
            data_len += 1;
            let merged_url =
                ConnectionAuthConfig::merge_url_with_auth(&self.url, &self.connection_auth)
                    .with_context(|| "failed to merge pg url with auth for ddl sink")?;
            let conn_options = PgConnectOptions::from_str(&merged_url)?;
            let mut pool_options = PgPoolOptions::new().max_connections(1);
            let sql = format!("SET search_path = '{}';", schema);

            if !schema.is_empty() {
                match ddl_data.ddl_type {
                    DdlType::CreateSchema | DdlType::DropSchema | DdlType::AlterSchema => {}
                    _ => {
                        pool_options = pool_options.after_connect(move |conn, _meta| {
                            let sql = sql.clone();
                            Box::pin(async move {
                                conn.execute(sql.as_str()).await?;
                                Ok(())
                            })
                        });
                    }
                }
            }

            let sql = ddl_data.to_sql();
            log_info!("sink ddl, schema: {}, sql: {}", schema, sql);

            let start_time = Instant::now();

            let conn_pool = pool_options.connect_with(conn_options).await?;
            let query = sqlx::query(&sql);
            query.execute(&conn_pool).await?;

            rts.push((start_time.elapsed().as_millis() as u64, 1));
            conn_pool.close().await;

            if last_monitor_time.elapsed().as_secs() >= monitor_interval {
                BaseSinker::update_serial_monitor(&self.monitor, data_len as u64, data_size)
                    .await?;
                BaseSinker::update_monitor_rt(&self.monitor, &rts).await?;
                rts.clear();
                data_size = 0;
                data_len = 0;
                last_monitor_time = Instant::now();
            }
        }

        if data_len > 0 || data_size > 0 {
            BaseSinker::update_serial_monitor(&self.monitor, data_len as u64, data_size).await?;
            BaseSinker::update_monitor_rt(&self.monitor, &rts).await?;
        }
        Ok(())
    }

    async fn refresh_meta(&mut self, data: Vec<DdlData>) -> anyhow::Result<()> {
        for ddl_data in data.iter() {
            self.meta_manager.invalidate_cache_by_ddl_data(ddl_data);
        }
        Ok(())
    }

    async fn close(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

impl PgSinker {
    fn is_gaussdb_target(&self) -> bool {
        matches!(
            self.db_type,
            DbType::GaussDBPg | DbType::GaussDBMySQL | DbType::GaussDBOracle
        )
    }

    fn is_failover_related_error(&self, err: &anyhow::Error) -> bool {
        if !self.is_gaussdb_target() {
            return false;
        }
        let msg = format!("{:#}", err).to_lowercase();
        msg.contains("read-only")
            || msg.contains("read only")
            || msg.contains("terminating connection")
            || msg.contains("server closed the connection")
            || msg.contains("connection reset")
            || msg.contains("broken pipe")
            || msg.contains("connection refused")
            || msg.contains("unexpected end of file")
            || msg.contains("pool timed out")
            || msg.contains("timeout expired")
            || msg.contains("operation timed out")
    }

    fn connect_timeout() -> Duration {
        let secs = std::env::var("GAUSSDB_TARGET_CONNECT_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(10);
        Duration::from_secs(secs)
    }

    fn failover_max_wait() -> Duration {
        let secs = std::env::var("GAUSSDB_TARGET_FAILOVER_MAX_WAIT_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(120);
        Duration::from_secs(secs)
    }

    fn parse_candidate_hosts(raw: &str, default_port: u16) -> Vec<(String, u16)> {
        let mut out = Vec::new();
        for candidate in raw.split(',').map(|s| s.trim()) {
            if candidate.is_empty() {
                continue;
            }

            let (host, port) = match candidate.rsplit_once(':') {
                Some((h, p))
                    if !h.is_empty() && !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) =>
                {
                    (h.trim(), p.parse::<u16>().unwrap_or(default_port))
                }
                _ => (candidate, default_port),
            };
            out.push((host.to_string(), port));
        }
        out
    }

    fn candidate_endpoints(
        base_host: &str,
        base_port: u16,
        candidates: &[(String, u16)],
        last_success_endpoint: Option<&(String, u16)>,
    ) -> Vec<(String, u16)> {
        let mut endpoints = Vec::new();
        let mut seen = std::collections::HashSet::<String>::new();

        let mut push = |host: &str, port: u16| {
            let key = format!("{}:{}", host, port);
            if seen.insert(key) {
                endpoints.push((host.to_string(), port));
            }
        };

        // Sticky endpoint first to reduce standby probe noise on reconnect.
        if let Some((host, port)) = last_success_endpoint {
            push(host, *port);
        }

        if !candidates.is_empty() {
            for (host, port) in candidates {
                push(host, *port);
            }
            // Base URL is only a final fallback when all candidates fail.
            push(base_host, base_port);
        } else {
            push(base_host, base_port);
        }

        endpoints
    }

    async fn current_pool(&self) -> Pool<Postgres> {
        self.conn_pool.read().await.clone()
    }

    async fn pool_is_writable(pool: &Pool<Postgres>) -> anyhow::Result<bool> {
        // Failover can temporarily expose coordinators that are "not in recovery" but still
        // reject writes with "read-only transaction". Treat those as not writable.
        //
        // NOTE: Some GaussDB distributions don't implement `current_setting(text, boolean)`,
        // so we probe via `pg_settings` instead.
        let (in_recovery, transaction_ro, default_transaction_ro): (
            bool,
            Option<String>,
            Option<String>,
        ) = sqlx::query_as(
            "SELECT \
                pg_is_in_recovery()::bool, \
                (SELECT setting FROM pg_settings WHERE name='transaction_read_only')::text, \
                (SELECT setting FROM pg_settings WHERE name='default_transaction_read_only')::text",
        )
        .fetch_one(pool)
        .await?;

        let is_on = |v: &Option<String>| v.as_deref().is_some_and(|s| s.eq_ignore_ascii_case("on"));
        Ok(!in_recovery && !is_on(&transaction_ro) && !is_on(&default_transaction_ro))
    }

    async fn reconnect_gaussdb_target(&self, attempt: u32) -> anyhow::Result<()> {
        if !self.is_gaussdb_target() {
            return Ok(());
        }

        // Avoid thundering-herd reconnects from parallel sinkers.
        let _guard = self.reconnect_lock.lock().await;

        let merged_url =
            ConnectionAuthConfig::merge_url_with_auth(&self.url, &self.connection_auth)
                .with_context(|| "failed to merge sinker url with auth")?;
        let url_info = Url::parse(&merged_url)?;
        let base_host = url_info
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("missing host in sinker url"))?
            .to_string();
        let base_port = url_info
            .port()
            .ok_or_else(|| anyhow::anyhow!("missing port in sinker url"))?;

        let raw_candidates = std::env::var("gaussdb_pg_candidate_hosts").unwrap_or_default();
        let parsed_candidates = Self::parse_candidate_hosts(&raw_candidates, base_port);
        let prefer_candidates = !parsed_candidates.is_empty();

        let last_success = self.last_success_endpoint.read().await.clone();
        let endpoints = Self::candidate_endpoints(
            &base_host,
            base_port,
            &parsed_candidates,
            last_success.as_ref(),
        );

        let started = std::time::Instant::now();
        let timeout_per = Self::connect_timeout();
        let max_wait = Self::failover_max_wait();
        let mut last_err: Option<anyhow::Error> = None;

        log_info!(
            "gaussdb target reconnect: attempt={} prefer_candidates={} candidates={} base={}:{} last_success={}",
            attempt,
            prefer_candidates,
            parsed_candidates.len(),
            base_host,
                base_port,
                last_success
                    .as_ref()
                    .map(|(h, p)| format!("{}:{}", h, p))
                    .unwrap_or_else(|| "none".to_string())
        );
        if prefer_candidates {
            log_info!("gaussdb target candidates (sql ports): {}", raw_candidates);
        }
        log_info!(
            "gaussdb target probe order: {}",
            endpoints
                .iter()
                .map(|(h, p)| format!("{}:{}", h, p))
                .collect::<Vec<_>>()
                .join(",")
        );

        while started.elapsed() < max_wait {
            for (host, port) in &endpoints {
                let mut u = Url::parse(&merged_url)?;
                u.set_host(Some(host))
                    .map_err(|_| anyhow::anyhow!("failed to set host in url"))?;
                u.set_port(Some(*port))
                    .map_err(|_| anyhow::anyhow!("failed to set port in url"))?;
                let candidate_url = u.to_string();

                let pool = match self
                    .create_pool_with_timeout(&candidate_url, timeout_per)
                    .await
                {
                    Ok(v) => v,
                    Err(e) => {
                        log_warn!(
                            "gaussdb target candidate connect failed: {}:{}, error: {}",
                            host,
                            port,
                            e
                        );
                        last_err = Some(e);
                        continue;
                    }
                };

                match Self::pool_is_writable(&pool).await {
                    Ok(true) => {
                        {
                            let mut w = self.conn_pool.write().await;
                            *w = pool;
                        }
                        {
                            let mut w = self.last_success_endpoint.write().await;
                            *w = Some((host.clone(), *port));
                        }
                        log_info!("gaussdb target write pool switched: {}:{}", host, port);
                        return Ok(());
                    }
                    Ok(false) => {
                        log_warn!(
                            "gaussdb target candidate is standby/read-only: {}:{}",
                            host,
                            port
                        );
                        continue;
                    }
                    Err(e) => {
                        log_warn!(
                            "gaussdb target candidate precheck failed: {}:{}, error: {}",
                            host,
                            port,
                            e
                        );
                        last_err = Some(e);
                        continue;
                    }
                }
            }

            // Backoff a bit and retry probing, allowing CM to converge.
            let sleep_millis = 500_u64.saturating_mul(attempt as u64);
            tokio::time::sleep(Duration::from_millis(sleep_millis)).await;
        }

        Err(last_err.unwrap_or_else(|| {
            anyhow::anyhow!("gaussdb target reconnect failed: no writable endpoint found")
        }))
    }

    async fn create_pool_with_timeout(
        &self,
        url: &str,
        timeout_per: Duration,
    ) -> anyhow::Result<Pool<Postgres>> {
        let mut conn_options = PgConnectOptions::from_str(url)?;
        conn_options
            .log_statements(log::LevelFilter::Debug)
            .log_slow_statements(log::LevelFilter::Debug, Duration::from_secs(1));
        if !self.enable_sqlx_log {
            conn_options.disable_statement_logging();
        }

        let mut pool_options = PgPoolOptions::new()
            .max_connections(self.max_connections)
            .test_before_acquire(true)
            .acquire_timeout(Duration::from_secs(120));

        if self.disable_foreign_key_checks {
            pool_options = pool_options.after_connect(move |conn, _meta| {
                Box::pin(async move {
                    if let Err(e) = conn.execute("SET session_replication_role = 'replica';").await {
                        log_warn!(
                            "Failed to disable foreign key checks (user may lack superuser/replication role): {}. \
                            Foreign key constraints will remain enabled.",
                            e
                        );
                    }
                    Ok(())
                })
            });
        }

        let pool = timeout(timeout_per, pool_options.connect_with(conn_options))
            .await
            .with_context(|| format!("gaussdb target connect timeout after {:?}", timeout_per))??;
        Ok(pool)
    }

    async fn serial_sink(&mut self, data: &[RowData]) -> anyhow::Result<()> {
        let (schema, tb) = data
            .first()
            .map(|v| (v.schema.as_str(), v.tb.as_str()))
            .unwrap_or(("", ""));
        let monitor_interval = if self.monitor_interval > 0 {
            self.monitor_interval
        } else {
            10
        };
        let mut data_size = 0;
        let mut data_len = 0;
        let mut last_monitor_time = Instant::now();

        let conn_pool = self.current_pool().await;
        let mut tx = conn_pool
            .begin()
            .await
            .with_context(|| format!("failed to begin transaction for {}.{}", schema, tb))?;
        if let Some(sql) = self.get_data_marker_sql().await {
            sqlx::query(&sql)
                .execute(&mut tx)
                .await
                .with_context(|| format!("failed to execute data marker sql: [{}]", sql))?;
        }
        let mut rts = LimitedQueue::new(cmp::min(100, data.len()));
        for row_data in data.iter() {
            data_size += row_data.data_size;

            let tb_meta = self
                .meta_manager
                .get_tb_meta_by_row_data(row_data)
                .await
                .with_context(|| {
                    format!(
                        "failed to get table metadata for serial sink: {}.{}",
                        row_data.schema, row_data.tb
                    )
                })?;
            let query_builder =
                RdbQueryBuilder::new_for_pg_compatible(tb_meta, None, self.db_type.clone());

            let query_info = query_builder.get_query_info(row_data, self.replace)?;
            let query = query_builder.create_pg_query(&query_info)?;

            let start_time = Instant::now();
            query
                .execute(&mut tx)
                .await
                .with_context(|| format!("serial sink failed, row_data: [{}]", row_data))?;

            rts.push((start_time.elapsed().as_millis() as u64, 1));
            if last_monitor_time.elapsed().as_secs() >= monitor_interval {
                BaseSinker::update_serial_monitor(&self.monitor, data_len as u64, data_size as u64)
                    .await?;
                BaseSinker::update_monitor_rt(&self.monitor, &rts).await?;
                rts.clear();
                data_size = 0;
                data_len = 0;
                last_monitor_time = Instant::now();
            }
        }
        tx.commit()
            .await
            .with_context(|| format!("failed to commit transaction for {}.{}", schema, tb))?;

        if data_len > 0 || data_size > 0 {
            BaseSinker::update_serial_monitor(&self.monitor, data_len as u64, data_size as u64)
                .await?;
            BaseSinker::update_monitor_rt(&self.monitor, &rts).await?;
        }
        Ok(())
    }

    async fn batch_delete(
        &mut self,
        data: &mut [RowData],
        start_index: usize,
        batch_size: usize,
    ) -> anyhow::Result<()> {
        let tb_meta = self.meta_manager.get_tb_meta_by_row_data(&data[0]).await?;
        let query_builder =
            RdbQueryBuilder::new_for_pg_compatible(tb_meta, None, self.db_type.clone());

        let (query_info, data_size) =
            query_builder.get_batch_delete_query(data, start_index, batch_size)?;
        let query = query_builder.create_pg_query(&query_info)?;

        let start_time = Instant::now();
        let mut rts = LimitedQueue::new(1);
        let conn_pool = self.current_pool().await;
        if let Some(sql) = self.get_data_marker_sql().await {
            let mut tx = conn_pool.begin().await?;
            sqlx::query(&sql).execute(&mut tx).await?;
            query.execute(&mut tx).await?;
            tx.commit().await?;
        } else {
            query.execute(&conn_pool).await?;
        }
        rts.push((start_time.elapsed().as_millis() as u64, 1));

        BaseSinker::update_batch_monitor(&self.monitor, batch_size as u64, data_size as u64)
            .await?;
        BaseSinker::update_monitor_rt(&self.monitor, &rts).await
    }

    async fn batch_insert(
        &mut self,
        data: &mut [RowData],
        start_index: usize,
        batch_size: usize,
    ) -> anyhow::Result<()> {
        let schema = data[0].schema.clone();
        let tb = data[0].tb.clone();
        let tb_meta = self
            .meta_manager
            .get_tb_meta_by_row_data(&data[0])
            .await
            .with_context(|| {
                format!(
                    "failed to get table metadata for batch insert: {}.{}",
                    schema, tb
                )
            })?
            .to_owned();
        let query_builder =
            RdbQueryBuilder::new_for_pg_compatible(&tb_meta, None, self.db_type.clone());

        let (query_info, data_size) =
            query_builder.get_batch_insert_query(data, start_index, batch_size, self.replace)?;
        let query = query_builder
            .create_pg_query(&query_info)
            .with_context(|| {
                format!(
                    "failed to create pg query for batch insert: {}.{}",
                    tb_meta.basic.schema, tb_meta.basic.tb
                )
            })?;

        let start_time = Instant::now();
        let mut rts = LimitedQueue::new(1);
        let conn_pool = self.current_pool().await;
        let exec_result: anyhow::Result<()> = if let Some(sql) = self.get_data_marker_sql().await {
            let mut tx = conn_pool.begin().await.with_context(|| {
                format!("failed to begin tx for batch insert: {}.{}", schema, tb)
            })?;
            sqlx::query(&sql)
                .execute(&mut tx)
                .await
                .with_context(|| format!("failed to execute data marker sql: [{}]", sql))?;
            query.execute(&mut tx).await.with_context(|| {
                format!(
                    "batch insert execute failed (with data marker): {}.{} sql=[{}]",
                    schema, tb, query_info.sql
                )
            })?;
            tx.commit().await.with_context(|| {
                format!("failed to commit tx for batch insert: {}.{}", schema, tb)
            })?;
            Ok(())
        } else {
            query
                .execute(&conn_pool)
                .await
                .map(|_| ())
                .with_context(|| {
                    format!(
                        "batch insert execute failed: {}.{} sql=[{}]",
                        schema, tb, query_info.sql
                    )
                })
        };

        if let Err(error) = exec_result {
            log_error!(
                "batch insert failed, will insert one by one, schema: {}, tb: {}, error: {:#}",
                tb_meta.basic.schema,
                tb_meta.basic.tb,
                error
            );
            let sub_data = &data[start_index..start_index + batch_size];
            self.serial_sink(sub_data).await?;
        } else {
            rts.push((start_time.elapsed().as_millis() as u64, 1));
        }

        BaseSinker::update_batch_monitor(&self.monitor, batch_size as u64, data_size as u64)
            .await?;
        BaseSinker::update_monitor_rt(&self.monitor, &rts).await
    }

    async fn get_data_marker_sql(&self) -> Option<String> {
        if let Some(data_marker) = &self.data_marker {
            let data_marker = data_marker.read().await;
            // CREATE TABLE ape_trans_pg.topo1 (
            //     data_origin_node varchar(255) NOT NULL,
            //     src_node varchar(255) NOT NULL,
            //     dst_node varchar(255) NOT NULL,
            //     n bigint DEFAULT NULL,
            //     PRIMARY KEY (data_origin_node, src_node, dst_node)
            //   );
            let sql = format!(
                r#"INSERT INTO "{}"."{}"(data_origin_node, src_node, dst_node, n)
                VALUES('{}', '{}', '{}', 1) 
                ON CONFLICT (data_origin_node, src_node, dst_node) 
                DO UPDATE SET n="{}"."{}".n+1"#,
                data_marker.marker_schema,
                data_marker.marker_tb,
                data_marker.data_origin_node,
                data_marker.src_node,
                data_marker.dst_node,
                data_marker.marker_schema,
                data_marker.marker_tb,
            );
            Some(sql)
        } else {
            None
        }
    }
}
