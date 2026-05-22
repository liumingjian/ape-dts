use std::collections::{HashMap, HashSet};

use anyhow::{bail, Context};
use async_trait::async_trait;

use crate::extractor::base_extractor::BaseExtractor;
use crate::oracle::OracleSqlPlusClient;
use crate::Extractor;
use dt_common::log_info;
use dt_common::meta::struct_meta::{
    statement::{
        pg_create_schema_statement::PgCreateSchemaStatement,
        pg_create_table_statement::PgCreateTableStatement, struct_statement::StructStatement,
    },
    struct_data::StructData,
    structure::{
        column::Column,
        constraint::{Constraint, ConstraintType},
        schema::Schema,
        table::Table,
    },
};
use dt_common::rdb_filter::RdbFilter;

#[derive(Debug, Clone)]
struct OracleColumnMeta {
    name: String,
    data_type: String,
    data_length: Option<u32>,
    data_precision: Option<u32>,
    data_scale: Option<u32>,
    nullable: bool,
    ordinal_position: u32,
}

pub struct OracleStructExtractor {
    pub base_extractor: BaseExtractor,
    pub client: OracleSqlPlusClient,
    pub schemas: Vec<String>,
    pub filter: RdbFilter,
    pub db_batch_size: usize,
}

#[async_trait]
impl Extractor for OracleStructExtractor {
    async fn extract(&mut self) -> anyhow::Result<()> {
        log_info!(
            "OracleStructExtractor starts, schemas: {}, db_batch_size: {}",
            self.schemas.join(","),
            self.db_batch_size
        );

        let schema_chunks: Vec<Vec<String>> = self
            .schemas
            .chunks(self.db_batch_size)
            .map(|chunk| chunk.to_vec())
            .collect();
        for schema_chunk in schema_chunks.into_iter() {
            self.extract_internal(schema_chunk.into_iter().collect())
                .await?;
        }
        self.base_extractor.wait_task_finish().await
    }

