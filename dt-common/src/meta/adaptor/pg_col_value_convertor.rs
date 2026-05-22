use bytes::Bytes;
use sqlx::{postgres::PgRow, Row, ValueRef};

use crate::meta::{
    col_value::ColValue,
    pg::{pg_col_type::PgColType, pg_meta_manager::PgMetaManager, pg_value_type::PgValueType},
};

pub struct PgColValueConvertor {}

impl PgColValueConvertor {
    pub fn get_extract_type(col_type: &PgColType) -> String {
        if col_type.alias == "oid" {
            "int8".to_string()
        } else if col_type.name == "blob" {
            "text".to_string()
        } else {
            match col_type.value_type {
                PgValueType::Bytes
                | PgValueType::Boolean
                | PgValueType::Int16
                | PgValueType::Int32
                | PgValueType::Int64
                | PgValueType::Float32
                | PgValueType::Float64 => col_type.alias.to_string(),
                _ => "text".to_string(),
            }
        }
    }

    pub fn from_str(
        col_type: &PgColType,
        value_str: &str,
        meta_manager: &mut PgMetaManager,
    ) -> anyhow::Result<ColValue> {
        if value_str.is_empty() {
            return Ok(ColValue::None);
        }

        if col_type.parent_oid != 0 {
            let parent_col_type = meta_manager.get_col_type_by_oid(col_type.parent_oid)?;
            return Self::from_str(&parent_col_type, value_str, meta_manager);
        }

        let mut value_str = value_str.to_string();
        if col_type.is_array() {
            return Ok(ColValue::String(value_str));
        }

        let col_value = match col_type.value_type {
            PgValueType::Boolean => ColValue::Bool("t" == value_str.to_lowercase()),

            PgValueType::Int32 => {
                let res: i32 = value_str.parse()?;
                ColValue::Long(res)
            }

            PgValueType::Int16 => {
                let value: i16 = value_str.parse()?;
                ColValue::Short(value)
            }

            PgValueType::Int64 => {
                let value: i64 = value_str.parse()?;
                ColValue::LongLong(value)
            }

            PgValueType::Float32 => {
                let value: f32 = value_str.parse()?;
                ColValue::Float(value)
            }

            PgValueType::Float64 => {
                let value: f64 = value_str.parse()?;
                ColValue::Double(value)
            }

            PgValueType::Bytes => {
                // value_str == "\x000102"
                if value_str.starts_with(r#"\x"#) {
                    let bytes = hex::decode(value_str.trim_start_matches(r#"\x"#))?;
                    ColValue::Blob(bytes)
                } else {
                    ColValue::Blob(hex::decode(value_str)?)
                }
            }

            PgValueType::Numeric => ColValue::Decimal(value_str),

            PgValueType::TimestampTZ => ColValue::Timestamp(value_str),

            PgValueType::Timestamp => ColValue::DateTime(value_str),

            PgValueType::Time => ColValue::Time(value_str),

            PgValueType::TimeTZ => ColValue::String(value_str),

            PgValueType::Date => ColValue::String(value_str),

            PgValueType::JSON => ColValue::Json2(value_str),

            _ => {
                // bpchar: fixed-length, blank-padded
                // In wal log, if a column type is char(10), column value is 'aaa',
                // the value_str will be 'aaa      ' which is blank-padded.
                if col_type.alias == "bpchar" {
                    value_str = value_str.trim_end().into();
                }
                ColValue::String(value_str)
            }
        };
        Ok(col_value)
    }

    pub fn from_wal(
        col_type: &PgColType,
        value: &Bytes,
        meta_manager: &mut PgMetaManager,
    ) -> anyhow::Result<ColValue> {
        // include all types from https://www.postgresql.org/docs/current/static/datatype.html#DATATYPE-TABLE
        // plus aliases from the shorter names produced by older wal2json
        // let value = value.unwrap();
        let value_str = std::str::from_utf8(value)?;
        Self::from_str(col_type, value_str, meta_manager)
    }

    pub fn from_query(row: &PgRow, col: &str, col_type: &PgColType) -> anyhow::Result<ColValue> {
        let value = row.try_get_raw(col)?;
        if value.is_null() {
            return Ok(ColValue::None);
        }

        if col_type.is_array() {
            let value: String = row.try_get(col)?;
            return Ok(ColValue::String(value));
        }

        let col_value = match col_type.value_type {
            PgValueType::Boolean => {
                let value: bool = row.try_get(col)?;
                ColValue::Bool(value)
            }

            PgValueType::Int32 => {
                let value: i32 = row.try_get(col)?;
                ColValue::Long(value)
            }

            PgValueType::Int16 => {
                let value: i16 = row.try_get(col)?;
                ColValue::Short(value)
            }

            PgValueType::Int64 => {
                let value: i64 = row.try_get(col)?;
                ColValue::LongLong(value)
            }

            PgValueType::Float32 => {
                let value: f32 = row.try_get(col)?;
                ColValue::Float(value)
            }

            PgValueType::Float64 => {
                let value: f64 = row.try_get(col)?;
                ColValue::Double(value)
            }

            PgValueType::Bytes => {
                if col_type.name == "blob" {
                    let value: String = row.try_get(col)?;
                    ColValue::Blob(hex::decode(value)?)
                } else {
                    let value: Vec<u8> = row.try_get(col)?;
                    ColValue::Blob(value)
                }
            }

            PgValueType::Numeric => {
                let value: String = row.try_get(col)?;
                ColValue::Decimal(value)
            }

            PgValueType::TimestampTZ => {
                let value: String = row.try_get(col)?;
                ColValue::Timestamp(value)
            }

            PgValueType::Timestamp => {
                let value: String = row.try_get(col)?;
                ColValue::DateTime(value)
            }

            PgValueType::Time => {
                let value: String = row.try_get(col)?;
                ColValue::Time(value)
            }

            PgValueType::TimeTZ => {
                let value: String = row.try_get(col)?;
                ColValue::String(value)
            }

            PgValueType::Date => {
                let value: String = row.try_get(col)?;
                ColValue::String(value)
            }

            PgValueType::JSON => {
                let value: String = row.try_get(col)?;
                ColValue::Json2(value)
            }

            _ => {
                let value: String = row.try_get(col)?;
                ColValue::String(value)
            }
        };
        Ok(col_value)
    }
}

#[cfg(test)]
mod tests {
    use super::PgColValueConvertor;
    use crate::meta::{
        adaptor::pg_col_value_convertor::PgColValueConvertor as Convertor,
        col_value::ColValue,
        pg::{
            pg_col_type::PgColType, pg_meta_manager::PgMetaManager, pg_value_type::PgValueType,
            type_registry::TypeRegistry,
        },
    };
    use sqlx::postgres::PgPoolOptions;
    use std::collections::HashMap;

    fn dummy_meta_manager() -> PgMetaManager {
        let conn_pool = PgPoolOptions::new()
            .connect_lazy("postgres://localhost/dummy")
            .expect("lazy pg pool");
        PgMetaManager {
            conn_pool: conn_pool.clone(),
            type_registry: TypeRegistry::new(conn_pool),
            name_to_tb_meta: HashMap::new(),
            oid_to_tb_meta: HashMap::new(),
        }
    }

    fn make_col_type(name: &str, alias: &str, value_type: PgValueType) -> PgColType {
        PgColType {
            value_type,
            name: name.to_string(),
            alias: alias.to_string(),
            oid: 0,
            parent_oid: 0,
            element_oid: 0,
            category: String::new(),
            enum_values: None,
            schema_name: "public".to_string(),
        }
    }

    #[test]
    fn gaussdb_type_matrix_convertor_maps_datetime_and_integer_aliases() {
        let mut meta_manager = dummy_meta_manager();

        let smalldatetime = make_col_type("smalldatetime", "timestamp", PgValueType::Timestamp);
        let tinyint = make_col_type("tinyint", "int2", PgValueType::Int16);

        assert_eq!(
            PgColValueConvertor::from_str(&smalldatetime, "2026-04-02 13:20:00", &mut meta_manager)
                .unwrap(),
            ColValue::DateTime("2026-04-02 13:20:00".to_string())
        );
        assert_eq!(
            Convertor::from_str(&tinyint, "7", &mut meta_manager).unwrap(),
            ColValue::Short(7)
        );
    }

    #[test]
    fn gaussdb_type_matrix_convertor_maps_string_and_blob_aliases() {
        let mut meta_manager = dummy_meta_manager();

        let nvarchar2 = make_col_type("nvarchar2", "varchar", PgValueType::String);
        let clob = make_col_type("clob", "text", PgValueType::String);
        let blob = make_col_type("blob", "bytea", PgValueType::Bytes);

        assert_eq!(
            Convertor::from_str(&nvarchar2, "hello", &mut meta_manager).unwrap(),
            ColValue::String("hello".to_string())
        );
        assert_eq!(
            Convertor::from_str(&clob, "long text", &mut meta_manager).unwrap(),
            ColValue::String("long text".to_string())
        );
        assert_eq!(
            Convertor::from_str(&blob, r"\x0001ff", &mut meta_manager).unwrap(),
            ColValue::Blob(vec![0x00, 0x01, 0xff])
        );
    }

    #[test]
    fn gaussdb_type_matrix_extract_type_uses_text_for_blob() {
        let blob = make_col_type("blob", "bytea", PgValueType::Bytes);
        assert_eq!(PgColValueConvertor::get_extract_type(&blob), "text");
    }
}
