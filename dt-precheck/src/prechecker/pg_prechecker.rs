use std::collections::HashSet;
use std::time::Duration;

use anyhow::bail;
use async_trait::async_trait;
use dt_common::config::{
    config_enums::DbType, connection_auth_config::ConnectionAuthConfig, filter_config::FilterConfig,
};
use dt_task::task_util::TaskUtil;
use sqlx::Row;
use tokio::net::TcpStream;
use url::Url;

use crate::{
    config::precheck_config::PrecheckConfig,
    fetcher::{postgresql::pg_fetcher::PgFetcher, traits::Fetcher},
    meta::{
        check_item::CheckItem, check_result::CheckResult, db_table_model::DbTable,
        pg_enums::ConstraintTypeEnum,
    },
};

use super::{basic::BasicPrechecker, traits::Prechecker};

const PG_SUPPORT_DB_VERSION_NUM_MIN: i32 = 120000;

pub struct PostgresqlPrechecker {
    pub db_type: DbType,
    pub fetcher: PgFetcher,
    pub filter_config: FilterConfig,
    pub precheck_config: PrecheckConfig,
    pub is_source: bool,
    pub slot_name: Option<String>,
    pub selected_endpoint: Option<(String, u16)>,
}

#[async_trait]
impl Prechecker for PostgresqlPrechecker {
    async fn build_connection(&mut self) -> anyhow::Result<CheckResult> {
        let mut check_error = None;

        // For GaussDBPg CDC source, bind the precheck connection to a RW primary endpoint
        // (prefer candidate list if configured). This avoids "connected but in recovery" cases.
        if matches!(self.db_type, DbType::GaussDBPg)
            && self.is_source
            && self.precheck_config.do_cdc
        {
            if let Err(e) = self.bind_to_gaussdb_primary_endpoint().await {
                check_error = Some(e);
            }
        }

        if check_error.is_none() {
            let result = self.fetcher.build_connection().await;
            if let Err(e) = result {
                check_error = Some(e);
            }
        }

        Ok(CheckResult::build_with_err(
            CheckItem::CheckDatabaseConnection,
            self.is_source,
            self.db_type.clone(),
            check_error,
            None,
        ))
    }

    async fn check_database_version(&mut self) -> anyhow::Result<CheckResult> {
        let mut check_error = None;
        let mut warn_msg = None;

        let result = self.fetcher.fetch_version().await;
        match result {
            Ok(version) => {
                if version.is_empty() {
                    check_error = Some(anyhow::Error::msg("found no version info"));
                } else {
                    match version.parse::<i32>() {
                        Ok(version_i32) => {
                            warn_msg = Some(anyhow::Error::msg(format!(
                                "server_version_num: {}",
                                version_i32
                            )));

                            // For PostgreSQL we keep the existing minimum version gate.
                            // For GaussDB (PG compatible mode) do not hardcode a PG version gate.
                            if matches!(self.db_type, DbType::Pg)
                                && version_i32 < PG_SUPPORT_DB_VERSION_NUM_MIN
                            {
                                check_error = Some(anyhow::Error::msg(format!(
                                    "version:{} is not supported yet",
                                    version_i32
                                )));
                            }
                        }
                        Err(_) => {
                            // server_version_num should be numeric; treat it as hard error for PG,
                            // but only warn for GaussDB to keep precheck usable.
                            if matches!(self.db_type, DbType::Pg) {
                                check_error = Some(anyhow::Error::msg(format!(
                                    "invalid server_version_num: {}",
                                    version
                                )));
                            } else {
                                warn_msg = Some(anyhow::Error::msg(format!(
                                    "server_version_num: {}",
                                    version
                                )));
                            }
                        }
                    }
                }
            }
            Err(e) => check_error = Some(e),
        }

        Ok(CheckResult::build_with_err(
            CheckItem::CheckDatabaseVersionSupported,
            self.is_source,
            self.db_type.clone(),
            check_error,
            warn_msg,
        ))
    }

    async fn check_permission(&mut self) -> anyhow::Result<CheckResult> {
        Ok(CheckResult::build(
            CheckItem::CheckAccountPermission,
            self.is_source,
        ))
    }

