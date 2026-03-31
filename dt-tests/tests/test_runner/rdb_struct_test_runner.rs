use dt_common::{
    config::{config_enums::DbType, task_config::TaskConfig},
    meta::ddl_meta::{ddl_parser::DdlParser, ddl_statement::DdlStatement},
};
use dt_connector::meta_fetcher::{
    mysql::mysql_struct_check_fetcher::MysqlStructCheckFetcher,
    pg::pg_struct_check_fetcher::{PgCheckTableInfo, PgStructCheckFetcher},
};
use regex::Regex;
use std::collections::{HashMap, HashSet};

use super::rdb_test_runner::PUBLIC;
use super::{base_test_runner::BaseTestRunner, rdb_test_runner::RdbTestRunner};

pub struct RdbStructTestRunner {
    pub base: RdbTestRunner,
}

const PG_GET_INDEXDEF: &str = "pg_get_indexdef";
const CROSS_ENGINE_SUMMARY_KEYS: [&str; 7] = [
    "relchecks",
    "relkind",
    "relhasindex",
    "relhasrules",
    "relhastriggers",
    "relhasoids",
    "relpersistence",
];
const CROSS_ENGINE_INDEX_KEYS: [&str; 9] = [
    "relname",
    "indisprimary",
    "indisunique",
    "indisclustered",
    "indisvalid",
    "pg_get_constraintdef",
    "contype",
    "condeferrable",
    "condeferred",
];

impl RdbStructTestRunner {
    pub async fn new(relative_test_dir: &str) -> anyhow::Result<Self> {
        let base = RdbTestRunner::new(relative_test_dir).await?;
        Ok(Self { base })
    }

    pub async fn close(&self) -> anyhow::Result<()> {
        self.base.close().await
    }

    pub async fn run_mysql_struct_test(&mut self) -> anyhow::Result<()> {
        self.base.execute_prepare_sqls().await?;
        self.base.base.start_task().await?;

        let expect_ddl_sqls = self.load_expect_ddl_sqls().await;
        let src_check_fetcher =
            self.base
                .src_conn_pool_mysql
                .as_ref()
                .map(|conn_pool| MysqlStructCheckFetcher {
                    conn_pool: conn_pool.clone(),
                });
        let dst_check_fetcher = MysqlStructCheckFetcher {
            conn_pool: self.base.dst_conn_pool_mysql.as_mut().unwrap().clone(),
        };

        let get_sql_lines = |sql: &str| -> HashSet<String> {
            let mut line_set = HashSet::new();
            let lines: Vec<&str> = sql.split("\n").collect();
            for line in lines {
                line_set.insert(line.trim().trim_end_matches(",").to_owned());
            }
            line_set
        };

        let (src_db_tbs, dst_db_tbs) = self.base.get_compare_db_tbs().unwrap();
        for i in 0..src_db_tbs.len() {
            if let Some(src_check_fetcher) = &src_check_fetcher {
                let src_ddl_sql = src_check_fetcher
                    .fetch_table(&src_db_tbs[i].0, &src_db_tbs[i].1)
                    .await;
                println!("src_ddl_sql: {}\n", src_ddl_sql);
            }

            let dst_ddl_sql = dst_check_fetcher
                .fetch_table(&dst_db_tbs[i].0, &dst_db_tbs[i].1)
                .await;
            let key = format!("{}.{}", &dst_db_tbs[i].0, &dst_db_tbs[i].1);
            let expect_ddl_sql = expect_ddl_sqls.get(&key).unwrap().to_owned();

            println!("dst_ddl_sql: {}\n", dst_ddl_sql);
            println!("expect_ddl_sql: {}\n", expect_ddl_sql);
            let dst_ddl_sql_lines = get_sql_lines(&dst_ddl_sql);
            let expect_ddl_sql_lines = get_sql_lines(&expect_ddl_sql);

            for line in dst_ddl_sql_lines.iter() {
                println!("dst_ddl_sql_line: {}", line);
                if !expect_ddl_sql_lines.contains(line) {
                    println!("dst_ddl_sql_line NOT exists in expect_ddl_sql_lines");
                    panic!();
                }
            }
            assert_eq!(dst_ddl_sql_lines.len(), expect_ddl_sql_lines.len());
        }

        let mut tested_dbs = HashSet::new();
        for i in 0..src_db_tbs.len() {
            if tested_dbs.contains(&src_db_tbs[i].0) {
                continue;
            }

            if let Some(src_check_fetcher) = &src_check_fetcher {
                let src_ddl_sql = src_check_fetcher.fetch_database(&src_db_tbs[i].0).await;
                println!("src_ddl_sql: {}\n", src_ddl_sql);
            }

            let dst_ddl_sql = dst_check_fetcher.fetch_database(&dst_db_tbs[i].0).await;
            let key = dst_db_tbs[i].0.to_string();
            let expect_ddl_sql = expect_ddl_sqls.get(&key).unwrap().to_owned();

            println!("dst_ddl_sql: {}\n", dst_ddl_sql);
            println!("expect_ddl_sql: {}\n", expect_ddl_sql);

            assert_eq!(dst_ddl_sql, expect_ddl_sql);
            tested_dbs.insert(&src_db_tbs[i].0);
        }

        Ok(())
    }

