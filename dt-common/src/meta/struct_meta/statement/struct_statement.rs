use crate::rdb_filter::RdbFilter;

use super::{
    mysql_create_database_statement::MysqlCreateDatabaseStatement,
    mysql_create_table_statement::MysqlCreateTableStatement,
    pg_create_rbac_statement::PgCreateRbacStatement,
    pg_create_routine_statement::PgCreateRoutineStatement,
    pg_create_schema_statement::PgCreateSchemaStatement,
    pg_create_table_statement::PgCreateTableStatement,
    pg_create_view_statement::PgCreateViewStatement,
};

#[derive(Debug, Clone, Default)]
pub enum StructStatement {
    MysqlCreateDatabase(MysqlCreateDatabaseStatement),
    PgCreateSchema(PgCreateSchemaStatement),
    MysqlCreateTable(MysqlCreateTableStatement),
    PgCreateTable(PgCreateTableStatement),
    PgCreateRoutine(PgCreateRoutineStatement),
    PgCreateView(PgCreateViewStatement),
    PgCreateRbac(PgCreateRbacStatement),
    #[default]
    Unknown,
}

impl StructStatement {
    pub fn to_sqls(&mut self, filter: &RdbFilter) -> anyhow::Result<Vec<(String, String)>> {
        match self {
            Self::MysqlCreateDatabase(s) => s.to_sqls(filter),
            Self::PgCreateSchema(s) => s.to_sqls(filter),
            Self::MysqlCreateTable(s) => s.to_sqls(filter),
            Self::PgCreateTable(s) => s.to_sqls(filter),
            Self::PgCreateRoutine(s) => s.to_sqls(filter),
            Self::PgCreateView(s) => s.to_sqls(filter),
            Self::PgCreateRbac(s) => s.to_sqls(filter),
            _ => Ok(vec![]),
        }
    }
}