    async fn check_cdc_supported(&mut self) -> anyhow::Result<CheckResult> {
        let mut check_error = None;

        if !self.is_source {
            // do nothing when the database is target
            return Ok(CheckResult::build_with_err(
                CheckItem::CheckIfDatabaseSupportCdc,
                self.is_source,
                self.db_type.clone(),
                check_error,
                None,
            ));
        }

        let mut err_msgs: Vec<String> = vec![];

        // check the cdc settings (PostgreSQL-compatible)
        let configs: Vec<String> = ["wal_level", "max_wal_senders", "max_replication_slots"]
            .iter()
            .map(|c| c.to_string())
            .collect();
        let mut max_replication_slots_i32: Option<i32> = None;
        match self.fetcher.fetch_configuration(configs).await {
            Ok(rows) => {
                for (k, v) in rows {
                    match k.as_str() {
                        "wal_level" => {
                            if v.is_empty() {
                                err_msgs.push("wal_level is empty".to_string());
                            } else if v.to_lowercase() != "logical" {
                                err_msgs.push(format!(
                                    "wal_level should not be '{}', need to be 'logical'.",
                                    v
                                ))
                            }
                        }
                        "max_replication_slots" => match v.parse::<i32>() {
                            Ok(n) => {
                                max_replication_slots_i32 = Some(n);
                                if n < 1 {
                                    err_msgs.push(format!(
                                        "max_replication_slots needs to be greater than 0. current is '{}'",
                                        n
                                    ))
                                }
                            }
                            Err(_) => {
                                err_msgs.push(format!(
                                    "max_replication_slots is invalid, current is '{}'",
                                    v
                                ));
                            }
                        },
                        "max_wal_senders" => match v.parse::<i32>() {
                            Ok(sender_i32) => {
                                if sender_i32 < 1 {
                                    err_msgs.push(format!(
                                        "max_wel_senders needs to be greater than 0, current is '{}'",
                                        sender_i32
                                    ))
                                }
                            }
                            Err(_) => {
                                err_msgs.push(format!(
                                    "max_wal_senders is invalid, current is '{}'",
                                    v
                                ));
                            }
                        },
                        _ => {}
                    }
                }
            }
            Err(e) => {
                check_error = Some(e);
            }
        }

        if check_error.is_none() {
            // check the slot count is less than max_replication_slots or not
            if err_msgs.is_empty() {
                if let Some(max_replication_slots_i32) = max_replication_slots_i32 {
                    let slot_result = self.fetcher.fetch_slot_names().await;
                    match slot_result {
                        Ok(slots) => {
                            if max_replication_slots_i32 == (slots.len() as i32) {
                                check_error = Some(anyhow::Error::msg(format!(
                                    "the current number of slots:[{}] has reached max_replication_slots, and new slots cannot be created",
                                    max_replication_slots_i32
                                )));
                            }
                        }
                        Err(e) => check_error = Some(e),
                    }
                }
            }
        }

        // Additional fail-fast checks for GaussDBPg CDC source.
        if check_error.is_none() && matches!(self.db_type, DbType::GaussDBPg) {
            if let Some(pool) = &self.fetcher.pool {
                // Standby/recovery mode cannot do logical decoding.
                match sqlx::query("SELECT pg_is_in_recovery() AS in_recovery")
                    .fetch_one(pool)
                    .await
                {
                    Ok(row) => match row.try_get::<bool, _>("in_recovery") {
                        Ok(true) => err_msgs.push(
                            "GaussDB is in recovery/standby mode (pg_is_in_recovery=true), logical decoding is not supported"
                                .to_string(),
                        ),
                        Ok(false) => {}
                        Err(e) => err_msgs.push(format!(
                            "failed to parse pg_is_in_recovery() result: {}",
                            e
                        )),
                    },
                    Err(e) => err_msgs.push(format!(
                        "failed to query pg_is_in_recovery() for GaussDB primary check: {}",
                        e
                    )),
                }

                // Permission gate: need superuser or replication role.
                let perm_sql =
                    "SELECT current_user::text AS current_user, r.rolsuper, r.rolreplication \
                                FROM pg_roles r WHERE r.rolname = current_user";
                match sqlx::query(perm_sql).fetch_one(pool).await {
                    Ok(row) => {
                        let current_user: String = row
                            .try_get("current_user")
                            .unwrap_or_else(|_| "<unknown>".to_string());
                        let rolsuper: bool = row.try_get("rolsuper").unwrap_or(false);
                        let rolreplication: bool = row.try_get("rolreplication").unwrap_or(false);
                        if !rolsuper && !rolreplication {
                            err_msgs.push(format!(
                                "insufficient permission for CDC: current_user={} rolsuper=false rolreplication=false (need superuser or replication role)",
                                current_user
                            ));
                        }
                    }
                    Err(e) => err_msgs.push(format!(
                        "failed to query current user permissions (rolsuper/rolreplication): {}",
                        e
                    )),
                }

                // Slot-active check (no creation side effects).
                if let Some(slot_name) = self.slot_name.as_ref().filter(|s| !s.trim().is_empty()) {
                    let slot_sql =
                        "SELECT active FROM pg_catalog.pg_replication_slots WHERE slot_name = $1";
                    match sqlx::query_scalar::<_, bool>(slot_sql)
                        .bind(slot_name)
                        .fetch_optional(pool)
                        .await
                    {
                        Ok(Some(true)) => err_msgs.push(format!(
                            "replication slot '{}' is active; stop the old task or use a different slot_name",
                            slot_name
                        )),
                        Ok(Some(false)) => {}
                        Ok(None) => {}
                        Err(e) => err_msgs.push(format!(
                            "failed to check replication slot '{}' active status: {}",
                            slot_name, e
                        )),
                    }
                }
            } else {
                err_msgs.push("internal error: pg connection pool is not initialized".to_string());
            }

            // HA port reachability (sql_port + 1) for replication.
            match self.selected_sql_endpoint() {
                Ok((host, sql_port)) => {
                    let ha_port = sql_port.saturating_add(1);
                    let addr = format!("{}:{}", host, ha_port);
                    match tokio::time::timeout(Duration::from_secs(2), TcpStream::connect(&addr))
                        .await
                    {
                        Ok(Ok(_)) => {}
                        Ok(Err(e)) => err_msgs.push(format!(
                            "GaussDB HA replication port is not reachable: {} (sql_port={}, ha_port={}), error: {}",
                            addr, sql_port, ha_port, e
                        )),
                        Err(_) => err_msgs.push(format!(
                            "GaussDB HA replication port connect timed out: {} (sql_port={}, ha_port={})",
                            addr, sql_port, ha_port
                        )),
                    }
                }
                Err(e) => err_msgs.push(format!(
                    "failed to resolve selected sql endpoint for HA port check: {}",
                    e
                )),
            }
        }

        if check_error.is_none() && !err_msgs.is_empty() {
            check_error = Some(anyhow::Error::msg(err_msgs.join(";")));
        }

        Ok(CheckResult::build_with_err(
            CheckItem::CheckIfDatabaseSupportCdc,
            self.is_source,
            self.db_type.clone(),
            check_error,
            None,
        ))
    }