    pub async fn run_pg_struct_test(&mut self) -> anyhow::Result<()> {
        self.base.execute_prepare_sqls().await?;
        self.base.base.start_task().await?;

        let src_db_type = self.base.config.extractor_basic.db_type.clone();
        let dst_db_type = self.base.config.sinker_basic.db_type.clone();
        let cross_engine_compare =
            Self::requires_pg_struct_normalization(&src_db_type, &dst_db_type);
        let src_check_fetcher = PgStructCheckFetcher {
            conn_pool: self.base.src_conn_pool_pg.as_mut().unwrap().clone(),
            db_type: src_db_type.clone(),
        };
        let dst_check_fetcher = PgStructCheckFetcher {
            conn_pool: self.base.dst_conn_pool_pg.as_mut().unwrap().clone(),
            db_type: dst_db_type,
        };

        let (src_db_tbs, dst_db_tbs) = self.base.get_compare_db_tbs().unwrap();
        for i in 0..src_db_tbs.len() {
            let src_db_tb = &src_db_tbs[i];
            let dst_db_tb = &dst_db_tbs[i];

            let src_table = src_check_fetcher
                .fetch_table(&src_db_tb.0, &src_db_tb.1)
                .await?;
            let dst_table = dst_check_fetcher
                .fetch_table(&dst_db_tb.0, &dst_db_tb.1)
                .await?;

            println!(
                "comparing src table: {:?} with dst table: {:?}\n",
                src_db_tb, dst_db_tb
            );

            if src_db_tb == dst_db_tb && !cross_engine_compare {
                println!("src_table: {:?}\n", src_table);
                println!("dst_table: {:?}\n", dst_table);
                assert_eq!(src_table, dst_table);
                continue;
            }

            if cross_engine_compare {
                Self::assert_cross_engine_pg_table_eq(&src_table, &dst_table, src_db_tb, dst_db_tb);
                continue;
            }

            assert_eq!(src_table.columns, dst_table.columns);
            assert_eq!(src_table.summary, dst_table.summary);
            assert_eq!(src_table.constraints, dst_table.constraints);
            Self::assert_pg_indexes_equal(
                &src_table.indexes,
                &dst_table.indexes,
                src_db_tb,
                dst_db_tb,
                false,
            );
        }

        // Smoke-check: ensure the migrated objects are queryable on the destination side.
        //
        // 1) Tables are covered by `get_compare_db_tbs()` (CREATE TABLE in src_prepare/src_test).
        // 2) Views/materialized views are not part of `\d` output, so we parse CREATE VIEW DDL
        //    from src sqls and verify they are queryable on dst.
        let mut dst_smoke_objects: HashSet<(String, String)> = HashSet::new();
        for (schema, tb) in dst_db_tbs.iter() {
            dst_smoke_objects.insert((schema.clone(), tb.clone()));
        }
        let src_views = Self::get_compare_pg_views_from_sqls(
            &src_db_type,
            &self.base.base.src_prepare_sqls,
            &self.base.base.src_test_sqls,
        );
        for (schema, view) in src_views.iter() {
            let (dst_schema, dst_view) = self.base.router.get_tb_map(schema, view);
            dst_smoke_objects.insert((dst_schema.to_string(), dst_view.to_string()));
        }

        for (schema, tb) in dst_smoke_objects.iter() {
            let schema_escaped = schema.replace('"', "\"\"");
            let tb_escaped = tb.replace('"', "\"\"");
            let sql = format!(
                "SELECT 1 FROM \"{}\".\"{}\" LIMIT 1",
                schema_escaped, tb_escaped
            );
            sqlx::query(&sql)
                .execute(self.base.dst_conn_pool_pg.as_ref().unwrap())
                .await?;
        }

        // Smoke-check routines (functions/procedures): ensure they exist and can be invoked.
        let src_routines = Self::get_compare_pg_routines_from_sqls(
            &src_db_type,
            &self.base.base.src_prepare_sqls,
            &self.base.base.src_test_sqls,
        );
        for (schema, routine, is_procedure) in src_routines.iter() {
            let (dst_schema, dst_routine) = self.base.router.get_tb_map(schema, routine);
            let schema_escaped = dst_schema.replace('"', "\"\"");
            let routine_escaped = dst_routine.replace('"', "\"\"");
            let sql = if *is_procedure {
                format!("CALL \"{}\".\"{}\"()", schema_escaped, routine_escaped)
            } else {
                format!("SELECT \"{}\".\"{}\"()", schema_escaped, routine_escaped)
            };
            sqlx::query(&sql)
                .execute(self.base.dst_conn_pool_pg.as_ref().unwrap())
                .await?;
        }

        println!(
            "summary: src tables: {:?}, dst tables: {:?}",
            src_db_tbs, dst_db_tbs
        );
        Ok(())
    }

