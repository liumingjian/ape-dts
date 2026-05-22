use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, OnceLock},
    time::Duration,
};

use anyhow::Context;
use dt_common::config::{config_enums::DbType, connection_auth_config::ConnectionAuthConfig};
use dt_common::meta::{
    adaptor::pg_col_value_convertor::PgColValueConvertor,
    col_value::ColValue,
    mysql::{mysql_meta_manager::MysqlMetaManager, mysql_tb_meta::MysqlTbMeta},
    pg::{pg_meta_manager::PgMetaManager, pg_tb_meta::PgTbMeta},
    row_data::RowData,
};
use dt_common::utils::sql_util::SqlUtil;
use dt_connector::oracle::OracleSqlPlusClient;
use dt_connector::rdb_query_builder::RdbQueryBuilder;
use futures::TryStreamExt;
use openssl::ssl::{SslConnector, SslMethod, SslVerifyMode};
use postgres_openssl::MakeTlsConnector;
use sqlx::{MySql, Pool, Postgres};
use tokio::sync::Mutex;
use tokio::time::{sleep, timeout};
use tokio_postgres::{Client, NoTls, SimpleQueryMessage};
use url::Url;

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
        // Cache is keyed by the address of `Pool<Postgres>` *value*. In dt-tests we frequently
        // create/close pools across retries, and the address can be reused by the allocator.
        // If we blindly reuse the old handle, we'll end up querying on a closed pool.
        let cached = { cache.lock().await.get(&key).cloned() };
        if let Some(handle) = cached {
            if !handle.lock().await.conn_pool.is_closed() {
                return Ok(handle);
            }
            // Drop the stale entry so we can rebuild from the current pool.
            cache.lock().await.remove(&key);
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
        Self::fetch_data_pg_compatible(conn_pool, ignore_cols, db_tb, &DbType::Pg, where_sql).await
    }

    pub async fn fetch_data_oracle(
        client: &OracleSqlPlusClient,
        db_tb: &(String, String),
        where_sql: &str,
    ) -> anyhow::Result<Vec<RowData>> {
        let owner = db_tb.0.to_uppercase().replace('\'', "''");
        let table = db_tb.1.to_uppercase().replace('\'', "''");
        let col_sql = format!(
            "SELECT column_name, data_type FROM all_tab_columns WHERE owner='{}' AND table_name='{}' ORDER BY column_id ASC",
            owner, table
        );
        let col_lines = client.query_lines(&col_sql).await?;

        let mut cols: Vec<(String, String)> = Vec::new();
        for line in col_lines {
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() < 2 {
                continue;
            }
            cols.push((parts[0].trim().to_string(), parts[1].trim().to_string()));
        }
        if cols.is_empty() {
            return Ok(Vec::new());
        }

        let select_cols = cols.iter().map(|(c, _)| c.as_str()).collect::<Vec<_>>();
        let order_col = select_cols[0];
        let select_sql = format!(
            "SELECT {} FROM {}.{} {} ORDER BY {} ASC",
            select_cols.join(","),
            db_tb.0,
            db_tb.1,
            where_sql,
            order_col
        );
        let lines = client.query_lines(&select_sql).await?;

        let mut out = Vec::with_capacity(lines.len());
        for line in lines {
            let values: Vec<&str> = line.split('|').collect();
            if values.len() != cols.len() {
                anyhow::bail!(
                    "oracle fetch_data column count mismatch: expected {}, got {}, line={}",
                    cols.len(),
                    values.len(),
                    line
                );
            }

            let mut after = HashMap::with_capacity(cols.len());
            for (idx, (name, ty)) in cols.iter().enumerate() {
                let raw = values[idx].trim();
                after.insert(name.clone(), Self::oracle_parse_col_value(raw, ty)?);
            }

            out.push(RowData::new(
                db_tb.0.clone(),
                db_tb.1.clone(),
                dt_common::meta::row_type::RowType::Insert,
                None,
                Some(after),
            ));
        }
        Ok(out)
    }

    pub async fn fetch_data_pg_compatible(
        conn_pool: &Pool<Postgres>,
        ignore_cols: Option<&HashSet<String>>,
        db_tb: &(String, String),
        db_type: &DbType,
        where_sql: &str,
    ) -> anyhow::Result<Vec<RowData>> {
        let mut last_err: Option<anyhow::Error> = None;
        for attempt in 1..=3 {
            let res = timeout(Duration::from_secs(15), async {
                let tb_meta = Self::get_tb_meta_pg(conn_pool, db_tb).await?;
                let query_builder =
                    RdbQueryBuilder::new_for_pg_compatible(&tb_meta, ignore_cols, db_type.clone());
                let cols_str = query_builder.build_extract_cols_str().with_context(|| {
                    format!("build_extract_cols_str failed for tb: {:?}", db_tb)
                })?;
                let sql = format!(
                    "SELECT {} FROM {}.{} {} ORDER BY {} ASC",
                    cols_str,
                    SqlUtil::escape_by_db_type(&db_tb.0, db_type),
                    SqlUtil::escape_by_db_type(&db_tb.1, db_type),
                    where_sql,
                    SqlUtil::escape_by_db_type(&tb_meta.basic.cols[0], db_type),
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
                    "fetch_data_pg_compatible failed for tb: {:?}, db_type: {:?}, where_sql: {}",
                    db_tb, db_type, where_sql
                )
            },
        )
    }

    fn oracle_parse_col_value(raw: &str, data_type: &str) -> anyhow::Result<ColValue> {
        if raw.is_empty() || raw == "<NULL>" {
            return Ok(ColValue::None);
        }

        let ty = data_type.trim().to_uppercase();
        Ok(match ty.as_str() {
            "NUMBER" => {
                if raw.contains('.') {
                    ColValue::Decimal(raw.to_string())
                } else if let Ok(v) = raw.parse::<i64>() {
                    ColValue::LongLong(v)
                } else {
                    ColValue::Decimal(raw.to_string())
                }
            }
            "FLOAT" | "BINARY_FLOAT" | "BINARY_DOUBLE" => ColValue::Double(raw.parse::<f64>()?),
            "DATE" => ColValue::DateTime(raw.to_string()),
            "TIMESTAMP" | "TIMESTAMP(6)" => ColValue::Timestamp(raw.to_string()),
            _ => ColValue::String(raw.to_string()),
        })
    }

    pub async fn fetch_data_gaussdb_mysql_simple_query(
        conn_pool: &Pool<Postgres>,
        url: &str,
        connection_auth: &ConnectionAuthConfig,
        ignore_cols: Option<&HashSet<String>>,
        db_tb: &(String, String),
        where_sql: &str,
    ) -> anyhow::Result<Vec<RowData>> {
        let handle = Self::get_or_create_pg_meta_manager(conn_pool).await?;
        let mut meta_manager = handle.lock().await;
        let tb_meta = meta_manager
            .get_tb_meta(&db_tb.0, &db_tb.1)
            .await?
            .to_owned();
        let cols: Vec<String> = tb_meta
            .basic
            .cols
            .iter()
            .filter(|col| !ignore_cols.is_some_and(|cols| cols.contains(*col)))
            .cloned()
            .collect();
        let order_col = tb_meta
            .basic
            .cols
            .first()
            .cloned()
            .context("gaussdb mysql simple query requires at least one column")?;
        let sql = format!(
            "SELECT {} FROM {}.{} {} ORDER BY {} ASC",
            cols.join(","),
            tb_meta.basic.schema,
            tb_meta.basic.tb,
            where_sql,
            order_col,
        );

        let client = Self::connect_pg_simple_client(url, connection_auth).await?;
        // GaussDB MySQL-compatible mode TIMESTAMP behaves like a timezone-aware type (similar to
        // timestamptz): its text output depends on the session TimeZone. For dt-tests we fetch
        // MySQL TIMESTAMP in UTC (sqlx initializes `session.time_zone='+00:00'`), so we align the
        // GaussDB session timezone to UTC to make comparisons deterministic.
        client
            .simple_query("SET TIME ZONE 'UTC'")
            .await
            .context("gaussdb mysql: failed to set session timezone to UTC")?;
        let messages = client.simple_query(&sql).await.with_context(|| {
            format!(
                "gaussdb mysql simple_query failed for tb: {:?}, sql: {}",
                db_tb, sql
            )
        })?;

        let mut result = Vec::new();
        for message in messages {
            if let SimpleQueryMessage::Row(row) = message {
                let mut after = HashMap::new();
                for col in cols.iter() {
                    let col_type = tb_meta.get_col_type(col)?;
                    let col_value = match row.get(col.as_str()) {
                        Some(value) => {
                            PgColValueConvertor::from_str(col_type, value, &mut meta_manager)?
                        }
                        None => ColValue::None,
                    };
                    after.insert(col.clone(), col_value);
                }
                result.push(RowData::build_insert_row_data(after, &tb_meta.basic));
            }
        }
        Ok(result)
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

    fn set_sslmode(url: &str, sslmode: &str) -> anyhow::Result<String> {
        let mut parsed = Url::parse(url)?;
        let mut query_pairs: Vec<(String, String)> = parsed
            .query_pairs()
            .filter(|(k, _)| k != "sslmode")
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        query_pairs.push(("sslmode".to_string(), sslmode.to_string()));
        parsed.set_query(None);
        {
            let mut pairs = parsed.query_pairs_mut();
            for (k, v) in query_pairs {
                pairs.append_pair(&k, &v);
            }
        }
        Ok(parsed.to_string())
    }

    async fn connect_pg_simple_client(
        url: &str,
        connection_auth: &ConnectionAuthConfig,
    ) -> anyhow::Result<Client> {
        let final_url = ConnectionAuthConfig::merge_url_with_auth(url, connection_auth)?;
        let prefer_ssl = Url::parse(&final_url)
            .ok()
            .and_then(|parsed| {
                parsed
                    .query_pairs()
                    .find(|(k, _)| k == "sslmode")
                    .map(|(_, v)| v.into_owned())
            })
            .map_or(true, |sslmode| sslmode != "disable");

        let connect_no_ssl = || async {
            let conn_info = Self::set_sslmode(&final_url, "disable")?;
            let (client, connection) = tokio_postgres::connect(&conn_info, NoTls).await?;
            tokio::spawn(async move {
                let _ = connection.await;
            });
            Ok::<_, anyhow::Error>(client)
        };

        let connect_ssl = || async {
            let conn_info = Self::set_sslmode(&final_url, "require")?;
            let mut builder = SslConnector::builder(SslMethod::tls())?;
            builder.set_verify(SslVerifyMode::NONE);
            let connector = MakeTlsConnector::new(builder.build());
            let (client, connection) = tokio_postgres::connect(&conn_info, connector).await?;
            tokio::spawn(async move {
                let _ = connection.await;
            });
            Ok::<_, anyhow::Error>(client)
        };

        if prefer_ssl {
            match connect_ssl().await {
                Ok(client) => Ok(client),
                Err(error) => {
                    let msg = error.to_string();
                    if msg.contains("SSL on") {
                        connect_no_ssl().await
                    } else {
                        Err(error)
                    }
                }
            }
        } else {
            match connect_no_ssl().await {
                Ok(client) => Ok(client),
                Err(error) => {
                    let msg = error.to_string();
                    if msg.contains("SSL off") {
                        connect_ssl().await
                    } else {
                        Err(error)
                    }
                }
            }
        }
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

    pub async fn execute_sqls_oracle(
        client: &OracleSqlPlusClient,
        sqls: &[String],
    ) -> anyhow::Result<()> {
        for sql in sqls.iter() {
            if sql.trim().is_empty() {
                continue;
            }
            println!("executing oracle sql: {}", sql);
            client.exec(sql).await?;
        }
        Ok(())
    }
}
