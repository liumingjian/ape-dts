use std::collections::HashSet;

use anyhow::bail;
use async_trait::async_trait;
use dt_common::{
    config::{config_enums::DbType, filter_config::FilterConfig},
    rdb_filter::RdbFilter,
};

use crate::{
    config::precheck_config::PrecheckConfig,
    fetcher::traits::Fetcher,
    fetcher::oracle::oracle_fetcher::OracleFetcher,
    meta::{check_item::CheckItem, check_result::CheckResult, db_table_model::DbTable},
    prechecker::basic::BasicPrechecker,
};

use super::traits::Prechecker;

pub struct OraclePrechecker {
    pub db_type: DbType,
    pub fetcher: OracleFetcher,
    pub filter_config: FilterConfig,
    pub precheck_config: PrecheckConfig,
    pub is_source: bool,
}

#[async_trait]
impl Prechecker for OraclePrechecker {
    async fn build_connection(&mut self) -> anyhow::Result<CheckResult> {
        self.fetcher.build_connection().await?;
        Ok(CheckResult::build_with_err(
            CheckItem::CheckDatabaseConnection,
            self.is_source,
            self.db_type.clone(),
            None,
            None,
        ))
    }

    async fn check_database_version(&mut self) -> anyhow::Result<CheckResult> {
        let mut check_error = None;
        match self.fetcher.fetch_version().await {
            Ok(version) => {
                if version.trim().is_empty() {
                    check_error = Some(anyhow::Error::msg("found no version info."));
                }
            }
            Err(e) => check_error = Some(e),
        }

        Ok(CheckResult::build_with_err(
            CheckItem::CheckDatabaseVersionSupported,
            self.is_source,
            self.db_type.clone(),
            check_error,
            None,
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
            return Ok(CheckResult::build_with_err(
                CheckItem::CheckIfDatabaseSupportCdc,
                self.is_source,
                self.db_type.clone(),
                check_error,
                None,
            ));
        }

        let filter = RdbFilter::from_config(&self.filter_config, &self.db_type).unwrap();
        if BasicPrechecker::is_filter_pattern(self.db_type.clone(), &filter) {
            check_error = Some(anyhow::Error::msg(
                "oracle cdc bootstrap does not support pattern filter yet; please use explicit [filter].do_tbs",
            ));
            return Ok(CheckResult::build_with_err(
                CheckItem::CheckIfDatabaseSupportCdc,
                self.is_source,
                self.db_type.clone(),
                check_error,
                None,
            ));
        }

        if self.filter_config.do_tbs.trim().is_empty() {
            check_error = Some(anyhow::Error::msg(
                "oracle cdc requires [filter].do_tbs to be set (explicit tables only)",
            ));
            return Ok(CheckResult::build_with_err(
                CheckItem::CheckIfDatabaseSupportCdc,
                self.is_source,
                self.db_type.clone(),
                check_error,
                None,
            ));
        }

        // Keep precheck aligned with current OracleCdcExtractor limitations.
        let username = self.fetcher.current_user()?.to_uppercase();
        let mut db_tables = Vec::new();
        DbTable::from_str(&self.filter_config.do_tbs, &mut db_tables);
        let (_schemas, _tb_schemas, tbs) = DbTable::get_config_maps(&db_tables).unwrap();
        for tb_key in tbs {
            let parts: Vec<&str> = tb_key.split('.').collect();
            if parts.len() != 2 {
                bail!("invalid do_tbs entry: {}", tb_key);
            }
            let schema = parts[0].trim().to_uppercase();
            if schema != username {
                check_error = Some(anyhow::Error::msg(format!(
                    "oracle cdc bootstrap only supports tables in current user schema (expected {}, got {})",
                    username, schema
                )));
                return Ok(CheckResult::build_with_err(
                    CheckItem::CheckIfDatabaseSupportCdc,
                    self.is_source,
                    self.db_type.clone(),
                    check_error,
                    None,
                ));
            }
        }

        // Verify the minimal privileges required for trigger-based CDC setup (table/seq/trigger).
        let required = ["CREATE TABLE", "CREATE SEQUENCE", "CREATE TRIGGER"];
        let privs = self.fetch_user_sys_privs(&required).await?;
        let missing = required
            .iter()
            .filter(|p| !privs.contains(**p))
            .cloned()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            check_error = Some(anyhow::Error::msg(format!(
                "oracle cdc requires sys privileges: [{}]",
                missing.join(",")
            )));
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

        let filter = RdbFilter::from_config(&self.filter_config, &self.db_type).unwrap();
        let is_filter_pattern = BasicPrechecker::is_filter_pattern(self.db_type.clone(), &filter);
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

        let (mut models, mut err_msgs): (Vec<DbTable>, Vec<String>) = (Vec::new(), Vec::new());
        if !self.filter_config.do_tbs.is_empty() {
            DbTable::from_str(&self.filter_config.do_tbs, &mut models)
        } else if !self.filter_config.do_schemas.is_empty() {
            DbTable::from_str(&self.filter_config.do_schemas, &mut models)
        }

        let (schemas, tb_schemas, tbs) = DbTable::get_config_maps(&models).unwrap();
        let mut all_schema_names = Vec::new();
        all_schema_names.extend(schemas.into_iter());
        all_schema_names.extend(tb_schemas.into_iter());

        if !tbs.is_empty() {
            let tables = self.fetcher.fetch_tables().await?;
            let current_tbs: HashSet<String> = tables
                .iter()
                .map(|t| format!("{}.{}", t.schema_name, t.table_name))
                .collect();
            let mut not_existed_tbs: HashSet<String> = HashSet::new();
            for tb in tbs {
                if !current_tbs.contains(&tb.to_uppercase()) {
                    not_existed_tbs.insert(tb);
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

        if !all_schema_names.is_empty() {
            let schemas = self.fetcher.fetch_schemas().await?;
            let current_schemas: HashSet<String> =
                schemas.iter().map(|s| s.schema_name.clone()).collect();

            let mut not_existed_schemas: HashSet<String> = HashSet::new();
            for schema in all_schema_names {
                if !current_schemas.contains(&schema.to_uppercase()) {
                    not_existed_schemas.insert(schema);
                }
            }
            if !not_existed_schemas.is_empty() {
                err_msgs.push(format!(
                    "schemas not existed: [{}]",
                    not_existed_schemas
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
        let mut check_error = None;

        if !self.is_source && self.precheck_config.do_struct_init {
            return Ok(CheckResult::build_with_err(
                CheckItem::CheckIfTableStructSupported,
                self.is_source,
                self.db_type.clone(),
                check_error,
                None,
            ));
        }

        let filter = RdbFilter::from_config(&self.filter_config, &self.db_type).unwrap();
        let is_filter_pattern = BasicPrechecker::is_filter_pattern(self.db_type.clone(), &filter);
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

        let (mut models, mut err_msgs): (Vec<DbTable>, Vec<String>) = (Vec::new(), Vec::new());
        if !self.filter_config.do_tbs.is_empty() {
            DbTable::from_str(&self.filter_config.do_tbs, &mut models)
        } else if !self.filter_config.do_schemas.is_empty() {
            DbTable::from_str(&self.filter_config.do_schemas, &mut models)
        }

        let (schemas, tb_schemas, tbs) = DbTable::get_config_maps(&models).unwrap();
        let mut all_schema_names = Vec::new();
        all_schema_names.extend(schemas.into_iter());
        all_schema_names.extend(tb_schemas.into_iter());

        let tables = self.fetcher.fetch_tables().await?;
        let current_tables: HashSet<String> = tables
            .iter()
            .map(|t| format!("{}.{}", t.schema_name, t.table_name))
            .collect();

        let constraints = self.fetcher.fetch_constraints().await?;
        let mut has_pkuk_tables: HashSet<String> = HashSet::new();
        let mut fkref_nonexists_tables: HashSet<String> = HashSet::new();
        for c in constraints {
            let schema_table_name = format!("{}.{}", c.schema_name, c.table_name);
            if c.constraint_type == "p" || c.constraint_type == "u" {
                has_pkuk_tables.insert(schema_table_name);
                continue;
            }
            if c.constraint_type == "f"
                && !c.rel_schema_name.is_empty()
                && !c.rel_table_name.is_empty()
            {
                let ref_key = format!("{}.{}", c.rel_schema_name, c.rel_table_name);
                if !current_tables.contains(&ref_key) {
                    fkref_nonexists_tables.insert(schema_table_name);
                }
            }
        }

        let mut check_targets: HashSet<String> = HashSet::new();
        if !tbs.is_empty() {
            for tb in tbs {
                check_targets.insert(tb.to_uppercase());
            }
        } else if !all_schema_names.is_empty() {
            for schema in all_schema_names {
                let schema_upper = schema.to_uppercase();
                for tb in current_tables.iter() {
                    if tb.starts_with(&format!("{}.", schema_upper)) {
                        check_targets.insert(tb.to_string());
                    }
                }
            }
        } else {
            // No explicit targets configured; keep behavior consistent with existing precheckers.
            bail!("found no schema need to do migrate, very strange");
        }

        let mut no_pkuk_tables: HashSet<String> = HashSet::new();
        for tb in check_targets.iter() {
            if !has_pkuk_tables.contains(tb) {
                no_pkuk_tables.insert(tb.to_string());
            }
        }

        if !no_pkuk_tables.is_empty() {
            err_msgs.push(format!(
                "tables have no primary key or unique key: [{}]",
                no_pkuk_tables
                    .iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<String>>()
                    .join(";")
            ));
        }
        if !fkref_nonexists_tables.is_empty() {
            err_msgs.push(format!(
                "foreign key reference table not existed: [{}]",
                fkref_nonexists_tables
                    .iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<String>>()
                    .join(";")
            ));
        }

        if !err_msgs.is_empty() {
            check_error = Some(anyhow::Error::msg(err_msgs.join(".")))
        }

        Ok(CheckResult::build_with_err(
            CheckItem::CheckIfTableStructSupported,
            self.is_source,
            self.db_type.clone(),
            check_error,
            None,
        ))
    }
}

impl OraclePrechecker {
    async fn fetch_user_sys_privs(&self, required: &[&str]) -> anyhow::Result<HashSet<String>> {
        if required.is_empty() {
            return Ok(HashSet::new());
        }

        let in_list = required
            .iter()
            .map(|p| format!("'{}'", p.replace('\'', "''")))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT privilege FROM user_sys_privs WHERE privilege IN ({})",
            in_list
        );
        let lines = self.fetcher.client()?.query_lines(&sql).await?;
        let mut out = HashSet::new();
        for line in lines {
            let s = line.trim().to_uppercase();
            if !s.is_empty() && s != "<NULL>" {
                out.insert(s);
            }
        }
        Ok(out)
    }
}
