use crate::config::config_enums::DbType;

pub struct SystemDb {}

impl SystemDb {
    const MYSQL: [&str; 4] = ["information_schema", "mysql", "performance_schema", "sys"];
    const POSTGRES: [&str; 2] = ["pg_catalog", "information_schema"];
    // Keep this list minimal and conservative to avoid hiding user schemas.
    // Add GaussDB-specific system schemas as needed.
    const GAUSSDB_PG: [&str; 6] = [
        "pg_catalog",
        "information_schema",
        "dbe_perf",
        "dbe_pldebugger",
        "db4ai",
        "cstore",
    ];
    const MONGO: [&str; 3] = ["admin", "config", "local"];

    pub fn is_system_db(db: &str, db_type: &DbType) -> bool {
        match db_type {
            DbType::Mysql | DbType::GaussDBMySQL => Self::MYSQL.contains(&db),
            DbType::Pg => Self::POSTGRES.contains(&db),
            DbType::GaussDBPg => Self::GAUSSDB_PG.contains(&db),
            DbType::Mongo => Self::MONGO.contains(&db),
            _ => false,
        }
    }

    pub fn get_system_dbs(db_type: &DbType) -> Option<Vec<&str>> {
        match db_type {
            DbType::Mysql | DbType::GaussDBMySQL => Some(Self::MYSQL.to_vec()),
            DbType::Pg => Some(Self::POSTGRES.to_vec()),
            DbType::GaussDBPg => Some(Self::GAUSSDB_PG.to_vec()),
            DbType::Mongo => Some(Self::MONGO.to_vec()),
            _ => None,
        }
    }
}
