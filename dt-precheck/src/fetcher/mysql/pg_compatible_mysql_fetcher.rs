use std::collections::HashMap;

use anyhow::bail;
use async_trait::async_trait;
use dt_common::{
    config::connection_auth_config::ConnectionAuthConfig, error::Error, rdb_filter::RdbFilter,
    system_dbs::SystemDb,
};
use dt_task::task_util::TaskUtil;
use futures::{Stream, TryStreamExt};
use sqlx::{postgres::PgRow, query, Pool, Postgres, Row};

use crate::{
    fetcher::traits::Fetcher,
    meta::database_mode::{Constraint, Database, Schema, Table},
};

pub struct PgCompatibleMysqlFetcher {
    pub pool: Option<Pool<Postgres>>,
    pub url: String,
    pub connection_auth: ConnectionAuthConfig,
    pub is_source: bool,
    pub filter: RdbFilter,
}

#[async_trait]
impl Fetcher for PgCompatibleMysqlFetcher {
    async fn build_connection(&mut self) -> anyhow::Result<()> {
        self.pool = Some(
            TaskUtil::create_pg_conn_pool(&self.url, &self.connection_auth, 1, true, false).await?,
        );
        Ok(())
    }

    async fn fetch_version(&mut self) -> anyhow::Result<String> {
        let sql = String::from("SELECT version() AS version");
        let mut version = String::new();

        let result = self.fetch_all(sql, "gaussdb mysql-compatible query database version").await;
        match result {
            Ok(rows) => {
                if !rows.is_empty() {
                    version = rows.first().unwrap().get("version");
                }
            }
            Err(e) => bail! {e},
        }

        Ok(version)
    }

    async fn fetch_databases(&mut self) -> anyhow::Result<Vec<Database>> {
        let mut results: Vec<Database> = vec![];
        let query_db = "SELECT schema_name FROM information_schema.schemata";

        let rows_result = self.fetch_row(query_db, "gaussdb mysql-compatible query dbs sql:");
        match rows_result {
            Ok(mut rows) => {
                while let Some(row) = rows.try_next().await.unwrap() {
                    let schema_name: String = row.get("schema_name");
                    if !SystemDb::is_system_db(&schema_name, &self.filter.db_type)
                        && !self.filter.filter_schema(&schema_name)
                    {
                        results.push(Database {
                            database_name: schema_name,
                        })
                    }
                }
            }
            Err(e) => bail! {e},
        }

        Ok(results)
    }

    async fn fetch_schemas(&mut self) -> anyhow::Result<Vec<Schema>> {
        Ok(vec![])
    }

    async fn fetch_tables(&mut self) -> anyhow::Result<Vec<Table>> {
        let mut results: Vec<Table> = vec![];
        let query_tb = "SELECT table_schema, table_name FROM information_schema.tables WHERE table_type = 'BASE TABLE'";

        let rows_result = self.fetch_row(query_tb, "gaussdb mysql-compatible query tables sql:");
        match rows_result {
            Ok(mut rows) => {
                while let Some(row) = rows.try_next().await.unwrap() {
                    let (db, tb): (String, String) = (row.get("table_schema"), row.get("table_name"));
                    if !SystemDb::is_system_db(&db, &self.filter.db_type)
                        && !self.filter.filter_tb(&db, &tb)
                    {
                        results.push(Table {
                            database_name: db,
                            schema_name: String::new(),
                            table_name: tb,
                        })
                    }
                }
            }
            Err(e) => bail! {e},
        }

        Ok(results)
    }

