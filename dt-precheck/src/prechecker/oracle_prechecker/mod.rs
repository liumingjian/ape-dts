mod cdc;
mod privileges;
mod struct_check;

use async_trait::async_trait;
use dt_common::config::{config_enums::DbType, filter_config::FilterConfig};

use crate::{
    config::precheck_config::PrecheckConfig,
    fetcher::{oracle::oracle_fetcher::OracleFetcher, traits::Fetcher},
    meta::{check_item::CheckItem, check_result::CheckResult},
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
        self.check_cdc_supported_internal().await
    }

    async fn check_struct_existed_or_not(&mut self) -> anyhow::Result<CheckResult> {
        self.check_struct_existed_or_not_internal().await
    }

    async fn check_table_structs(&mut self) -> anyhow::Result<CheckResult> {
        self.check_table_structs_internal().await
    }
}