    async fn close(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

impl OracleStructExtractor {
    async fn extract_internal(&mut self, schemas: HashSet<String>) -> anyhow::Result<()> {
        for schema in schemas.iter() {
            if self.filter.filter_schema(schema) {
                continue;
            }

            self.push_struct(StructStatement::PgCreateSchema(PgCreateSchemaStatement {
                schema: Schema {
                    name: schema.clone(),
                },
            }))
            .await?;

            for tb in self.fetch_tables(schema).await? {
                if self.filter.filter_tb(schema, &tb) {
                    continue;
                }
                let stmt = self.build_create_table_statement(schema, &tb).await?;
                self.push_struct(StructStatement::PgCreateTable(stmt))
                    .await?;
            }
        }
        Ok(())
    }

    async fn push_struct(&mut self, statement: StructStatement) -> anyhow::Result<()> {
        self.base_extractor
            .push_struct(StructData {
                schema: "".to_string(),
                statement,
            })
            .await
    }

    async fn fetch_tables(&self, schema: &str) -> anyhow::Result<Vec<String>> {
        let owner = escape_sql_literal(&schema.to_uppercase());
        let sql = format!(
            "SELECT table_name FROM all_tables WHERE owner='{}' ORDER BY table_name ASC",
            owner
        );
        self.client.query_lines(&sql).await
    }

    async fn build_create_table_statement(
        &self,
        schema: &str,
        tb: &str,
    ) -> anyhow::Result<PgCreateTableStatement> {
        let mut stmt = PgCreateTableStatement {
            table: Table {
                schema_name: schema.to_string(),
                table_name: tb.to_string(),
                columns: self.fetch_columns(schema, tb).await?,
                ..Default::default()
            },
            table_comments: Vec::new(),
            column_comments: Vec::new(),
            constraints: Vec::new(),
            indexes: Vec::new(),
            sequences: Vec::new(),
            sequence_owners: Vec::new(),
        };

        if let Some(pk) = self.fetch_primary_key(schema, tb).await? {
            let (pk_name, pk_cols) = pk;
            let (dst_schema, dst_tb) = self.base_extractor.router.get_tb_map(schema, tb);
            let constraint_name = format!("{}_pk", dst_tb);
            let definition = format!("PRIMARY KEY ({})", join_quoted_cols(&pk_cols));
            stmt.constraints.push(Constraint {
                database_name: String::new(),
                schema_name: dst_schema.to_string(),
                table_name: dst_tb.to_string(),
                constraint_name: if pk_name.trim().is_empty() {
                    constraint_name
                } else {
                    pk_name
                },
                constraint_type: ConstraintType::Primary,
                definition,
            });
        }

        Ok(stmt)
    }

    async fn fetch_columns(&self, schema: &str, tb: &str) -> anyhow::Result<Vec<Column>> {
        let owner = escape_sql_literal(&schema.to_uppercase());
        let table = escape_sql_literal(&tb.to_uppercase());
        let sql = format!(
            "SELECT column_name, data_type, data_length, data_precision, data_scale, nullable, column_id \
             FROM all_tab_columns \
             WHERE owner='{}' AND table_name='{}' \
             ORDER BY column_id ASC",
            owner, table
        );

        let col_map = self
            .base_extractor
            .router
            .get_col_map(schema, tb)
            .cloned()
            .unwrap_or_default();

        let lines = self.client.query_lines(&sql).await?;
        let mut cols = Vec::new();
        for line in lines {
            let meta = parse_oracle_column_meta(&line).with_context(|| {
                format!(
                    "failed to parse oracle column meta line: schema={}, tb={}, line={}",
                    schema, tb, line
                )
            })?;

            let mapped_name = col_map.get(&meta.name).cloned().unwrap_or(meta.name);
            let column_type = oracle_to_pg_column_type(
                &meta.data_type,
                meta.data_length,
                meta.data_precision,
                meta.data_scale,
            )?;

            cols.push(Column {
                column_name: mapped_name,
                ordinal_position: meta.ordinal_position,
                is_nullable: meta.nullable,
                column_type,
                ..Default::default()
            });
        }
        if cols.is_empty() {
            bail!("oracle columns not found for {}.{}", schema, tb);
        }
        Ok(cols)
    }

    async fn fetch_primary_key(
        &self,
        schema: &str,
        tb: &str,
    ) -> anyhow::Result<Option<(String, Vec<String>)>> {
        let owner = escape_sql_literal(&schema.to_uppercase());
        let table = escape_sql_literal(&tb.to_uppercase());

        let pk_name_sql = format!(
            "SELECT constraint_name FROM all_constraints \
             WHERE owner='{}' AND table_name='{}' AND constraint_type='P' \
             ORDER BY constraint_name ASC",
            owner, table
        );
        let pk_name = self
            .client
            .query_lines(&pk_name_sql)
            .await?
            .into_iter()
            .next()
            .unwrap_or_default();
        if pk_name.trim().is_empty() {
            return Ok(None);
        }

        let cols_sql = format!(
            "SELECT cols.column_name \
             FROM all_constraints cons \
             JOIN all_cons_columns cols \
               ON cons.owner=cols.owner AND cons.constraint_name=cols.constraint_name \
             WHERE cons.owner='{}' AND cons.table_name='{}' AND cons.constraint_type='P' \
             ORDER BY cols.position ASC",
            owner, table
        );
        let src_cols = self.client.query_lines(&cols_sql).await?;
        if src_cols.is_empty() {
            return Ok(None);
        }

        let col_map: HashMap<String, String> = self
            .base_extractor
            .router
            .get_col_map(schema, tb)
            .cloned()
            .unwrap_or_default();
        let mapped = src_cols
            .into_iter()
            .map(|c| col_map.get(&c).cloned().unwrap_or(c))
            .collect();
        Ok(Some((pk_name, mapped)))
    }
}

fn parse_oracle_column_meta(line: &str) -> anyhow::Result<OracleColumnMeta> {
    let parts: Vec<&str> = line.split('|').map(|s| s.trim()).collect();
    if parts.len() < 7 {
        bail!(
            "invalid oracle column meta line (expected 7 cols): {}",
            line
        );
    }

    Ok(OracleColumnMeta {
        name: parts[0].to_string(),
        data_type: parts[1].to_string(),
        data_length: parse_opt_u32(parts[2])?,
        data_precision: parse_opt_u32(parts[3])?,
        data_scale: parse_opt_u32(parts[4])?,
        nullable: parse_nullable(parts[5])?,
        ordinal_position: parse_required_u32(parts[6])
            .with_context(|| format!("invalid column_id in oracle column meta: {}", parts[6]))?,
    })
}

fn oracle_to_pg_column_type(
    data_type: &str,
    data_length: Option<u32>,
    data_precision: Option<u32>,
    data_scale: Option<u32>,
) -> anyhow::Result<String> {
    let ty = data_type.trim().to_uppercase();
    if ty.starts_with("TIMESTAMP") {
        return Ok("TIMESTAMP".to_string());
    }

    match ty.as_str() {
        "NUMBER" => Ok(map_oracle_number(data_precision, data_scale)),
        "VARCHAR2" | "NVARCHAR2" => Ok(format!(
            "VARCHAR({})",
            data_length.context("oracle VARCHAR2 requires data_length")?
        )),
        "CHAR" | "NCHAR" => Ok(format!(
            "CHAR({})",
            data_length.context("oracle CHAR requires data_length")?
        )),
        "DATE" => Ok("TIMESTAMP".to_string()),
        "CLOB" | "NCLOB" => Ok("TEXT".to_string()),
        "BLOB" => Ok("BYTEA".to_string()),
        other => bail!("unsupported oracle column data_type: {}", other),
    }
}

fn map_oracle_number(precision: Option<u32>, scale: Option<u32>) -> String {
    let scale = scale.unwrap_or(0);
    let Some(precision) = precision else {
        return "NUMERIC".to_string();
    };
    if scale != 0 {
        return format!("NUMERIC({},{})", precision, scale);
    }
    if precision <= 9 {
        return "INT".to_string();
    }
    if precision <= 18 {
        return "BIGINT".to_string();
    }
    format!("NUMERIC({})", precision)
}

fn parse_nullable(raw: &str) -> anyhow::Result<bool> {
    let v = raw.trim();
    if v.eq_ignore_ascii_case("Y") {
        return Ok(true);
    }
    if v.eq_ignore_ascii_case("N") {
        return Ok(false);
    }
    bail!("invalid oracle nullable flag: {}", raw);
}

fn parse_opt_u32(raw: &str) -> anyhow::Result<Option<u32>> {
    let v = raw.trim();
    if v.is_empty() || v == "<NULL>" {
        return Ok(None);
    }
    Ok(Some(
        v.parse::<u32>()
            .with_context(|| format!("invalid integer: {}", raw))?,
    ))
}

fn parse_required_u32(raw: &str) -> anyhow::Result<u32> {
    raw.trim()
        .parse::<u32>()
        .with_context(|| format!("invalid integer: {}", raw))
}

fn join_quoted_cols(cols: &[String]) -> String {
    cols.iter()
        .map(|c| format!(r#""{}""#, c.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(",")
}

fn escape_sql_literal(s: &str) -> String {
    s.replace('\'', "''")
}