    async fn fetch_constraints(&mut self) -> anyhow::Result<Vec<Constraint>> {
        let mut results: Vec<Constraint> = vec![];
        let system_dbs = SystemDb::get_system_dbs(&self.filter.db_type)
            .unwrap_or_default()
            .into_iter()
            .map(|s| format!("'{}'", s.replace('\'', "''")))
            .collect::<Vec<_>>()
            .join(",");

        let query_constraint = format!(
            "SELECT
              kcu.constraint_name AS constraint_name,
              tc.constraint_type AS constraint_type,
              kcu.constraint_schema AS constraint_schema,
              kcu.table_name AS table_name,
              kcu.column_name AS column_name,
              kcu.referenced_table_schema AS referenced_table_schema,
              kcu.referenced_table_name AS referenced_table_name,
              kcu.referenced_column_name AS referenced_column_name
            FROM information_schema.key_column_usage kcu
            JOIN information_schema.table_constraints tc
              ON kcu.constraint_name = tc.constraint_name
             AND kcu.constraint_schema = tc.constraint_schema
             AND kcu.table_name = tc.table_name
            WHERE kcu.constraint_schema NOT IN ({})",
            system_dbs
        );

        let rows_result = self.fetch_row(
            &query_constraint,
            "gaussdb mysql-compatible query constraints sql:",
        );
        match rows_result {
            Ok(mut rows) => {
                while let Some(row) = rows.try_next().await.unwrap() {
                    let (db, table, rel_db, rel_table, constraint_name, constraint_type): (
                        String,
                        String,
                        String,
                        String,
                        String,
                        String,
                    ) = (
                        Self::get_text_with_null(&row, "constraint_schema").unwrap(),
                        Self::get_text_with_null(&row, "table_name").unwrap(),
                        Self::get_text_with_null(&row, "referenced_table_schema").unwrap(),
                        Self::get_text_with_null(&row, "referenced_table_name").unwrap(),
                        Self::get_text_with_null(&row, "constraint_name").unwrap(),
                        Self::get_text_with_null(&row, "constraint_type").unwrap(),
                    );
                    if !self.filter.filter_tb(&db, &table) {
                        results.push(Constraint {
                            database_name: db,
                            schema_name: String::new(),
                            table_name: table,
                            column_name: String::new(),
                            rel_database_name: rel_db,
                            rel_schema_name: String::new(),
                            rel_table_name: rel_table,
                            rel_column_name: String::new(),
                            constraint_name,
                            constraint_type,
                        })
                    }
                }
            }
            Err(e) => bail! {e},
        }

        Ok(results)
    }

    async fn fetch_configuration(
        &mut self,
        _config_keys: Vec<String>,
    ) -> anyhow::Result<HashMap<String, String>> {
        Ok(HashMap::new())
    }
}

impl PgCompatibleMysqlFetcher {
    async fn fetch_all(&self, sql: String, mut sql_msg: &str) -> anyhow::Result<Vec<PgRow>> {
        let pg_pool = match &self.pool {
            Some(pool) => pool,
            None => bail! {Error::from(sqlx::Error::PoolClosed)},
        };

        sql_msg = if sql_msg.is_empty() { "sql" } else { sql_msg };
        println!("{}: {}", sql_msg, sql);

        let rows_result = query(&sql).fetch_all(pg_pool).await;
        match rows_result {
            Ok(rows) => Ok(rows),
            Err(e) => bail! {Error::from(e)},
        }
    }

    fn fetch_row<'a>(
        &self,
        sql: &'a str,
        mut sql_msg: &str,
    ) -> anyhow::Result<impl Stream<Item = anyhow::Result<PgRow, sqlx::Error>> + 'a> {
        match &self.pool {
            Some(pool) => {
                sql_msg = if sql_msg.is_empty() { "sql" } else { sql_msg };
                println!("{}: {}", sql_msg, sql);
                Ok(query(sql).fetch(pool))
            }
            None => bail! {Error::from(sqlx::Error::PoolClosed)},
        }
    }

    fn get_text_with_null(row: &PgRow, col_name: &str) -> anyhow::Result<String> {
        let mut str_val = String::new();
        if let Some(s) = row.get(col_name) {
            str_val = s;
        }
        Ok(str_val)
    }
}