    async fn check_struct_existed_or_not(&mut self) -> anyhow::Result<CheckResult> {
        let mut check_error = None;

        if !self.is_source && self.precheck_config.do_struct_init {
            return Ok(CheckResult::build_with_err(
                CheckItem::CheckIfStructExisted,
                self.is_source,
                self.db_type.clone(),
                check_error,
                None,
            ));
        }

        let is_filter_pattern =
            BasicPrechecker::is_filter_pattern(self.db_type.clone(), &self.fetcher.filter);
        if is_filter_pattern {
            return Ok(CheckResult::build_with_err(
                CheckItem::CheckIfStructExisted,
                self.is_source,
                self.db_type.clone(),
                check_error,
                Some(anyhow::Error::msg(
                    "CheckIfStructExisted with filter in pattern is not supported.",
                )),
            ));
        }

        let (mut db_tables, mut err_msgs): (Vec<DbTable>, Vec<String>) = (Vec::new(), Vec::new());
        if !self.filter_config.do_tbs.is_empty() {
            DbTable::from_str(&self.filter_config.do_tbs, &mut db_tables)
        } else if !self.filter_config.do_schemas.is_empty() {
            DbTable::from_str(&self.filter_config.do_schemas, &mut db_tables)
        }

        let (schemas, tb_schemas, tbs) = DbTable::get_config_maps(&db_tables).unwrap();
        let mut all_schemas = Vec::new();
        all_schemas.extend(&schemas);
        all_schemas.extend(&tb_schemas);

        // When a specific table to be migrated is specified and the following conditions are met, check the existence of the table
        // 1. this check is for the source database
        // 2. this check is for the sink database, and specified no structure initialization
        if !tbs.is_empty() {
            let mut not_existed_tbs: HashSet<String> = HashSet::new();

            let table_result = self.fetcher.fetch_tables().await;
            let current_tbs: HashSet<String> = match table_result {
                Ok(tables) => tables
                    .iter()
                    .map(|t| format!("{}.{}", t.schema_name, t.table_name))
                    .collect(),
                Err(e) => bail! {e},
            };
            for tb_key in tbs {
                if !current_tbs.contains(&tb_key) {
                    not_existed_tbs.insert(tb_key);
                }
            }
            if !not_existed_tbs.is_empty() {
                err_msgs.push(format!(
                    "tables not existed: [{}]",
                    not_existed_tbs
                        .iter()
                        .map(|e| e.to_string())
                        .collect::<Vec<String>>()
                        .join(";")
                ));
            }
        }

        if !all_schemas.is_empty() {
            let mut not_existed_schema: HashSet<String> = HashSet::new();
            let schema_result = self.fetcher.fetch_schemas().await;
            let current_schemas: HashSet<String> = match schema_result {
                Ok(schemas) => schemas.iter().map(|s| s.schema_name.clone()).collect(),
                Err(e) => bail! {e},
            };

            for schema in all_schemas {
                if !current_schemas.contains(schema) {
                    not_existed_schema.insert(schema.clone());
                }
            }
            if !not_existed_schema.is_empty() {
                err_msgs.push(format!(
                    "schemas not existed: [{}]",
                    not_existed_schema
                        .iter()
                        .map(|e| e.to_string())
                        .collect::<Vec<String>>()
                        .join(";")
                ));
            }
        }

        if !err_msgs.is_empty() {
            check_error = Some(anyhow::Error::msg(err_msgs.join(".")))
        }

        Ok(CheckResult::build_with_err(
            CheckItem::CheckIfStructExisted,
            self.is_source,
            self.db_type.clone(),
            check_error,
            None,
        ))
    }

