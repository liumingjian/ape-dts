use std::{env, fs, path::Path};

use anyhow::{bail, Context};
use dt_common::config::connection_auth_config::ConnectionAuthConfig;
use dt_task::task_util::TaskUtil;
use sqlx::{Column, Row};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    load_env_override("dt-tests/tests/.env")?;
    load_env_override("dt-tests/tests/.env.local")?;

    let prefix =
        env::var("GAUSSDB_ADMIN_PREFIX").unwrap_or_else(|_| "gaussdb_pg_extractor".to_string());
    let url = env::var("GAUSSDB_ADMIN_URL")
        .or_else(|_| env::var(format!("{prefix}_without_auth_url")))?;
    let auth = ConnectionAuthConfig::Basic {
        username: env::var(format!("{prefix}_username"))?,
        password: Some(env::var(format!("{prefix}_password"))?),
    };
    let sql = admin_sql()?;
    let statements = split_sql(&sql);
    if statements.is_empty() {
        bail!("GAUSSDB_ADMIN_SQL did not contain executable SQL");
    }
    let verbose = env::var("GAUSSDB_ADMIN_VERBOSE").as_deref() == Ok("1");

    let pool = TaskUtil::create_pg_conn_pool(&url, &auth, 1, false, false).await?;
    let mut executed_count = 0usize;
    for statement in statements {
        if statement
            .trim_start()
            .to_ascii_lowercase()
            .starts_with("select")
        {
            print_query(&pool, &statement).await?;
        } else {
            sqlx::query(&statement).execute(&pool).await?;
            executed_count += 1;
            if verbose {
                println!("OK {statement}");
            }
        }
    }
    if executed_count > 0 {
        println!("OK executed_statements={executed_count}");
    }
    pool.close().await;
    Ok(())
}

fn admin_sql() -> anyhow::Result<String> {
    if let Ok(path) = env::var("GAUSSDB_ADMIN_SQL_FILE") {
        return fs::read_to_string(path).context("failed to read GAUSSDB_ADMIN_SQL_FILE");
    }
    env::var("GAUSSDB_ADMIN_SQL").context("missing GAUSSDB_ADMIN_SQL")
}

async fn print_query(pool: &sqlx::Pool<sqlx::Postgres>, sql: &str) -> anyhow::Result<()> {
    let rows = sqlx::query(sql).fetch_all(pool).await?;
    for row in rows {
        let values = row
            .columns()
            .iter()
            .enumerate()
            .map(|(index, column)| value_text(&row, index, column.name()))
            .collect::<Vec<_>>();
        println!("{}", values.join("\t"));
    }
    Ok(())
}

fn value_text(row: &sqlx::postgres::PgRow, index: usize, name: &str) -> String {
    if let Ok(value) = row.try_get::<String, _>(index) {
        return format!("{name}={value}");
    }
    if let Ok(value) = row.try_get::<bool, _>(index) {
        return format!("{name}={value}");
    }
    if let Ok(value) = row.try_get::<i32, _>(index) {
        return format!("{name}={value}");
    }
    format!("{name}=<unsupported>")
}

fn split_sql(sql: &str) -> Vec<String> {
    sql.split(';')
        .map(str::trim)
        .filter(|statement| !statement.is_empty())
        .map(ToOwned::to_owned)
        .collect()
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
