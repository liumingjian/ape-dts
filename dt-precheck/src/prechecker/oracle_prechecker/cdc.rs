use anyhow::bail;
use dt_common::rdb_filter::RdbFilter;

use crate::{
    meta::{check_item::CheckItem, check_result::CheckResult, db_table_model::DbTable},
    prechecker::basic::BasicPrechecker,
};

use super::OraclePrechecker;

impl OraclePrechecker {
    pub(super) async fn check_cdc_supported_internal(&mut self) -> anyhow::Result<CheckResult> {
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

        self.validate_do_tbs_in_current_user_schema()?;
        self.validate_trigger_cdc_privileges(&mut check_error).await?;

        Ok(CheckResult::build_with_err(
            CheckItem::CheckIfDatabaseSupportCdc,
            self.is_source,
            self.db_type.clone(),
            check_error,
            None,
        ))
    }

    fn validate_do_tbs_in_current_user_schema(&self) -> anyhow::Result<()> {
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
                bail!(
                    "oracle cdc bootstrap only supports tables in current user schema (expected {}, got {})",
                    username,
                    schema
                );
            }
        }
        Ok(())
    }

    async fn validate_trigger_cdc_privileges(
        &self,
        check_error: &mut Option<anyhow::Error>,
    ) -> anyhow::Result<()> {
        let required = ["CREATE TABLE", "CREATE SEQUENCE", "CREATE TRIGGER"];
        let privs = self.fetch_user_sys_privs(&required).await?;
        let missing = required
            .iter()
            .filter(|p| !privs.contains(**p))
            .cloned()
            .collect::<Vec<_>>();
        if missing.is_empty() {
            return Ok(());
        }
        *check_error = Some(anyhow::Error::msg(format!(
            "oracle cdc requires sys privileges: [{}]",
            missing.join(",")
        )));
        Ok(())
    }
}

