use std::cmp;
use std::sync::Arc;

use anyhow::{bail, Context};
use async_trait::async_trait;
use tokio::time::Instant;

use crate::oracle::OracleSqlPlusClient;
use crate::sinker::base_sinker::BaseSinker;
use crate::Sinker;
use dt_common::config::config_enums::ConflictPolicyEnum;
use dt_common::log_error;
use dt_common::log_info;
use dt_common::meta::struct_meta::statement::struct_statement::StructStatement;
use dt_common::meta::struct_meta::structure::column::ColumnDefault;
use dt_common::meta::struct_meta::structure::constraint::ConstraintType;
use dt_common::meta::struct_meta::structure::structure_type::StructureType;
use dt_common::meta::struct_meta::struct_data::StructData;
use dt_common::monitor::monitor::Monitor;
use dt_common::rdb_filter::RdbFilter;
use dt_common::utils::limit_queue::LimitedQueue;

const ORACLE_IDENT_MAX_LEN: usize = 30;

#[derive(Clone)]
pub struct OracleStructSinker {
    pub client: OracleSqlPlusClient,
    pub conflict_policy: ConflictPolicyEnum,
    pub filter: RdbFilter,
    pub monitor: Arc<Monitor>,
    pub monitor_interval: u64,
}

#[async_trait]
impl Sinker for OracleStructSinker {
    async fn sink_struct(&mut self, data: Vec<StructData>) -> anyhow::Result<()> {
        if data.is_empty() {
            return Ok(());
        }

        let monitor_interval_secs = cmp::max(1, self.monitor_interval);
        let mut rts = LimitedQueue::new(cmp::min(100, data.len()));
        let mut last_monitor_time = Instant::now();
        let mut record_count = 0u64;

        for struct_data in data {
            record_count += 1;
            let mut statement = struct_data.statement;
            let sqls = oracle_struct_sqls(&mut statement, &self.filter)?;
            for (_key, sql) in sqls {
                log_info!("oracle struct ddl begin: {}", sql);
                let start_time = Instant::now();
                match self.client.exec(&sql).await {
                    Ok(()) => {
                        log_info!("oracle struct ddl succeed");
                    }
                    Err(error) => {
                        log_error!("oracle struct ddl failed: {}", error);
                        match self.conflict_policy {
                            ConflictPolicyEnum::Interrupt => bail!(error),
                            ConflictPolicyEnum::Ignore => continue,
                        }
                    }
                }

                rts.push((start_time.elapsed().as_millis() as u64, 1));
                if last_monitor_time.elapsed().as_secs() >= monitor_interval_secs {
                    BaseSinker::update_serial_monitor(&self.monitor, record_count, 0).await?;
                    BaseSinker::update_monitor_rt(&self.monitor, &rts).await?;
                    rts.clear();
                    record_count = 0;
                    last_monitor_time = Instant::now();
                }
            }
        }

        if record_count > 0 {
            BaseSinker::update_serial_monitor(&self.monitor, record_count, 0).await?;
            BaseSinker::update_monitor_rt(&self.monitor, &rts).await?;
        }
        Ok(())
    }