    fn get_compare_pg_views_from_sqls(
        db_type: &DbType,
        src_prepare_sqls: &[String],
        src_test_sqls: &[String],
    ) -> Vec<(String, String)> {
        let view_re = Regex::new(r"(?is)^\s*CREATE\s+(?:OR\s+REPLACE\s+)?VIEW\s+").unwrap();
        let matview_re = Regex::new(r"(?is)^\s*CREATE\s+MATERIALIZED\s+VIEW\s+").unwrap();

        let mut results = Vec::new();
        for sql in src_prepare_sqls.iter().chain(src_test_sqls.iter()) {
            let sql_trimmed = sql.trim();
            if let Some(m) = view_re.find(sql_trimmed) {
                let name = sql_trimmed[m.end()..]
                    .split_whitespace()
                    .next()
                    .unwrap_or("");
                if name.is_empty() {
                    continue;
                }
                let (mut schema, view) = RdbTestRunner::parse_full_tb_name(name, db_type);
                if schema.is_empty() {
                    schema = PUBLIC.to_string();
                }
                results.push((schema, view));
                continue;
            }
            if let Some(m) = matview_re.find(sql_trimmed) {
                let name = sql_trimmed[m.end()..]
                    .split_whitespace()
                    .next()
                    .unwrap_or("");
                if name.is_empty() {
                    continue;
                }
                let (mut schema, view) = RdbTestRunner::parse_full_tb_name(name, db_type);
                if schema.is_empty() {
                    schema = PUBLIC.to_string();
                }
                results.push((schema, view));
                continue;
            }
        }

        results
    }

