use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, OnceLock},
    time::Duration,
};

use anyhow::Context;
use dt_common::config::config_enums::DbType;
use dt_common::meta::{
    mysql::{mysql_meta_manager::MysqlMetaManager, mysql_tb_meta::MysqlTbMeta},
    pg::{pg_meta_manager::PgMetaManager, pg_tb_meta::PgTbMeta},
    row_data::RowData,
};
use dt_connector::rdb_query_builder::RdbQueryBuilder;
use futures::TryStreamExt;
use sqlx::{MySql, Pool, Postgres};
use tokio::sync::Mutex;
use tokio::time::{sleep, timeout};

pub struct RdbUtil {}

type PgMetaManagerHandle = Arc<Mutex<PgMetaManager>>;
static PG_META_MANAGERS: OnceLock<Mutex<HashMap<usize, PgMetaManagerHandle>>> = OnceLock::new();

impl RdbUtil {
    fn is_transient_error(err: &anyhow::Error) -> bool {
        let msg = format!("{:#}", err).to_lowercase();
        msg.contains("unexpected end of file")
            || msg.contains("read-only transaction")
            || msg.contains("connection reset")
            || msg.contains("broken pipe")
            || msg.contains("connection refused")
            || msg.contains("operation timed out")
            || msg.contains("timeout expired")
            || msg.contains("pool timed out")
            || msg.contains("terminating connection")
            || msg.contains("server closed the connection")
    }

    async fn get_or_create_pg_meta_manager(
        conn_pool: &Pool<Postgres>,
    ) -> anyhow::Result<PgMetaManagerHandle> {
        let key = conn_pool as *const Pool<Postgres> as usize;
        let cache = PG_META_MANAGERS.get_or_init(|| Mutex::new(HashMap::new()));
        {
            let map = cache.lock().await;
            if let Some(handle) = map.get(&key) {
                return Ok(handle.clone());
            }
        }

        let manager = PgMetaManager::new(conn_pool.clone()).await?;
        let handle = Arc::new(Mutex::new(manager));

        let mut map = cache.lock().await;
        Ok(map.entry(key).or_insert_with(|| handle.clone()).clone())
    }

    pub async fn fetch_data_mysql(
        conn_pool: &Pool<MySql>,
        ignore_cols: Option<&HashSet<String>>,
        db_tb: &(String, String),
        condition: &str,
    ) -> anyhow::Result<Vec<RowData>> {
        Self::fetch_data_mysql_compatible(conn_pool, ignore_cols, db_tb, &DbType::Mysql, condition)
            .await
    }

    pub async fn fetch_data_mysql_compatible(
        conn_pool: &Pool<MySql>,
        ignore_cols: Option<&HashSet<String>>,
        db_tb: &(String, String),
        db_type: &DbType,
        where_sql: &str,
    ) -> anyhow::Result<Vec<RowData>> {
        let tb_meta = Self::get_tb_meta_mysql_compatible(conn_pool, db_tb, db_type).await?;
        let query_builder = RdbQueryBuilder::new_for_mysql(&tb_meta, ignore_cols);
        let cols_str = query_builder.build_extract_cols_str().unwrap();
        let sql = format!(
            "SELECT {} FROM `{}`.`{}` {} ORDER BY `{}` ASC",
            cols_str, &db_tb.0, &db_tb.1, where_sql, &tb_meta.basic.cols[0],
        );

        let mut query = sqlx::query(&sql);
        if matches!(db_type, DbType::StarRocks | DbType::Foxlake | DbType::Doris) {
            query = query.disable_arguments();
        }

        let mut rows = query.fetch(conn_pool);
        let mut result = Vec::new();
        while let Some(row) = rows.try_next().await.unwrap() {
            let row_data = RowData::from_mysql_compatible_row(&row, &tb_meta, &None, db_type);
            result.push(row_data);
        }

        Ok(result)
    }

