use serde::{Deserialize, Serialize};
use strum::{Display, EnumString, IntoStaticStr};
use url::Url;

#[derive(
    Clone,
    Display,
    EnumString,
    IntoStaticStr,
    Debug,
    PartialEq,
    Eq,
    Default,
    Serialize,
    Deserialize,
    Hash,
)]
pub enum DbType {
    #[default]
    #[strum(serialize = "mysql")]
    Mysql,
    #[strum(serialize = "pg")]
    Pg,
    #[strum(serialize = "gaussdb_pg")]
    GaussDBPg,
    #[strum(serialize = "gaussdb_mysql")]
    GaussDBMySQL,
    #[strum(serialize = "kafka")]
    Kafka,
    #[strum(serialize = "mongo")]
    Mongo,
    #[strum(serialize = "redis")]
    Redis,
    #[strum(serialize = "clickhouse")]
    ClickHouse,
    #[strum(serialize = "starrocks")]
    StarRocks,
    #[strum(serialize = "doris")]
    Doris,
    #[strum(serialize = "foxlake")]
    Foxlake,
    #[strum(serialize = "tidb")]
    Tidb,
}

#[derive(
    Clone,
    Display,
    EnumString,
    IntoStaticStr,
    Debug,
    PartialEq,
    Eq,
    Default,
    Serialize,
    Deserialize,
    Hash,
)]
pub enum WireProtocol {
    #[strum(serialize = "mysql")]
    Mysql,
    #[default]
    #[strum(serialize = "postgresql", serialize = "postgres", serialize = "pg")]
    PostgreSQL,
}

impl WireProtocol {
    pub fn from_url(url: &str) -> Option<Self> {
        let parsed = Url::parse(url).ok()?;
        match parsed.scheme() {
            "mysql" => Some(Self::Mysql),
            "postgres" | "postgresql" => Some(Self::PostgreSQL),
            _ => None,
        }
    }
}

#[derive(
    Clone, Display, EnumString, IntoStaticStr, Debug, PartialEq, Eq, Serialize, Deserialize, Hash,
)]
pub enum GaussDBSqlCompatibility {
    #[strum(serialize = "P")]
    PostgreSQL,
    #[strum(serialize = "M")]
    MySQL,
    #[strum(serialize = "A")]
    Oracle,
}