    fn get_compare_pg_routines_from_sqls(
        db_type: &DbType,
        src_prepare_sqls: &[String],
        src_test_sqls: &[String],
    ) -> Vec<(String, String, bool)> {
        let func_re =
            Regex::new(r"(?is)^\s*CREATE\s+(?:OR\s+REPLACE\s+)?FUNCTION\s+([^\s(]+)\s*\(").unwrap();
        let proc_re =
            Regex::new(r"(?is)^\s*CREATE\s+(?:OR\s+REPLACE\s+)?PROCEDURE\s+([^\s(]+)\s*\(")
                .unwrap();

        let mut results = Vec::new();
        for sql in src_prepare_sqls.iter().chain(src_test_sqls.iter()) {
            let sql_trimmed = sql.trim();
            if let Some(cap) = func_re.captures(sql_trimmed) {
                let name = cap.get(1).map(|m| m.as_str()).unwrap_or("");
                if name.is_empty() {
                    continue;
                }
                let (mut schema, routine) = RdbTestRunner::parse_full_tb_name(name, db_type);
                if schema.is_empty() {
                    schema = PUBLIC.to_string();
                }
                results.push((schema, routine, false));
                continue;
            }
            if let Some(cap) = proc_re.captures(sql_trimmed) {
                let name = cap.get(1).map(|m| m.as_str()).unwrap_or("");
                if name.is_empty() {
                    continue;
                }
                let (mut schema, routine) = RdbTestRunner::parse_full_tb_name(name, db_type);
                if schema.is_empty() {
                    schema = PUBLIC.to_string();
                }
                results.push((schema, routine, true));
                continue;
            }
        }

        results
    }

    fn requires_pg_struct_normalization(src_db_type: &DbType, dst_db_type: &DbType) -> bool {
        matches!(src_db_type, DbType::GaussDBPg) || matches!(dst_db_type, DbType::GaussDBPg)
    }

    fn assert_cross_engine_pg_table_eq(
        src_table: &PgCheckTableInfo,
        dst_table: &PgCheckTableInfo,
        src_db_tb: &(String, String),
        dst_db_tb: &(String, String),
    ) {
        assert_eq!(src_table.columns, dst_table.columns);
        assert_eq!(
            Self::normalize_pg_summary_for_cross_engine(&src_table.summary),
            Self::normalize_pg_summary_for_cross_engine(&dst_table.summary)
        );
        assert_eq!(src_table.constraints, dst_table.constraints);
        Self::assert_pg_indexes_equal(
            &src_table.indexes,
            &dst_table.indexes,
            src_db_tb,
            dst_db_tb,
            true,
        );
    }

    fn normalize_pg_summary_for_cross_engine(
        summary: &[HashMap<String, String>],
    ) -> Vec<HashMap<String, String>> {
        summary
            .iter()
            .map(|row| {
                let mut normalized = HashMap::new();
                for key in CROSS_ENGINE_SUMMARY_KEYS {
                    if let Some(value) = row.get(key) {
                        normalized.insert(key.to_string(), value.to_string());
                    }
                }
                normalized
            })
            .collect()
    }

    fn normalize_pg_indexdef(indexdef: &str) -> String {
        indexdef.replace("USING ubtree", "USING btree")
    }

