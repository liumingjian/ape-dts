use anyhow::{bail, Context};
use sqlx::Row;

use dt_common::rdb_filter::RdbFilter;

use super::rdb_test_runner::RdbTestRunner;

pub struct OracleStructTestRunner {
    pub base: RdbTestRunner,
}

impl OracleStructTestRunner {
    pub async fn new(relative_test_dir: &str) -> anyhow::Result<Self> {
        Ok(Self {
            base: RdbTestRunner::new(relative_test_dir).await?,
        })
    }

    pub async fn close(&self) -> anyhow::Result<()> {
        self.base.close().await
    }

    pub async fn run_struct_test(&mut self) -> anyhow::Result<()> {
        self.base.execute_prepare_sqls().await?;
        self.base.base.start_task().await?;
        self.assert_dst_tables_ready().await
    }

    async fn assert_dst_tables_ready(&self) -> anyhow::Result<()> {
        let src_oracle = self
            .base
            .src_oracle_client
            .as_ref()
            .context("oracle struct test requires src_oracle_client")?;
        let dst_pool = self
            .base
            .dst_conn_pool_pg
            .as_ref()
            .context("oracle struct test requires dst_conn_pool_pg")?;

        let src_tbs = collect_explicit_do_tbs(&self.base.filter)?;
        for (src_schema, src_tb) in src_tbs.into_iter() {
            let (dst_schema, dst_tb) = self.base.router.get_tb_map(&src_schema, &src_tb);

            // Smoke-check: ensure the migrated table is queryable on dst.
            let schema_escaped = dst_schema.replace('"', "\"\"");
            let tb_escaped = dst_tb.replace('"', "\"\"");
            let smoke_sql = format!(
                "SELECT 1 FROM \"{}\".\"{}\" LIMIT 1",
                schema_escaped, tb_escaped
            );
            sqlx::query(&smoke_sql).execute(dst_pool).await?;

            // Column smoke-check: names + nullability (types may be normalized by dst engine).
            let oracle_cols = fetch_oracle_columns(src_oracle, &src_schema, &src_tb).await?;
            let col_map = self
                .base
                .router
                .get_col_map(&src_schema, &src_tb)
                .cloned()
                .unwrap_or_default();
            let expected_cols: Vec<(String, bool)> = oracle_cols
                .into_iter()
                .map(|(name, nullable)| (col_map.get(&name).cloned().unwrap_or(name), nullable))
                .collect();

            let mut dst_cols: Vec<(String, bool)> = Vec::new();
            let rows = sqlx::query(
                "SELECT column_name, is_nullable \
                 FROM information_schema.columns \
                 WHERE table_schema=$1 AND table_name=$2 \
                 ORDER BY ordinal_position ASC",
            )
            .bind(dst_schema)
            .bind(dst_tb)
            .fetch_all(dst_pool)
            .await?;
            for row in rows {
                let name: String = row.try_get(0)?;
                let nullable: String = row.try_get(1)?;
                dst_cols.push((name, nullable.eq_ignore_ascii_case("YES")));
            }

            if dst_cols.is_empty() {
                bail!(
                    "destination columns not found after struct sync: {}.{}",
                    dst_schema,
                    dst_tb
                );
            }

            let dst_names: Vec<String> = dst_cols.iter().map(|(n, _)| n.clone()).collect();
            let expected_names: Vec<String> =
                expected_cols.iter().map(|(n, _)| n.clone()).collect();
            if dst_names != expected_names {
                bail!(
                    "destination column names mismatch: {}.{} expected={:?} got={:?}",
                    dst_schema,
                    dst_tb,
                    expected_names,
                    dst_names
                );
            }

            let dst_nullable: Vec<bool> = dst_cols.iter().map(|(_, n)| *n).collect();
            let expected_nullable: Vec<bool> = expected_cols.iter().map(|(_, n)| *n).collect();
            if dst_nullable != expected_nullable {
                bail!(
                    "destination column nullability mismatch: {}.{} expected={:?} got={:?}",
                    dst_schema,
                    dst_tb,
                    expected_nullable,
                    dst_nullable
                );
            }
        }

        Ok(())
    }
}

fn collect_explicit_do_tbs(filter: &RdbFilter) -> anyhow::Result<Vec<(String, String)>> {
    if filter.do_tbs.is_empty() {
        bail!("oracle struct test requires explicit filter.do_tbs");
    }
    let mut out = Vec::new();
    for (schema, tb) in filter.do_tbs.iter() {
        if RdbFilter::is_pattern(schema, &filter.db_type)
            || RdbFilter::is_pattern(tb, &filter.db_type)
        {
            bail!(
                "oracle struct test does not support wildcard do_tbs: {}.{}",
                schema,
                tb
            );
        }
        out.push((schema.clone(), tb.clone()));
    }
    out.sort();
    Ok(out)
}

async fn fetch_oracle_columns(
    client: &dt_connector::oracle::OracleSqlPlusClient,
    schema: &str,
    tb: &str,
) -> anyhow::Result<Vec<(String, bool)>> {
    let owner = escape_sql_literal(&schema.to_uppercase());
    let table = escape_sql_literal(&tb.to_uppercase());
    let sql = format!(
        "SELECT column_name, nullable, column_id \
         FROM all_tab_columns \
         WHERE owner='{}' AND table_name='{}' \
         ORDER BY column_id ASC",
        owner, table
    );

    let mut out = Vec::new();
    for line in client.query_lines(&sql).await? {
        let parts: Vec<&str> = line.split('|').map(|s| s.trim()).collect();
        if parts.len() < 2 {
            continue;
        }
        let name = parts[0].to_string();
        let nullable = parts[1].eq_ignore_ascii_case("Y");
        out.push((name, nullable));
    }
    if out.is_empty() {
        bail!("oracle columns not found for {}.{}", schema, tb);
    }
    Ok(out)
}

fn escape_sql_literal(s: &str) -> String {
    s.replace('\'', "''")
}
