use anyhow::Context;
use async_trait::async_trait;
use futures::TryStreamExt;
use openssl::ssl::{SslConnector, SslMethod, SslVerifyMode};
use postgres_openssl::MakeTlsConnector;
use sqlx::{Pool, Postgres};
use std::collections::{HashMap, HashSet};
use tokio_postgres::{Client, NoTls, SimpleQueryMessage};
use url::Url;

use crate::{
    meta_fetcher::pg::pg_struct_fetcher::PgStructFetcher,
    rdb_query_builder::RdbQueryBuilder,
    sinker::base_checker::{BaseChecker, Checker, CheckerCommon, CheckerTbMeta},
};
use dt_common::{
    config::{config_enums::DbType, connection_auth_config::ConnectionAuthConfig},
    meta::{
        adaptor::pg_col_value_convertor::PgColValueConvertor,
        col_value::ColValue,
        pg::{pg_meta_manager::PgMetaManager, pg_tb_meta::PgTbMeta},
        row_data::RowData,
        struct_meta::statement::struct_statement::StructStatement,
    },
};

#[derive(Clone)]
pub struct PgChecker {
    pub url: String,
    pub connection_auth: ConnectionAuthConfig,
    pub db_type: DbType,
    pub conn_pool: Pool<Postgres>,
    pub meta_manager: PgMetaManager,
    pub common: CheckerCommon,
}

#[async_trait]
impl Checker for PgChecker {
    fn common_mut(&mut self) -> &mut CheckerCommon {
        &mut self.common
    }

    async fn get_tb_meta_by_row(&mut self, row: &RowData) -> anyhow::Result<CheckerTbMeta> {
        Ok(CheckerTbMeta::Pg(
            self.meta_manager
                .get_tb_meta_by_row_data(row)
                .await?
                .clone(),
        ))
    }

    async fn fetch_batch(
        &self,
        tb_meta: &CheckerTbMeta,
        data: &[&RowData],
    ) -> anyhow::Result<Vec<RowData>> {
        let pg_meta = tb_meta.pg()?;
        if matches!(self.db_type, DbType::GaussDBMySQL) {
            return self.fetch_batch_simple_query(pg_meta, data).await;
        }
        let qb = RdbQueryBuilder::new_for_pg(pg_meta, None);

        let mut res = Vec::with_capacity(data.len());
        let mut batch_rows = Vec::with_capacity(data.len());

        for &row in data {
            if BaseChecker::has_null_key(row, &pg_meta.basic.id_cols) {
                let query_info = qb.get_select_query(row)?;
                let query = qb.create_pg_query(&query_info)?;
                let mut rows = query.fetch(&self.conn_pool);
                while let Some(r) = rows.try_next().await? {
                    res.push(RowData::from_pg_row(&r, pg_meta, &None));
                }
            } else {
                batch_rows.push(row);
            }
        }

        if !batch_rows.is_empty() {
            let query_info = qb.get_batch_select_query(&batch_rows, 0, batch_rows.len())?;
            let query = qb.create_pg_query(&query_info)?;
            let mut rows = query.fetch(&self.conn_pool);
            while let Some(row) = rows.try_next().await? {
                res.push(RowData::from_pg_row(&row, pg_meta, &None));
            }
        }

        Ok(res)
    }

    async fn fetch_dst_struct(&self, src: &StructStatement) -> anyhow::Result<StructStatement> {
        let schema = match src {
            StructStatement::PgCreateSchema(s) => s.schema.name.clone(),
            StructStatement::PgCreateTable(s) => s.table.schema_name.clone(),
            _ => return Ok(StructStatement::Unknown),
        };

        let mut struct_fetcher = PgStructFetcher {
            conn_pool: self.conn_pool.to_owned(),
            db_type: self.db_type.clone(),
            schemas: HashSet::from([schema.clone()]),
            filter: None,
        };

        match src {
            StructStatement::PgCreateSchema(_) => {
                let statement = struct_fetcher
                    .get_create_schema_statements(&schema)
                    .await?
                    .into_iter()
                    .next();
                Ok(statement
                    .map(StructStatement::PgCreateSchema)
                    .unwrap_or(StructStatement::Unknown))
            }
            StructStatement::PgCreateTable(statement) => {
                let statement = struct_fetcher
                    .get_create_table_statements(&schema, &statement.table.table_name)
                    .await?
                    .into_iter()
                    .next();
                Ok(statement
                    .map(StructStatement::PgCreateTable)
                    .unwrap_or(StructStatement::Unknown))
            }
            _ => Ok(StructStatement::Unknown),
        }
    }
}

impl PgChecker {
    async fn fetch_batch_simple_query(
        &self,
        pg_meta: &PgTbMeta,
        data: &[&RowData],
    ) -> anyhow::Result<Vec<RowData>> {
        let qb = RdbQueryBuilder::new_for_pg_compatible(pg_meta, None, self.db_type.clone());
        let client = self.connect_simple_client().await?;
        let mut res = Vec::with_capacity(data.len());
        let mut batch_rows = Vec::with_capacity(data.len());
        let mut meta_manager = self.meta_manager.clone();

        for &row in data {
            if BaseChecker::has_null_key(row, &pg_meta.basic.id_cols) {
                let sql = qb.get_select_query_sql(row)?;
                self.append_simple_query_rows(&client, &sql, pg_meta, &mut meta_manager, &mut res)
                    .await?;
            } else {
                batch_rows.push(row);
            }
        }

        if !batch_rows.is_empty() {
            let sql = qb.get_batch_select_query_sql(&batch_rows, 0, batch_rows.len())?;
            self.append_simple_query_rows(&client, &sql, pg_meta, &mut meta_manager, &mut res)
                .await?;
        }

        Ok(res)
    }

    async fn append_simple_query_rows(
        &self,
        client: &Client,
        sql: &str,
        pg_meta: &PgTbMeta,
        meta_manager: &mut PgMetaManager,
        result: &mut Vec<RowData>,
    ) -> anyhow::Result<()> {
        let messages = client.simple_query(sql).await.with_context(|| {
            format!(
                "gaussdb mysql simple_query failed, schema: {}, tb: {}, sql: {}",
                pg_meta.basic.schema, pg_meta.basic.tb, sql
            )
        })?;

        for message in messages {
            if let SimpleQueryMessage::Row(row) = message {
                let mut after = HashMap::new();
                for col in pg_meta.basic.cols.iter() {
                    let col_type = pg_meta.get_col_type(col)?;
                    let col_value = match row.get(col.as_str()) {
                        Some(value) => {
                            PgColValueConvertor::from_str(col_type, value, meta_manager)?
                        }
                        None => ColValue::None,
                    };
                    after.insert(col.clone(), col_value);
                }
                result.push(RowData::build_insert_row_data(after, &pg_meta.basic));
            }
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

    async fn connect_simple_client(&self) -> anyhow::Result<Client> {
        let final_url =
            ConnectionAuthConfig::merge_url_with_auth(&self.url, &self.connection_auth)?;
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
                    if error.to_string().contains("SSL on") {
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
                    if error.to_string().contains("SSL off") {
                        connect_ssl().await
                    } else {
                        Err(error)
                    }
                }
            }
        }
    }
}
