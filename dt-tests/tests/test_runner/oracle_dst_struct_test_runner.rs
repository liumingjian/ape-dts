use anyhow::{bail, Context};
use sqlx::Row;

use dt_common::rdb_filter::RdbFilter;

use super::rdb_test_runner::RdbTestRunner;

pub struct OracleDstStructTestRunner {
    pub base: RdbTestRunner,
}

impl OracleDstStructTestRunner {
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
        let src_pool = self
            .base
            .src_conn_pool_pg
            .as_ref()
            .context("oracle-dst struct test requires src_conn_pool_pg")?;
        let dst_oracle = self
            .base
            .dst_oracle_client
            .as_ref()
            .context("oracle-dst struct test requires dst_oracle_client")?;

        let src_tbs = collect_explicit_do_tbs(&self.base.filter)?;
        for (src_schema, src_tb) in src_tbs.into_iter() {
            let (dst_schema, dst_tb) = self.base.router.get_tb_map(&src_schema, &src_tb);

            // Smoke-check: ensure the migrated table is queryable on dst.
            let owner = escape_sql_literal(&dst_schema.to_uppercase());
            let table = escape_sql_literal(&dst_tb.to_uppercase());
            let smoke_sql = format!("SELECT 1 FROM {}.{} WHERE ROWNUM <= 1", owner, table);
            let _ = dst_oracle.query_lines(&smoke_sql).await?;

            // Expected columns from source: names + nullability.
            let src_cols = fetch_pg_columns(src_pool, &src_schema, &src_tb).await?;
            let col_map = self
                .base
                .router
                .get_col_map(&src_schema, &src_tb)
                .cloned()
                .unwrap_or_default();
            let expected_cols: Vec<(String, bool)> = src_cols
                .into_iter()
                .map(|(name, nullable)| (col_map.get(&name).cloned().unwrap_or(name), nullable))
                .collect();

            // Actual columns on Oracle dst.
            let dst_cols = fetch_oracle_columns(dst_oracle, &dst_schema, &dst_tb).await?;

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

            // PK smoke-check (basic expectation: at least one PK constraint exists).
            let pk_cnt = fetch_oracle_pk_count(dst_oracle, &dst_schema, &dst_tb).await?;
            if pk_cnt == 0 {
                bail!(
                    "destination primary key not found: {}.{}",
                    dst_schema,
                    dst_tb
                );
            }
        }

        Ok(())
    }
}

fn collect_explicit_do_tbs(filter: &RdbFilter) -> anyhow::Result<Vec<(String, String)>> {
    if filter.do_tbs.is_empty() {
        bail!("oracle-dst struct test requires explicit filter.do_tbs");
    }
    let mut out = Vec::new();
    for (schema, tb) in filter.do_tbs.iter() {
        if RdbFilter::is_pattern(schema, &filter.db_type)
            || RdbFilter::is_pattern(tb, &filter.db_type)
        {
            bail!(
                "oracle-dst struct test does not support wildcard do_tbs: {}.{}",
                schema,
                tb
            );
        }
        out.push((schema.clone(), tb.clone()));
    }
    out.sort();
    Ok(out)
}

async fn fetch_pg_columns(
    pool: &sqlx::Pool<sqlx::Postgres>,
    schema: &str,
    tb: &str,
) -> anyhow::Result<Vec<(String, bool)>> {
    let mut out = Vec::new();
    let rows = sqlx::query(
        "SELECT column_name, is_nullable \
         FROM information_schema.columns \
         WHERE table_schema=$1 AND table_name=$2 \
         ORDER BY ordinal_position ASC",
    )
    .bind(schema)
    .bind(tb)
    .fetch_all(pool)
    .await?;
    for row in rows {
        let name: String = row.try_get(0)?;
        let nullable: String = row.try_get(1)?;
        out.push((name, nullable.eq_ignore_ascii_case("YES")));
    }
    if out.is_empty() {
        bail!("source columns not found for {}.{}", schema, tb);
    }
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

async fn fetch_oracle_pk_count(
    client: &dt_connector::oracle::OracleSqlPlusClient,
    schema: &str,
    tb: &str,
) -> anyhow::Result<u64> {
    let owner = escape_sql_literal(&schema.to_uppercase());
    let table = escape_sql_literal(&tb.to_uppercase());
    let sql = format!(
        "SELECT COUNT(*) FROM all_constraints \
         WHERE owner='{}' AND table_name='{}' AND constraint_type='P'",
        owner, table
    );
    let lines = client.query_lines(&sql).await?;
    let first = lines
        .into_iter()
        .next()
        .context("oracle pk count query returned no rows")?;
    Ok(first.trim().parse::<u64>()?)
}

fn escape_sql_literal(s: &str) -> String {
    s.replace('\'', "''")
}
