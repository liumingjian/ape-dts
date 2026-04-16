use std::collections::HashMap;

use anyhow::{bail, Context};
use async_trait::async_trait;
use dt_common::{
    config::connection_auth_config::ConnectionAuthConfig,
    rdb_filter::RdbFilter,
};
use dt_connector::oracle::OracleSqlPlusClient;

use crate::{
    fetcher::traits::Fetcher,
    meta::database_mode::{Constraint, Database, Schema, Table},
};

pub struct OracleFetcher {
    pub client: Option<OracleSqlPlusClient>,
    pub url: String,
    pub connection_auth: ConnectionAuthConfig,
    pub is_source: bool,
    pub filter: RdbFilter,
    current_user: Option<String>,
}

#[async_trait]
impl Fetcher for OracleFetcher {
    async fn build_connection(&mut self) -> anyhow::Result<()> {
        let client = OracleSqlPlusClient::new(self.url.clone(), self.connection_auth.clone());
        let lines = client.query_lines("SELECT USER FROM dual").await?;
        let user = lines
            .get(0)
            .map(|s| s.trim().to_uppercase())
            .filter(|s| !s.is_empty())
            .context("oracle current user is empty")?;

        self.current_user = Some(user);
        self.client = Some(client);
        Ok(())
    }

    async fn fetch_version(&mut self) -> anyhow::Result<String> {
        let client = self.client.as_ref().context("oracle client not built")?;

        // `product_component_version` is accessible with minimal privileges and works on Oracle XE 11g.
        let lines = client
            .query_lines(
                "SELECT version FROM product_component_version WHERE product LIKE 'Oracle Database%'",
            )
            .await?;
        for line in lines {
            let s = line.trim();
            if !s.is_empty() && s.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                return Ok(s.to_string());
            }
        }
        Ok(String::new())
    }

    async fn fetch_configuration(
        &mut self,
        _config_keys: Vec<String>,
    ) -> anyhow::Result<HashMap<String, String>> {
        Ok(HashMap::new())
    }

    async fn fetch_databases(&mut self) -> anyhow::Result<Vec<Database>> {
        Ok(vec![])
    }

    async fn fetch_schemas(&mut self) -> anyhow::Result<Vec<Schema>> {
        // Bootstrap Oracle connector only supports current user schema.
        let user = self.current_user()?;
        Ok(vec![Schema {
            database_name: String::new(),
            schema_name: user.to_string(),
        }])
    }

    async fn fetch_tables(&mut self) -> anyhow::Result<Vec<Table>> {
        let client = self.client.as_ref().context("oracle client not built")?;
        let user = self.current_user()?;

        let lines = client.query_lines("SELECT table_name FROM user_tables").await?;
        let mut tables = Vec::new();
        for line in lines {
            let table_name = line.trim().to_uppercase();
            if table_name.is_empty() {
                continue;
            }
            if self.filter.filter_tb(user, &table_name) {
                continue;
            }
            tables.push(Table {
                database_name: String::new(),
                schema_name: user.to_string(),
                table_name,
            });
        }
        Ok(tables)
    }

    async fn fetch_constraints(&mut self) -> anyhow::Result<Vec<Constraint>> {
        let client = self.client.as_ref().context("oracle client not built")?;
        let user = self.current_user()?;

        // Return table-level constraint metadata to support the generic "must have pk/uk + fk ref exists" checks.
        let lines = client
            .query_lines(
                r#"
SELECT
  c.constraint_name,
  c.constraint_type,
  c.table_name,
  r.owner,
  r.table_name
FROM user_constraints c
LEFT JOIN all_constraints r
  ON c.r_owner = r.owner AND c.r_constraint_name = r.constraint_name
WHERE c.constraint_type IN ('P','U','R')
ORDER BY c.table_name, c.constraint_name
"#,
            )
            .await?;

        let mut out = Vec::new();
        for line in lines {
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() < 5 {
                continue;
            }

            let constraint_name = parts[0].trim().to_string();
            let constraint_type_raw = parts[1].trim().to_uppercase();
            let table_name = parts[2].trim().to_uppercase();
            let rel_schema_name = normalize_sqlplus_null(parts[3]).to_uppercase();
            let rel_table_name = normalize_sqlplus_null(parts[4]).to_uppercase();

            if constraint_name.is_empty() || constraint_type_raw.is_empty() || table_name.is_empty() {
                continue;
            }

            if self.filter.filter_tb(user, &table_name) {
                continue;
            }

            let constraint_type = match constraint_type_raw.as_str() {
                "P" => "p",
                "U" => "u",
                "R" => "f",
                other => bail!("unsupported oracle constraint type: {}", other),
            }
            .to_string();

            out.push(Constraint {
                database_name: String::new(),
                schema_name: user.to_string(),
                table_name,
                column_name: String::new(),
                rel_database_name: String::new(),
                rel_schema_name,
                rel_table_name,
                rel_column_name: String::new(),
                constraint_name,
                constraint_type,
            });
        }

        Ok(out)
    }
}

impl OracleFetcher {
    pub fn new(
        url: String,
        connection_auth: ConnectionAuthConfig,
        is_source: bool,
        filter: RdbFilter,
    ) -> Self {
        Self {
            client: None,
            url,
            connection_auth,
            is_source,
            filter,
            current_user: None,
        }
    }

    pub fn current_user(&self) -> anyhow::Result<&str> {
        self.current_user
            .as_deref()
            .context("oracle current user is missing; call build_connection first")
    }

    pub fn client(&self) -> anyhow::Result<&OracleSqlPlusClient> {
        self.client.as_ref().context("oracle client not built")
    }
}

fn normalize_sqlplus_null(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed == "<NULL>" {
        String::new()
    } else {
        trimmed.to_string()
    }
}
