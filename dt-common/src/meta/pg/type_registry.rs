use futures::TryStreamExt;
use sqlx::postgres::types::Oid as PgOid;
use sqlx::{postgres::PgRow, Pool, Postgres, Row};
use std::collections::HashMap;

use super::{pg_col_type::PgColType, pg_value_type::PgValueType};

#[derive(Clone)]
pub struct TypeRegistry {
    pub conn_pool: Pool<Postgres>,
    pub oid_to_type: HashMap<i32, PgColType>,
}

impl TypeRegistry {
    pub fn new(conn_pool: Pool<Postgres>) -> Self {
        Self {
            conn_pool,
            oid_to_type: HashMap::new(),
        }
    }

    pub async fn init(mut self) -> anyhow::Result<Self> {
        // TODO check duplicate typename in pg_catalog.pg_type
        let sql = "SELECT t.oid AS oid,
                    t.typname AS name,
                    t.typelem AS element,
                    t.typbasetype AS parentoid,
                    t.typtypmod AS modifiers,
                    t.typcategory AS category,
                    e.values AS enum_values,
                    n.nspname AS schema_name
            FROM pg_catalog.pg_type t
            JOIN pg_catalog.pg_namespace n
            ON (t.typnamespace = n.oid)
            LEFT JOIN 
            (SELECT t.enumtypid AS id, array_agg(t.enumlabel) AS values
            FROM pg_catalog.pg_enum t
            GROUP BY id) e
            ON (t.oid = e.id)
            WHERE n.nspname != 'pg_toast'";
        let mut rows = sqlx::query(sql).fetch(&self.conn_pool);
        while let Some(row) = rows.try_next().await? {
            let col_type = self.parse_col_meta(&row)?;
            self.oid_to_type.insert(col_type.oid, col_type.clone());
        }
        Ok(self)
    }

    fn parse_col_meta(&mut self, row: &PgRow) -> anyhow::Result<PgColType> {
        let oid: i32 = row.try_get::<PgOid, _>("oid")?.0 as i32;
        let value_type = PgValueType::from_oid(oid);
        let name: String = row.try_get("name")?;
        let alias = Self::name_to_alias(&name);
        let element_oid: i32 = row.try_get::<PgOid, _>("element")?.0 as i32;
        let parent_oid: i32 = row.try_get::<PgOid, _>("parentoid")?.0 as i32;
        let category: i8 = row.try_get("category")?;
        let category = (category as u8 as char).to_string();
        let enum_values: Option<Vec<String>> = row.try_get("enum_values")?;
        let schema_name: String = row.try_get("schema_name")?;

        Ok(PgColType {
            oid,
            value_type,
            name,
            alias,
            element_oid,
            parent_oid,
            category,
            enum_values,
            schema_name,
        })
    }

    fn name_to_alias(name: &str) -> String {
        // refer to: https://www.postgresql.org/docs/17/datatype.html
        match name {
            "bigint" => "int8",
            "bigserial" => "serial8",
            "bit varying" => "varbit",
            "boolean" => "bool",
            "char" => "\"char\"",
            // fixed-length, blank-padded, refer to: https://www.postgresql.org/docs/17/datatype-character.html
            "character" => "bpchar",
            "character varying" => "varchar",
            "double precision" => "float8",
            "int" | "integer" => "int4",
            "decimal" => "numeric",
            "real" => "float4",
            "smallint" => "int2",
            "smallserial" => "serial2",
            "serial" => "serial4",
            "timestamp with time zone" => "timestamptz",
            "timestamp without time zone" => "timestamp",
            "time without time zone" => "time",
            "time with time zone" => "timetz",
            _ => name,
        }
        .to_string()
    }
}
