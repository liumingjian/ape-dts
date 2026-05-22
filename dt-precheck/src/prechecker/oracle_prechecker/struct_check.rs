use std::collections::HashSet;

use dt_common::rdb_filter::RdbFilter;

use crate::{
    fetcher::traits::Fetcher,
    meta::{check_item::CheckItem, check_result::CheckResult, db_table_model::DbTable},
    prechecker::basic::BasicPrechecker,
};

use super::OraclePrechecker;

impl OraclePrechecker {
    pub(super) async fn check_struct_existed_or_not_internal(
        &mut self,
    ) -> anyhow::Result<CheckResult> {
        let item = CheckItem::CheckIfStructExisted;
        if Self::should_skip_struct_checks(self.is_source, self.precheck_config.do_struct_init) {
            return Ok(self.build_result(item, None, None));
        }

        if self.is_filter_pattern() {
            return Ok(self.build_result(
                item,
                None,
                Some(anyhow::Error::msg(
                    "CheckIfStructExisted with filter in pattern is not supported.",
                )),
            ));
        }

        let (tbs, schema_names) = Self::parse_targets(&self.filter_config);
        let err_msgs = self
            .collect_struct_existed_err_msgs(&tbs, &schema_names)
            .await?;

        Ok(self.build_result(item, Self::error_from_msgs(err_msgs), None))
    }

    pub(super) async fn check_table_structs_internal(&mut self) -> anyhow::Result<CheckResult> {
        let item = CheckItem::CheckIfTableStructSupported;
        if Self::should_skip_struct_checks(self.is_source, self.precheck_config.do_struct_init) {
            return Ok(self.build_result(item, None, None));
        }

        if self.is_filter_pattern() {
            return Ok(self.build_result(
                item,
                None,
                Some(anyhow::Error::msg(
                    "CheckIfTableStructSupported with filter in pattern is not supported.",
                )),
            ));
        }

        let (tbs, schema_names) = Self::parse_targets(&self.filter_config);
        let err_msgs = self
            .collect_table_struct_err_msgs(&tbs, &schema_names)
            .await?;

        Ok(self.build_result(item, Self::error_from_msgs(err_msgs), None))
    }

    fn should_skip_struct_checks(is_source: bool, do_struct_init: bool) -> bool {
        !is_source && do_struct_init
    }

    fn is_filter_pattern(&self) -> bool {
        let filter = RdbFilter::from_config(&self.filter_config, &self.db_type).unwrap();
        BasicPrechecker::is_filter_pattern(self.db_type.clone(), &filter)
    }

    fn build_result(
        &self,
        check_item: CheckItem,
        err_option: Option<anyhow::Error>,
        warn_option: Option<anyhow::Error>,
    ) -> CheckResult {
        CheckResult::build_with_err(
            check_item,
            self.is_source,
            self.db_type.clone(),
            err_option,
            warn_option,
        )
    }

    fn error_from_msgs(err_msgs: Vec<String>) -> Option<anyhow::Error> {
        if err_msgs.is_empty() {
            return None;
        }
        Some(anyhow::Error::msg(err_msgs.join(".")))
    }

    async fn collect_struct_existed_err_msgs(
        &mut self,
        tbs: &[String],
        schema_names: &[String],
    ) -> anyhow::Result<Vec<String>> {
        let mut err_msgs = Vec::<String>::new();

        if let Some(msg) = self.not_existed_tables_msg(tbs).await? {
            err_msgs.push(msg);
        }
        if let Some(msg) = self.not_existed_schemas_msg(schema_names).await? {
            err_msgs.push(msg);
        }

        Ok(err_msgs)
    }

    async fn not_existed_tables_msg(&mut self, tbs: &[String]) -> anyhow::Result<Option<String>> {
        if tbs.is_empty() {
            return Ok(None);
        }

        let tables = self.fetcher.fetch_tables().await?;
        let current_tbs: HashSet<String> = tables
            .iter()
            .map(|t| format!("{}.{}", t.schema_name, t.table_name))
            .collect();

        let not_existed_tbs: HashSet<String> = tbs
            .iter()
            .filter(|tb| !current_tbs.contains(&tb.to_uppercase()))
            .cloned()
            .collect();
        if not_existed_tbs.is_empty() {
            return Ok(None);
        }

        Ok(Some(format!(
            "tables not existed: [{}]",
            Self::join_set(&not_existed_tbs)
        )))
    }