impl GaussDBSqlCompatibility {
    pub fn from_show_value(value: &str) -> Option<Self> {
        match value.trim() {
            "P" => Some(Self::PostgreSQL),
            "M" => Some(Self::MySQL),
            "A" => Some(Self::Oracle),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{DbType, GaussDBSqlCompatibility, WireProtocol};

    #[test]
    fn test_db_type_gaussdb_pg_parse_and_display() {
        let t = DbType::from_str("gaussdb_pg").unwrap();
        assert_eq!(t, DbType::GaussDBPg);
        assert_eq!(t.to_string(), "gaussdb_pg");
    }

    #[test]
    fn test_db_type_gaussdb_mysql_parse_and_display() {
        let t = DbType::from_str("gaussdb_mysql").unwrap();
        assert_eq!(t, DbType::GaussDBMySQL);
        assert_eq!(t.to_string(), "gaussdb_mysql");
    }

    #[test]
    fn test_gaussdb_protocol_from_postgres_url_for_mysql_mode_db() {
        let url = "postgres://root@10.250.0.51:8000/jyp_test_m?sslmode=require";
        assert_eq!(WireProtocol::from_url(url), Some(WireProtocol::PostgreSQL));
    }

    #[test]
    fn test_gaussdb_protocol_from_mysql_url() {
        let url = "mysql://127.0.0.1:3311?ssl-mode=disabled";
        assert_eq!(WireProtocol::from_url(url), Some(WireProtocol::Mysql));
    }

    #[test]
    fn test_gaussdb_sql_compatibility_from_show_value() {
        assert_eq!(
            GaussDBSqlCompatibility::from_show_value("M"),
            Some(GaussDBSqlCompatibility::MySQL)
        );
        assert_eq!(
            GaussDBSqlCompatibility::from_show_value("P"),
            Some(GaussDBSqlCompatibility::PostgreSQL)
        );
        assert_eq!(
            GaussDBSqlCompatibility::from_show_value("A"),
            Some(GaussDBSqlCompatibility::Oracle)
        );
    }
}

#[derive(Display, EnumString, IntoStaticStr, Debug, Clone, Hash)]
pub enum ExtractType {
    #[strum(serialize = "snapshot")]
    Snapshot,
    #[strum(serialize = "cdc")]
    Cdc,
    #[strum(serialize = "snapshot_and_cdc")]
    SnapshotAndCdc,
    #[strum(serialize = "check_log")]
    CheckLog,
    #[strum(serialize = "struct")]
    Struct,
    #[strum(serialize = "snapshot_file")]
    SnapshotFile,
    #[strum(serialize = "scan")]
    Scan,
    #[strum(serialize = "reshard")]
    Reshard,
    #[strum(serialize = "foxlake_s3")]
    FoxlakeS3,
}

#[derive(Display, EnumString, IntoStaticStr, Clone, Debug, Default, Hash)]
pub enum SinkType {
    #[default]
    #[strum(serialize = "dummy")]
    Dummy,
    #[strum(serialize = "write")]
    Write,
    #[strum(serialize = "check")]
    Check,
    #[strum(serialize = "struct")]
    Struct,
    #[strum(serialize = "statistic")]
    Statistic,
    #[strum(serialize = "sql")]
    Sql,
    #[strum(serialize = "push")]
    Push,
    #[strum(serialize = "merge")]
    Merge,
}

#[derive(EnumString, IntoStaticStr, Clone, Display)]
pub enum ParallelType {
    #[strum(serialize = "serial")]
    Serial,
    #[strum(serialize = "snapshot")]
    Snapshot,
    #[strum(serialize = "rdb_partition")]
    RdbPartition,
    #[strum(serialize = "rdb_merge")]
    RdbMerge,
    #[strum(serialize = "rdb_check")]
    RdbCheck,
    #[strum(serialize = "table")]
    Table,
    #[strum(serialize = "mongo")]
    Mongo,
    #[strum(serialize = "redis")]
    Redis,
    #[strum(serialize = "foxlake")]
    Foxlake,
}

#[derive(EnumString, IntoStaticStr, Clone, Display)]
pub enum PipelineType {
    #[strum(serialize = "basic")]
    Basic,
    #[strum(serialize = "http_server")]
    HttpServer,
}

#[derive(Clone, Debug, EnumString, IntoStaticStr, PartialEq, Default)]
pub enum ConflictPolicyEnum {
    #[strum(serialize = "ignore")]
    Ignore,
    #[default]
    #[strum(serialize = "interrupt")]
    Interrupt,
}

#[derive(Display, EnumString, IntoStaticStr, PartialEq)]
pub enum MetaCenterType {
    #[strum(serialize = "basic")]
    Basic,
    #[strum(serialize = "dbengine")]
    DbEngine,
}

#[derive(Debug, Clone, PartialEq, Eq, Display, EnumString, IntoStaticStr)]
pub enum TaskType {
    #[strum(serialize = "struct")]
    Struct,
    #[strum(serialize = "snapshot")]
    Snapshot,
    #[strum(serialize = "cdc")]
    Cdc,
    #[strum(serialize = "check")]
    Check,
}

#[derive(Display, EnumString, IntoStaticStr, PartialEq, Default)]
pub enum ResumeType {
    #[strum(serialize = "from_log")]
    FromLog,
    #[strum(serialize = "from_target")]
    FromTarget,
    #[strum(serialize = "from_db")]
    FromDB,
    #[default]
    #[strum(serialize = "dummy")]
    Dummy,
}

#[derive(Display, EnumString, IntoStaticStr, PartialEq, Default, Clone, Debug)]
pub enum RdbTransactionIsolation {
    #[strum(serialize = "read_uncommitted")]
    ReadUncommitted,
    #[strum(serialize = "read_committed")]
    ReadCommitted,
    #[strum(serialize = "repeatable_read")]
    RepeatableRead,
    #[strum(serialize = "serializable")]
    Serializable,
    #[default]
    #[strum(serialize = "default")]
    Default,
}

pub fn build_task_type(extract_type: &ExtractType, sink_type: &SinkType) -> Option<TaskType> {
    match (extract_type, sink_type) {
        (ExtractType::Struct, SinkType::Struct) => Some(TaskType::Struct),
        (ExtractType::Snapshot, SinkType::Write) => Some(TaskType::Snapshot),
        (ExtractType::Cdc, SinkType::Write) => Some(TaskType::Cdc),
        (ExtractType::Snapshot, SinkType::Check) => Some(TaskType::Check),
        _ => None,
    }
}