    async fn check_table_structs(&mut self) -> anyhow::Result<CheckResult> {
        // all tables have a pk, and have no fk
        let (mut check_error, mut warn_error) = (None, None);

        if !self.is_source && self.precheck_config.do_struct_init {
            // do nothing when the database is a target
            return Ok(CheckResult::build_with_err(
                CheckItem::CheckIfTableStructSupported,
                self.is_source,
                self.db_type.clone(),
                check_error,
                None,
            ));
        }

        let is_filter_pattern =
            BasicPrechecker::is_filter_pattern(self.db_type.clone(), &self.fetcher.filter);
        if is_filter_pattern {
            return Ok(CheckResult::build_with_err(
                CheckItem::CheckIfTableStructSupported,
                self.is_source,
                self.db_type.clone(),
                check_error,
                Some(anyhow::Error::msg(
                    "CheckIfTableStructSupported with filter in pattern is not supported.",
                )),
            ));
        }

        let (mut db_tables, mut err_msgs, mut warn_msgs): (Vec<DbTable>, Vec<String>, Vec<String>) =
            (Vec::new(), Vec::new(), Vec::new());
        if !self.filter_config.do_tbs.is_empty() {
            DbTable::from_str(&self.filter_config.do_tbs, &mut db_tables)
        } else if !self.filter_config.do_schemas.is_empty() {
            DbTable::from_str(&self.filter_config.do_schemas, &mut db_tables)
        }

        let (schemas, tb_schemas, _) = DbTable::get_config_maps(&db_tables).unwrap();
        let mut all_schemas = Vec::new();
        all_schemas.extend(&schemas);
        all_schemas.extend(&tb_schemas);
        if all_schemas.is_empty() {
            println!("found no schema need to do migrate, very strange");
            bail! {
            "found no schema need to do migrate"};
        }

        let (mut has_pkuk_tables, mut fkref_nonexists_tables): (HashSet<String>, HashSet<String>) =
            (HashSet::new(), HashSet::new());

        let table_result = self.fetcher.fetch_tables().await;
        let current_tables: HashSet<String> = match table_result {
            Ok(tables) => tables
                .iter()
                .map(|t| format!("{}.{}", t.schema_name, t.table_name))
                .collect(),
            Err(e) => bail! {e},
        };

        let constraint_result = self.fetcher.fetch_constraints().await;
        match constraint_result {
            Ok(constraints) => {
                for c in constraints {
                    let schema_table_name = format!("{}.{}", c.schema_name, c.table_name);
                    if c.constraint_type == ConstraintTypeEnum::Primary.to_str().unwrap()
                        || c.constraint_type == ConstraintTypeEnum::Unique.to_str().unwrap()
                    {
                        has_pkuk_tables.insert(schema_table_name);
                    } else if c.constraint_type == ConstraintTypeEnum::Foreign.to_str().unwrap()
                        && self
                            .fetcher
                            .filter
                            .filter_tb(c.rel_schema_name.as_str(), &c.rel_table_name)
                    {
                        fkref_nonexists_tables.insert(schema_table_name);
                    }
                }
            }
            Err(e) => bail! {e},
        }

        if !fkref_nonexists_tables.is_empty() {
            err_msgs.push(format!(
                "the following foreign key dependent tables are not defined in the replication object:[{}]",
                fkref_nonexists_tables
                    .iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<String>>()
                    .join(";")
            ));
        }

        let mut no_pkuk_tables: HashSet<String> = HashSet::new();
        for current_table in current_tables {
            if !has_pkuk_tables.contains(&current_table) {
                no_pkuk_tables.insert(current_table);
            }
        }
        if !no_pkuk_tables.is_empty() {
            warn_msgs.push(format!(
                "primary key or unique key are needed, but these tables don't have any:[{}]",
                no_pkuk_tables
                    .iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<String>>()
                    .join(";")
            ));
        }
        if !err_msgs.is_empty() {
            check_error = Some(anyhow::Error::msg(err_msgs.join(";")))
        }
        if !warn_msgs.is_empty() {
            warn_error = Some(anyhow::Error::msg(warn_msgs.join(";")))
        }

        Ok(CheckResult::build_with_err(
            CheckItem::CheckIfTableStructSupported,
            self.is_source,
            self.db_type.clone(),
            check_error,
            warn_error,
        ))
    }
}