    async fn close(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

fn oracle_struct_sqls(
    statement: &mut StructStatement,
    filter: &RdbFilter,
) -> anyhow::Result<Vec<(String, String)>> {
    match statement {
        StructStatement::PgCreateTable(s) => {
            let mut out = Vec::new();
            let schema = oracle_ident(&s.table.schema_name)?;
            let table = oracle_ident(&s.table.table_name)?;

            if !filter.filter_structure(&StructureType::Table) {
                let sql = build_oracle_create_table_sql(&schema, &table, &s.table.columns)?;
                out.push((format!("table.{}.{}", schema, table), sql));
            }

            if !filter.filter_structure(&StructureType::Constraint) {
                for c in s.constraints.iter() {
                    let Some(sql) = build_oracle_constraint_sql(&schema, &table, c)? else {
                        continue;
                    };
                    out.push((
                        format!(
                            "constraint.{}.{}.{}",
                            schema,
                            table,
                            oracle_ident(&c.constraint_name)?
                        ),
                        sql,
                    ));
                }
            }

            Ok(out)
        }

        StructStatement::PgCreateSchema(s) => {
            if filter.filter_structure(&StructureType::Database) {
                return Ok(vec![]);
            }
            bail!(
                "oracle struct sinker does not support PgCreateSchema (schema={}): pre-create user and set filter.do_structures without 'database'",
                s.schema.name
            );
        }

        other => bail!(
            "oracle struct sinker only supports PgCreateTable in this epic, got: {:?}",
            other
        ),
    }
}

fn build_oracle_create_table_sql(
    schema: &str,
    table: &str,
    columns: &[dt_common::meta::struct_meta::structure::column::Column],
) -> anyhow::Result<String> {
    if columns.is_empty() {
        bail!("oracle struct: empty columns for {}.{}", schema, table);
    }

    let mut cols = columns.to_vec();
    cols.sort_by(|a, b| a.ordinal_position.cmp(&b.ordinal_position));

    let mut parts = Vec::with_capacity(cols.len());
    for col in cols.iter() {
        if col.generated.is_some() {
            bail!(
                "oracle struct: generated/identity columns are not supported on Oracle XE 11g: {}.{}.{}",
                schema,
                table,
                col.column_name
            );
        }

        let name = oracle_ident(&col.column_name)?;
        let ty = pg_type_to_oracle(&col.column_type).with_context(|| {
            format!(
                "oracle struct: map column type failed: {}.{}.{} type={}",
                schema, table, col.column_name, col.column_type
            )
        })?;
        let mut seg = format!("{} {}", name, ty);

        if let Some(default) = &col.column_default {
            let default_sql = match default {
                ColumnDefault::Literal(v) | ColumnDefault::Expression(v) => v.trim(),
            };
            if !default_sql.is_empty() {
                seg.push_str(" DEFAULT ");
                seg.push_str(default_sql);
            }
        }

        if !col.is_nullable {
            seg.push_str(" NOT NULL");
        }
        parts.push(seg);
    }

    Ok(format!(
        "CREATE TABLE {}.{} ({})",
        schema,
        table,
        parts.join(", ")
    ))
}

fn build_oracle_constraint_sql(
    schema: &str,
    table: &str,
    constraint: &dt_common::meta::struct_meta::structure::constraint::Constraint,
) -> anyhow::Result<Option<String>> {
    match constraint.constraint_type {
        ConstraintType::Primary => {
            let cols = parse_constraint_cols(&constraint.definition)?;
            let name = oracle_ident(&constraint.constraint_name)?;
            Ok(Some(format!(
                "ALTER TABLE {}.{} ADD CONSTRAINT {} PRIMARY KEY ({})",
                schema,
                table,
                name,
                cols.into_iter()
                    .map(|c| oracle_ident(&c))
                    .collect::<anyhow::Result<Vec<_>>>()?
                    .join(", ")
            )))
        }
        ConstraintType::Unique => {
            let cols = parse_constraint_cols(&constraint.definition)?;
            let name = oracle_ident(&constraint.constraint_name)?;
            Ok(Some(format!(
                "ALTER TABLE {}.{} ADD CONSTRAINT {} UNIQUE ({})",
                schema,
                table,
                name,
                cols.into_iter()
                    .map(|c| oracle_ident(&c))
                    .collect::<anyhow::Result<Vec<_>>>()?
                    .join(", ")
            )))
        }
        ConstraintType::Check | ConstraintType::Foreign | ConstraintType::Unknown => bail!(
            "oracle struct: unsupported constraint type {:?} for {}.{} (name={}, definition={})",
            constraint.constraint_type,
            schema,
            table,
            constraint.constraint_name,
            constraint.definition
        ),
    }
}

fn oracle_ident(raw: &str) -> anyhow::Result<String> {
    let ident = raw.trim().trim_matches('"').to_uppercase();
    if ident.is_empty() {
        bail!("oracle ident is empty");
    }
    if ident.len() > ORACLE_IDENT_MAX_LEN {
        bail!(
            "oracle ident too long (max={}): {}",
            ORACLE_IDENT_MAX_LEN,
            ident
        );
    }
    Ok(ident)
}

fn parse_constraint_cols(definition: &str) -> anyhow::Result<Vec<String>> {
    let def = definition.trim();
    let open = def.find('(').context("constraint definition missing '('")?;
    let close = def.rfind(')').context("constraint definition missing ')'")?;
    if close <= open + 1 {
        bail!("constraint definition has empty column list: {}", definition);
    }
    let inner = &def[open + 1..close];
    let cols = inner
        .split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    if cols.is_empty() {
        bail!("constraint definition has empty columns: {}", definition);
    }
    Ok(cols)
}

fn pg_type_to_oracle(pg_type: &str) -> anyhow::Result<String> {
    let t = pg_type.trim().to_lowercase();
    if t.contains("[]") {
        bail!("oracle struct: array types are not supported: {}", pg_type);
    }

    match t.as_str() {
        "integer" | "int" | "int4" => return Ok("NUMBER(10)".to_string()),
        "bigint" | "int8" => return Ok("NUMBER(19)".to_string()),
        "smallint" | "int2" => return Ok("NUMBER(5)".to_string()),
        "boolean" | "bool" => return Ok("NUMBER(1)".to_string()),
        "real" | "float4" => return Ok("BINARY_FLOAT".to_string()),
        "double precision" | "float8" => return Ok("BINARY_DOUBLE".to_string()),
        "text" => return Ok("CLOB".to_string()),
        "date" => return Ok("DATE".to_string()),
        "bytea" => return Ok("BLOB".to_string()),
        "json" | "jsonb" => return Ok("CLOB".to_string()),
        "timestamp without time zone" => return Ok("TIMESTAMP".to_string()),
        "timestamp with time zone" => return Ok("TIMESTAMP WITH TIME ZONE".to_string()),
        _ => {}
    };

    if let Some((prec, scale)) = parse_numeric(&t, "numeric").or_else(|| parse_numeric(&t, "decimal"))
    {
        return Ok(match (prec, scale) {
            (Some(p), Some(s)) => format!("NUMBER({},{})", p, s),
            (Some(p), None) => format!("NUMBER({})", p),
            _ => "NUMBER".to_string(),
        });
    }

    if let Some(len) = parse_len_type(&t, &["character varying", "varchar", "character"]) {
        return Ok(format!("VARCHAR2({})", len));
    }

    bail!("oracle struct: unsupported pg column type: {}", pg_type);
}

fn parse_numeric(s: &str, prefix: &str) -> Option<(Option<u32>, Option<u32>)> {
    let s = s.trim();
    if !s.starts_with(prefix) {
        return None;
    }
    if s == prefix {
        return Some((None, None));
    }
    let open = s.find('(')?;
    let close = s.rfind(')')?;
    if close <= open + 1 {
        return Some((None, None));
    }
    let inner = &s[open + 1..close];
    let mut parts = inner.split(',').map(|p| p.trim());
    let p = parts.next()?.parse::<u32>().ok();
    let sc = parts.next().and_then(|v| v.parse::<u32>().ok());
    Some((p, sc))
}

fn parse_len_type(s: &str, prefixes: &[&str]) -> Option<u32> {
    let s = s.trim();
    let prefix = prefixes.iter().find(|p| s.starts_with(**p))?;
    let rest = s.strip_prefix(prefix)?.trim();
    if !rest.starts_with('(') {
        return None;
    }
    let close = rest.find(')')?;
    rest[1..close].trim().parse::<u32>().ok()
}
