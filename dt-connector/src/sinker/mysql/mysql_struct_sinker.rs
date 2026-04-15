use std::sync::Arc;

use async_trait::async_trait;
use sqlx::{MySql, Pool};

use crate::{
    rdb_router::RdbRouter,
    sinker::base_struct_sinker::{BaseStructSinker, DBConnPool},
    Sinker,
};
use dt_common::{
    config::config_enums::ConflictPolicyEnum, meta::struct_meta::struct_data::StructData,
    monitor::monitor::Monitor, rdb_filter::RdbFilter,
};
use dt_common::meta::struct_meta::{
    statement::{
        mysql_create_database_statement::MysqlCreateDatabaseStatement,
        mysql_create_table_statement::MysqlCreateTableStatement,
        struct_statement::StructStatement,
    },
    structure::{column::Column, database::Database, table::Table},
};

use regex::Regex;

#[derive(Clone)]
pub struct MysqlStructSinker {
    pub conn_pool: Pool<MySql>,
    pub conflict_policy: ConflictPolicyEnum,
    pub filter: RdbFilter,
    pub router: RdbRouter,
    pub monitor: Arc<Monitor>,
    pub monitor_interval: u64,
}

#[async_trait]
impl Sinker for MysqlStructSinker {
    async fn sink_struct(&mut self, data: Vec<StructData>) -> anyhow::Result<()> {
        // `PgStructExtractor` produces Postgres-flavored struct statements. For MySQL as the
        // destination, translate the subset we can support into MySQL-flavored statements
        // before executing. Unsupported statement kinds are skipped.
        let mut converted = Vec::with_capacity(data.len());
        for struct_data in data.into_iter() {
            match struct_data.statement {
                StructStatement::MysqlCreateDatabase(_)
                | StructStatement::MysqlCreateTable(_) => converted.push(struct_data),

                StructStatement::PgCreateSchema(s) => {
                    converted.push(StructData {
                        schema: struct_data.schema,
                        statement: StructStatement::MysqlCreateDatabase(
                            MysqlCreateDatabaseStatement {
                                database: Database {
                                    name: s.schema.name,
                                    ..Default::default()
                                },
                            },
                        ),
                    });
                }

                StructStatement::PgCreateTable(s) => {
                    if let Some(mysql_stmt) = Self::try_convert_pg_table_to_mysql(s)? {
                        converted.push(StructData {
                            schema: struct_data.schema,
                            statement: StructStatement::MysqlCreateTable(mysql_stmt),
                        });
                    }
                }

                // MySQL doesn't support PG routines/views/RBAC in our struct pipeline today.
                // Keep the migration resilient by skipping them explicitly.
                _ => {}
            }
        }

        BaseStructSinker::sink_structs(
            &DBConnPool::MySQL(self.conn_pool.clone()),
            &self.conflict_policy,
            converted,
            &self.filter,
            &self.monitor,
            self.monitor_interval,
        )
        .await
    }

    async fn close(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

impl MysqlStructSinker {
    fn try_convert_pg_table_to_mysql(
        mut pg: dt_common::meta::struct_meta::statement::pg_create_table_statement::PgCreateTableStatement,
    ) -> anyhow::Result<Option<MysqlCreateTableStatement>> {
        // Only translate the core "table" structure for now; ignore sequences, indexes,
        // constraints (except primary key), and comments. This is sufficient for a basic
        // GaussDBPg -> MySQL bootstrap and can be expanded later.

        let pk_cols = Self::extract_pg_primary_key_cols(&pg.constraints);

        let mut table = Table::default();
        table.database_name = pg.table.schema_name.clone();
        table.table_name = pg.table.table_name.clone();
        table.columns = pg
            .table
            .columns
            .iter_mut()
            .map(|c| {
                let mut out = Column::default();
                out.column_name = c.column_name.clone();
                out.ordinal_position = c.ordinal_position;
                out.is_nullable = c.is_nullable;
                out.column_default = None;
                out.extra = String::new();
                out.column_comment = String::new();
                out.character_set_name = String::new();
                out.collation_name = String::new();

                out.column_type = Self::map_pg_col_type_to_mysql(&c.column_type);
                if pk_cols.contains(&c.column_name) {
                    out.column_key = "PRI".to_string();
                }
                out
            })
            .collect();

        Ok(Some(MysqlCreateTableStatement {
            table,
            constraints: Vec::new(),
            indexes: Vec::new(),
        }))
    }

    fn map_pg_col_type_to_mysql(pg_type: &str) -> String {
        let t = pg_type.trim().to_ascii_lowercase();

        // Common PG spellings.
        if t == "integer" || t == "int4" {
            return "int".to_string();
        }
        if t == "bigint" || t == "int8" {
            return "bigint".to_string();
        }
        if t == "smallint" || t == "int2" {
            return "smallint".to_string();
        }
        if t == "boolean" || t == "bool" {
            return "boolean".to_string();
        }
        if t == "text" {
            return "text".to_string();
        }
        if t == "bytea" {
            return "blob".to_string();
        }

        // varchar/character varying(n)
        if let Some(size) = Self::extract_pg_varchar_size(&t) {
            return format!("varchar({})", size);
        }

        // numeric(p,s)
        if let Some((p, s)) = Self::extract_pg_numeric_ps(&t) {
            return format!("decimal({},{})", p, s);
        }

        if t == "double precision" {
            return "double".to_string();
        }
        if t == "real" {
            return "float".to_string();
        }
        if t == "date" {
            return "date".to_string();
        }
        if t == "timestamp without time zone" || t == "timestamp" {
            return "datetime".to_string();
        }

        // Fallback: keep the raw type string; MySQL may reject it but this keeps
        // the behavior explicit and debuggable.
        pg_type.trim().to_string()
    }

    fn extract_pg_varchar_size(t: &str) -> Option<u32> {
        // Examples:
        // - "character varying(10)"
        // - "varchar(10)"
        // - "character varying"
        static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        let re = RE.get_or_init(|| Regex::new(r"^(?:character varying|varchar)\((\d+)\)$").unwrap());
        if let Some(caps) = re.captures(t) {
            return caps.get(1)?.as_str().parse::<u32>().ok();
        }
        None
    }

    fn extract_pg_numeric_ps(t: &str) -> Option<(u32, u32)> {
        static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        let re = RE.get_or_init(|| Regex::new(r"^numeric\((\d+),\s*(\d+)\)$").unwrap());
        let caps = re.captures(t)?;
        let p = caps.get(1)?.as_str().parse::<u32>().ok()?;
        let s = caps.get(2)?.as_str().parse::<u32>().ok()?;
        Some((p, s))
    }

    fn extract_pg_primary_key_cols(
        constraints: &[dt_common::meta::struct_meta::structure::constraint::Constraint],
    ) -> std::collections::HashSet<String> {
        use dt_common::meta::struct_meta::structure::constraint::ConstraintType;

        static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        let re = RE.get_or_init(|| Regex::new(r"(?i)primary\s+key\s*\(([^)]+)\)").unwrap());

        let mut out = std::collections::HashSet::new();
        for c in constraints.iter() {
            if c.constraint_type != ConstraintType::Primary {
                continue;
            }
            if let Some(caps) = re.captures(&c.definition) {
                if let Some(cols) = caps.get(1).map(|m| m.as_str()) {
                    for col in cols.split(',') {
                        let col = col.trim().trim_matches('"').trim_matches('`');
                        if !col.is_empty() {
                            out.insert(col.to_string());
                        }
                    }
                }
            }
        }
        out
    }
}