    fn assert_pg_indexes_equal(
        src_indexes: &[HashMap<String, String>],
        dst_indexes: &[HashMap<String, String>],
        src_db_tb: &(String, String),
        dst_db_tb: &(String, String),
        normalize_access_method: bool,
    ) {
        assert_eq!(src_indexes.len(), dst_indexes.len());
        let parser = DdlParser::new(DbType::Pg);
        for (j, src_index) in src_indexes.iter().enumerate() {
            let src_indexdef = match src_index.get(PG_GET_INDEXDEF) {
                Some(v) => v,
                None => continue,
            };
            let dst_index = &dst_indexes[j];

            let src_indexdef = if normalize_access_method {
                Self::normalize_pg_indexdef(src_indexdef)
            } else {
                src_indexdef.to_string()
            };
            let dst_indexdef = if normalize_access_method {
                Self::normalize_pg_indexdef(dst_index.get(PG_GET_INDEXDEF).unwrap())
            } else {
                dst_index.get(PG_GET_INDEXDEF).unwrap().to_string()
            };
            let src_ddl_data = parser.parse(&src_indexdef).unwrap().unwrap();
            let dst_ddl_data = parser.parse(&dst_indexdef).unwrap().unwrap();

            if let DdlStatement::PgCreateIndex(src) = src_ddl_data.statement {
                if !normalize_access_method || !src.schema.is_empty() {
                    assert_eq!(src.schema, src_db_tb.0);
                }
                assert_eq!(src.tb, src_db_tb.1);

                if let DdlStatement::PgCreateIndex(dst) = dst_ddl_data.statement {
                    if !normalize_access_method || !dst.schema.is_empty() {
                        assert_eq!(dst.schema, dst_db_tb.0);
                    }
                    assert_eq!(dst.tb, dst_db_tb.1);

                    assert_eq!(src.index_name, dst.index_name);
                    assert_eq!(src.is_unique, dst.is_unique);
                    assert_eq!(src.is_concurrently, dst.is_concurrently);
                    assert_eq!(src.if_not_exists, dst.if_not_exists);
                    assert_eq!(src.is_only, dst.is_only);
                    if !normalize_access_method {
                        assert_eq!(src.unparsed, dst.unparsed);
                    }
                }
            }

            if normalize_access_method {
                for key in CROSS_ENGINE_INDEX_KEYS {
                    println!("index property: {}", key);
                    assert_eq!(src_index.get(key), dst_index.get(key));
                }
                continue;
            }

            assert_eq!(src_index.len(), dst_index.len());
            for key in src_index.keys() {
                if key == PG_GET_INDEXDEF {
                    continue;
                }
                println!("index property: {}", key);
                assert_eq!(src_index.get(key), dst_index.get(key));
            }
        }
    }

    pub async fn run_struct_test_without_check(&mut self) -> anyhow::Result<()> {
        self.base.execute_prepare_sqls().await?;
        self.base.base.start_task().await
    }

    pub async fn load_expect_ddl_sqls(&self) -> HashMap<String, String> {
        let config = TaskConfig::new(&self.base.base.task_config_file).unwrap();
        let ddl_file = match config.sinker_basic.db_type {
            DbType::Mysql => {
                let version = self.base.get_dst_mysql_version().await;
                if version.starts_with("5.") {
                    format!("{}/expect_ddl_5.7.sql", self.base.base.test_dir)
                } else {
                    format!("{}/expect_ddl_8.0.sql", self.base.base.test_dir)
                }
            }
            _ => format!("{}/expect_ddl.sql", self.base.base.test_dir),
        };

        let mut ddl_sqls = HashMap::new();
        let lines = BaseTestRunner::load_file(&ddl_file);
        let mut lines = lines.iter().peekable();
        while let Some(line) = lines.next() {
            if line.trim().is_empty() {
                continue;
            }

            let key = line.trim().to_owned();
            let mut sql = String::new();
            for line in lines.by_ref() {
                if line.trim().is_empty() {
                    break;
                }
                sql.push_str(line);
                sql.push('\n');
            }
            ddl_sqls.insert(key, sql.trim().to_owned());
        }
        ddl_sqls
    }
}

#[cfg(test)]
mod tests {
    use super::RdbStructTestRunner;
    use std::collections::HashMap;

    fn row(entries: &[(&str, &str)]) -> HashMap<String, String> {
        entries
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    fn normalize_pg_indexdef_maps_ubtree_to_btree() {
        let sql = "CREATE UNIQUE INDEX foo ON public.bar USING ubtree (id)";
        assert_eq!(
            RdbStructTestRunner::normalize_pg_indexdef(sql),
            "CREATE UNIQUE INDEX foo ON public.bar USING btree (id)"
        );
    }

    #[test]
    fn normalize_pg_summary_for_cross_engine_keeps_logical_keys_only() {
        let summary = vec![row(&[
            ("relkind", "r"),
            ("relhasindex", "t"),
            ("relreplident", "d"),
            ("amname", "ubtree"),
        ])];
        assert_eq!(
            RdbStructTestRunner::normalize_pg_summary_for_cross_engine(&summary),
            vec![row(&[("relkind", "r"), ("relhasindex", "t")])]
        );
    }
}