impl PostgresqlPrechecker {
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

    fn rewrite_url_host_port(original_url: &str, host: &str, port: u16) -> anyhow::Result<String> {
        let mut u = Url::parse(original_url)?;
        u.set_host(Some(host))?;
        u.set_port(Some(port))
            .map_err(|()| anyhow::anyhow!("invalid port in url rewrite: {}", port))?;
        Ok(u.to_string())
    }

    async fn probe_pg_is_in_recovery(
        url: &str,
        connection_auth: &ConnectionAuthConfig,
    ) -> anyhow::Result<bool> {
        // Fail fast on unreachable candidates to avoid long stalls when some nodes are down.
        let timeout = Duration::from_secs(
            std::env::var("GAUSSDB_PRECHECK_CONNECT_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(10),
        );

        let pool = tokio::time::timeout(
            timeout,
            TaskUtil::create_pg_conn_pool(url, connection_auth, 1, true, false),
        )
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "gaussdb precheck connect timeout after {:?} (url={})",
                timeout,
                url
            )
        })??;

        let row = tokio::time::timeout(
            timeout,
            sqlx::query("SELECT pg_is_in_recovery() AS in_recovery").fetch_one(&pool),
        )
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "gaussdb precheck query timeout after {:?} (url={})",
                timeout,
                url
            )
        })??;

        // Prefer bool, fall back to string parsing for compatibility.
        let in_recovery = if let Ok(v) = row.try_get::<bool, _>("in_recovery") {
            v
        } else if let Ok(s) = row.try_get::<String, _>("in_recovery") {
            let s = s.trim().to_ascii_lowercase();
            s == "t" || s == "true" || s == "1"
        } else {
            false
        };
        pool.close().await;
        Ok(in_recovery)
    }

    fn selected_sql_endpoint(&self) -> anyhow::Result<(String, u16)> {
        if let Some((host, port)) = self.selected_endpoint.as_ref() {
            return Ok((host.clone(), *port));
        }
        let u = Url::parse(&self.fetcher.url)?;
        let host = u
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("missing host in url: {}", self.fetcher.url))?
            .to_string();
        let port = u
            .port()
            .ok_or_else(|| anyhow::anyhow!("missing port in url: {}", self.fetcher.url))?;
        Ok((host, port))
    }

    async fn bind_to_gaussdb_primary_endpoint(&mut self) -> anyhow::Result<()> {
        let base = Url::parse(&self.fetcher.url)?;
        let base_host = base
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("missing host in gaussdb url: {}", self.fetcher.url))?
            .to_string();
        let base_port = base
            .port()
            .ok_or_else(|| anyhow::anyhow!("missing port in gaussdb url: {}", self.fetcher.url))?;

        let raw_candidates = std::env::var("gaussdb_pg_candidate_hosts").unwrap_or_default();
        let parsed_candidates = Self::parse_candidate_hosts(&raw_candidates, base_port);

        let mut endpoints = Vec::<(String, u16)>::new();
        let mut seen = HashSet::<String>::new();
        let mut push = |host: &str, port: u16| {
            let key = format!("{}:{}", host, port);
            if seen.insert(key) {
                endpoints.push((host.to_string(), port));
            }
        };

        if !parsed_candidates.is_empty() {
            println!(
                "gaussdb precheck: gaussdb_pg_candidate_hosts (sql ports) = {}",
                raw_candidates
            );
            for (h, p) in &parsed_candidates {
                push(h, *p);
            }
            // Base URL is only a final fallback when all candidates fail.
            push(&base_host, base_port);
        } else {
            push(&base_host, base_port);
        }

        let mut probe_results: Vec<String> = Vec::new();
        for (host, port) in endpoints.iter() {
            let candidate_url = Self::rewrite_url_host_port(&self.fetcher.url, host, *port)?;
            match Self::probe_pg_is_in_recovery(&candidate_url, &self.fetcher.connection_auth).await
            {
                Ok(true) => {
                    probe_results
                        .push(format!("{}:{} standby(pg_is_in_recovery=true)", host, port));
                    continue;
                }
                Ok(false) => {
                    println!(
                        "gaussdb precheck: selected RW primary endpoint: {}:{}",
                        host, port
                    );
                    self.fetcher.url = candidate_url;
                    self.selected_endpoint = Some((host.clone(), *port));
                    return Ok(());
                }
                Err(e) => {
                    probe_results.push(format!("{}:{} connect_failed({})", host, port, e));
                    continue;
                }
            }
        }

        bail!(
            "no RW primary found for GaussDBPg CDC. probe_results=[{}]",
            probe_results.join("; ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_candidate_hosts_supports_missing_port_and_spaces() {
        let out = PostgresqlPrechecker::parse_candidate_hosts(" 10.0.0.1:8000,10.0.0.2  ,", 8000);
        assert_eq!(
            out,
            vec![
                ("10.0.0.1".to_string(), 8000),
                ("10.0.0.2".to_string(), 8000)
            ]
        );
    }

    #[test]
    fn rewrite_url_host_port_preserves_path_and_query() {
        let original = "postgres://10.0.0.1:8000/postgres?options[statement_timeout]=10s";
        let rewritten =
            PostgresqlPrechecker::rewrite_url_host_port(original, "10.0.0.2", 8001).unwrap();
        assert!(rewritten.contains("10.0.0.2:8001"));
        assert!(rewritten.contains("/postgres"));
        assert!(rewritten.contains("statement_timeout"));
    }
}