    pub async fn fetch_data_pg(
        conn_pool: &Pool<Postgres>,
        ignore_cols: Option<&HashSet<String>>,
        db_tb: &(String, String),
        where_sql: &str,
    ) -> anyhow::Result<Vec<RowData>> {
        let mut last_err: Option<anyhow::Error> = None;
        for attempt in 1..=3 {
            let res = timeout(Duration::from_secs(15), async {
                let tb_meta = Self::get_tb_meta_pg(conn_pool, db_tb).await?;
                let query_builder = RdbQueryBuilder::new_for_pg(&tb_meta, ignore_cols);
                let cols_str = query_builder.build_extract_cols_str().with_context(|| {
                    format!("build_extract_cols_str failed for tb: {:?}", db_tb)
                })?;
                let sql = format!(
                    r#"SELECT {} FROM "{}"."{}" {} ORDER BY "{}" ASC"#,
                    cols_str, &db_tb.0, &db_tb.1, where_sql, &tb_meta.basic.cols[0],
                );
                let query = sqlx::query(&sql);
                let mut rows = query.fetch(conn_pool);

                let mut result = Vec::new();
                while let Some(row) = rows.try_next().await? {
                    let row_data = RowData::from_pg_row(&row, &tb_meta, &None);
                    result.push(row_data);
                }

                Ok::<_, anyhow::Error>(result)
            })
            .await;

            match res {
                Ok(Ok(v)) => return Ok(v),
                Ok(Err(e)) => last_err = Some(e),
                Err(_) => {
                    last_err = Some(anyhow::anyhow!(
                        "fetch_data_pg timed out for tb: {:?}",
                        db_tb
                    ))
                }
            }

            let retryable = attempt < 3
                && last_err
                    .as_ref()
                    .map(|e| Self::is_transient_error(e))
                    .unwrap_or(false);
            if retryable {
                sleep(Duration::from_millis(200 * attempt as u64)).await;
                continue;
            }
            break;
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("fetch_data_pg failed"))).with_context(
            || {
                format!(
                    "fetch_data_pg failed for tb: {:?}, where_sql: {}",
                    db_tb, where_sql
                )
            },
        )
    }

    pub async fn get_tb_meta_mysql(
        conn_pool: &Pool<MySql>,
        db_tb: &(String, String),
    ) -> anyhow::Result<MysqlTbMeta> {
        Self::get_tb_meta_mysql_compatible(conn_pool, db_tb, &DbType::Mysql).await
    }

    pub async fn get_tb_meta_mysql_compatible(
        conn_pool: &Pool<MySql>,
        db_tb: &(String, String),
        db_type: &DbType,
    ) -> anyhow::Result<MysqlTbMeta> {
        let mut meta_manager =
            MysqlMetaManager::new_mysql_compatible(conn_pool.to_owned(), db_type.to_owned())
                .await?;
        Ok(meta_manager
            .get_tb_meta(&db_tb.0, &db_tb.1)
            .await?
            .to_owned())
    }

    pub async fn get_tb_meta_pg(
        conn_pool: &Pool<Postgres>,
        db_tb: &(String, String),
    ) -> anyhow::Result<PgTbMeta> {
        let handle = Self::get_or_create_pg_meta_manager(conn_pool).await?;

        let mut last_err: Option<anyhow::Error> = None;
        for attempt in 1..=3 {
            let res = timeout(Duration::from_secs(10), async {
                let mut meta_manager = handle.lock().await;
                Ok::<_, anyhow::Error>(
                    meta_manager
                        .get_tb_meta(&db_tb.0, &db_tb.1)
                        .await?
                        .to_owned(),
                )
            })
            .await;

            match res {
                Ok(Ok(v)) => return Ok(v),
                Ok(Err(e)) => last_err = Some(e),
                Err(_) => {
                    last_err = Some(anyhow::anyhow!(
                        "get_tb_meta_pg timed out for tb: {:?}",
                        db_tb
                    ))
                }
            }

            let retryable = attempt < 3
                && last_err
                    .as_ref()
                    .map(|e| Self::is_transient_error(e))
                    .unwrap_or(false);
            if retryable {
                sleep(Duration::from_millis(200 * attempt as u64)).await;
                continue;
            }
            break;
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("get_tb_meta_pg failed")))
            .with_context(|| format!("get_tb_meta_pg failed for tb: {:?}", db_tb))
    }

    pub async fn execute_sqls_mysql(
        conn_pool: &Pool<MySql>,
        sqls: &Vec<String>,
    ) -> anyhow::Result<()> {
        for sql in sqls {
            println!("executing sql: {}", sql);
            let query = sqlx::query(sql).disable_arguments();
            query
                .execute(conn_pool)
                .await
                .with_context(|| format!("execute_sqls_mysql failed, sql: {}", sql))?;
        }
        Ok(())
    }

    pub async fn execute_sqls_pg(
        conn_pool: &Pool<Postgres>,
        sqls: &[String],
    ) -> anyhow::Result<()> {
        for sql in sqls.iter() {
            println!("executing sql: {}", sql);

            let lower = sql.trim_start().to_lowercase();
            let max_attempts = if lower.starts_with("update") || lower.starts_with("delete") {
                // UPDATE/DELETE are safe enough to retry on transient connection errors during
                // GaussDB HA/VIP flakiness. For INSERT, retry may cause duplicate key issues if
                // the first attempt succeeded but the client lost the response.
                15
            } else {
                1
            };

            let mut last_err: Option<anyhow::Error> = None;
            for attempt in 1..=max_attempts {
                let exec_res = sqlx::query(sql).execute(conn_pool).await;

                match exec_res {
                    Ok(_) => {
                        last_err = None;
                        break;
                    }
                    Err(e) => {
                        let err = anyhow::Error::new(e)
                            .context(format!("execute_sqls_pg failed, sql: {}", sql));
                        let retryable = attempt < max_attempts && Self::is_transient_error(&err);
                        if retryable {
                            println!(
                                "transient error executing sql (attempt {}/{}), will retry: {:#}",
                                attempt, max_attempts, err
                            );
                            last_err = Some(err);
                            let base_ms = 200_u64;
                            let exp = 2_u64.saturating_pow((attempt - 1) as u32);
                            let delay_ms = base_ms
                                .saturating_mul(exp)
                                .min(Duration::from_secs(5).as_millis() as u64);
                            sleep(Duration::from_millis(delay_ms)).await;
                            continue;
                        }
                        return Err(err);
                    }
                }
            }

            if let Some(err) = last_err {
                return Err(err);
            }
        }
        Ok(())
    }
}