    async fn not_existed_schemas_msg(
        &mut self,
        schema_names: &[String],
    ) -> anyhow::Result<Option<String>> {
        if schema_names.is_empty() {
            return Ok(None);
        }

        let schemas = self.fetcher.fetch_schemas().await?;
        let current_schemas: HashSet<String> =
            schemas.iter().map(|s| s.schema_name.clone()).collect();

        let not_existed_schemas: HashSet<String> = schema_names
            .iter()
            .filter(|schema| !current_schemas.contains(&schema.to_uppercase()))
            .cloned()
            .collect();
        if not_existed_schemas.is_empty() {
            return Ok(None);
        }

        Ok(Some(format!(
            "schemas not existed: [{}]",
            Self::join_set(&not_existed_schemas)
        )))
    }

    async fn collect_table_struct_err_msgs(
        &mut self,
        tbs: &[String],
        schema_names: &[String],
    ) -> anyhow::Result<Vec<String>> {
        let tables = self.fetcher.fetch_tables().await?;
        let current_tables: HashSet<String> = tables
            .iter()
            .map(|t| format!("{}.{}", t.schema_name, t.table_name))
            .collect();

        let (has_pkuk_tables, fkref_nonexists_tables) =
            Self::classify_constraints(self.fetcher.fetch_constraints().await?, &current_tables);

        let check_targets = Self::resolve_check_targets(tbs, schema_names, &current_tables)?;
        let no_pkuk_tables = Self::find_no_pkuk_tables(&check_targets, &has_pkuk_tables);

        let mut err_msgs = Vec::<String>::new();
        if !no_pkuk_tables.is_empty() {
            err_msgs.push(format!(
                "tables have no primary key or unique key: [{}]",
                Self::join_set(&no_pkuk_tables)
            ));
        }
        if !fkref_nonexists_tables.is_empty() {
            err_msgs.push(format!(
                "foreign key reference table not existed: [{}]",
                Self::join_set(&fkref_nonexists_tables)
            ));
        }
        Ok(err_msgs)
    }

    fn join_set(set: &HashSet<String>) -> String {
        set.iter()
            .map(|e| e.to_string())
            .collect::<Vec<String>>()
            .join(";")
    }

    fn parse_targets(
        filter_config: &dt_common::config::filter_config::FilterConfig,
    ) -> (Vec<String>, Vec<String>) {
        let (mut models, mut schema_names): (Vec<DbTable>, Vec<String>) = (Vec::new(), Vec::new());
        if !filter_config.do_tbs.is_empty() {
            DbTable::from_str(&filter_config.do_tbs, &mut models)
        } else if !filter_config.do_schemas.is_empty() {
            DbTable::from_str(&filter_config.do_schemas, &mut models)
        }

        let (schemas, tb_schemas, tbs) = DbTable::get_config_maps(&models).unwrap();
        schema_names.extend(schemas);
        schema_names.extend(tb_schemas);
        (tbs, schema_names)
    }

    fn resolve_check_targets(
        tbs: &[String],
        schema_names: &[String],
        current_tables: &HashSet<String>,
    ) -> anyhow::Result<HashSet<String>> {
        if !tbs.is_empty() {
            return Ok(tbs.iter().map(|tb| tb.to_uppercase()).collect());
        }
        if !schema_names.is_empty() {
            let mut out = HashSet::new();
            for schema in schema_names {
                let schema_upper = schema.to_uppercase();
                for tb in current_tables {
                    if tb.starts_with(&format!("{}.", schema_upper)) {
                        out.insert(tb.to_string());
                    }
                }
            }
            return Ok(out);
        }
        anyhow::bail!("found no schema need to do migrate, very strange");
    }

    fn classify_constraints(
        constraints: Vec<crate::meta::database_mode::Constraint>,
        current_tables: &HashSet<String>,
    ) -> (HashSet<String>, HashSet<String>) {
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
        (has_pkuk_tables, fkref_nonexists_tables)
    }

    fn find_no_pkuk_tables(
        check_targets: &HashSet<String>,
        has_pkuk_tables: &HashSet<String>,
    ) -> HashSet<String> {
        let mut out = HashSet::new();
        for tb in check_targets {
            if !has_pkuk_tables.contains(tb) {
                out.insert(tb.to_string());
            }
        }
        out
    }
}
