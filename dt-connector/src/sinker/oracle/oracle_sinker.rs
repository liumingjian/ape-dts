use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{bail, Context};
use async_trait::async_trait;

use crate::oracle::OracleSqlPlusClient;
use crate::sinker::base_sinker::BaseSinker;
use crate::sinker::oracle::oracle_name::{oracle_ident, oracle_table_ref};
use crate::Sinker;
use dt_common::log_info;
use dt_common::meta::{
    col_value::ColValue, ddl_meta::ddl_data::DdlData, row_data::RowData, row_type::RowType,
};
use dt_common::monitor::monitor::Monitor;

#[derive(Clone)]
pub struct OracleSinker {
    pub client: OracleSqlPlusClient,
    pub batch_size: usize,
    pub replace: bool,
    pub key_cols: Vec<String>,
    pub monitor: Arc<Monitor>,
    pub monitor_interval: u64,
}

#[async_trait]
impl Sinker for OracleSinker {
    async fn sink_dml(&mut self, data: Vec<RowData>, _batch: bool) -> anyhow::Result<()> {
        if data.is_empty() {
            return Ok(());
        }

        let mut sqls = Vec::with_capacity(data.len() + 1);
        let mut data_size = 0u64;
        for row in &data {
            data_size += row.data_size as u64;
            let sql = match row.row_type {
                RowType::Insert => Self::build_insert_sql(row, self.replace, &self.key_cols)?,
                RowType::Update => Self::build_update_sql(row)?,
                RowType::Delete => Self::build_delete_sql(row)?,
            };
            sqls.push(sql);
        }
        sqls.push("COMMIT".to_string());

        let script = sqls.join(";\n");
        self.client.exec(&script).await?;

        BaseSinker::update_batch_monitor(&self.monitor, data.len() as u64, data_size).await?;
        Ok(())
    }

    async fn sink_ddl(&mut self, data: Vec<DdlData>, _batch: bool) -> anyhow::Result<()> {
        for ddl in data {
            let sql = ddl.to_sql();
            log_info!("oracle sink ddl: {}", sql);
            self.client.exec(&sql).await?;
        }
        Ok(())
    }

    async fn refresh_meta(&mut self, _data: Vec<DdlData>) -> anyhow::Result<()> {
        Ok(())
    }

    async fn close(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

impl OracleSinker {
    pub(crate) fn build_insert_sql(
        row: &RowData,
        replace: bool,
        key_cols: &[String],
    ) -> anyhow::Result<String> {
        let after = row.require_after()?;
        let mut cols = after.keys().cloned().collect::<Vec<_>>();
        cols.sort();

        let mut values = Vec::with_capacity(cols.len());
        for col in &cols {
            let v = after
                .get(col)
                .with_context(|| format!("missing col {} in oracle row_data.after", col))?;
            values.push(Self::to_oracle_literal(v)?);
        }

        let insert_sql = format!(
            "INSERT INTO {} ({}) VALUES ({})",
            Self::table_ref(row)?,
            Self::column_list(&cols)?,
            values.join(",")
        );

        if !replace {
            return Ok(insert_sql);
        }

        let delete_sql = Self::build_replace_delete_sql(row, key_cols)?;
        Ok(format!("{};\n{}", delete_sql, insert_sql))
    }

    pub(crate) fn build_update_sql(row: &RowData) -> anyhow::Result<String> {
        let before = row.require_before()?;
        let after = row.require_after()?;

        if before.is_empty() {
            bail!("oracle update requires non-empty row_data.before for WHERE clause");
        }
        if after.is_empty() {
            bail!("oracle update requires non-empty row_data.after for SET clause");
        }

        let mut set_cols = after.keys().cloned().collect::<Vec<_>>();
        set_cols.sort();
        let mut set_pairs = Vec::with_capacity(set_cols.len());
        for col in &set_cols {
            let v = after
                .get(col)
                .with_context(|| format!("missing col {} in oracle row_data.after", col))?;
            set_pairs.push(format!(
                "{}={}",
                oracle_ident(col)?,
                Self::to_oracle_literal(v)?
            ));
        }

        let where_sql = Self::build_where_sql(before)?;

        Ok(format!(
            "UPDATE {} SET {} WHERE {}",
            Self::table_ref(row)?,
            set_pairs.join(","),
            where_sql
        ))
    }

    pub(crate) fn build_delete_sql(row: &RowData) -> anyhow::Result<String> {
        let before = row.require_before()?;
        if before.is_empty() {
            bail!("oracle delete requires non-empty row_data.before for WHERE clause");
        }

        let where_sql = Self::build_where_sql(before)?;
        Ok(format!(
            "DELETE FROM {} WHERE {}",
            Self::table_ref(row)?,
            where_sql
        ))
    }

    pub(crate) fn build_where_sql(
        before: &std::collections::HashMap<String, ColValue>,
    ) -> anyhow::Result<String> {
        let mut cols = before.keys().cloned().collect::<Vec<_>>();
        cols.sort();
        let mut clauses = Vec::with_capacity(cols.len());
        for col in &cols {
            let v = before
                .get(col)
                .with_context(|| format!("missing col {} in oracle row_data.before", col))?;
            if matches!(v, ColValue::None) {
                clauses.push(format!("{} IS NULL", oracle_ident(col)?));
            } else {
                clauses.push(format!(
                    "{}={}",
                    oracle_ident(col)?,
                    Self::to_oracle_literal(v)?
                ));
            }
        }
        Ok(clauses.join(" AND "))
    }

    pub(crate) fn column_list(cols: &[String]) -> anyhow::Result<String> {
        cols.iter()
            .map(|col| oracle_ident(col))
            .collect::<anyhow::Result<Vec<_>>>()
            .map(|cols| cols.join(","))
    }

    fn table_ref(row: &RowData) -> anyhow::Result<String> {
        oracle_table_ref(&row.schema, &row.tb)
    }

    fn build_replace_delete_sql(row: &RowData, key_cols: &[String]) -> anyhow::Result<String> {
        let after = row.require_after()?;
        if after.is_empty() {
            bail!("oracle replace requires non-empty row_data.after");
        }
        let key_values = Self::replace_key_values(row, key_cols)?;
        let where_sql = Self::build_where_sql(&key_values)?;
        Ok(format!(
            "DELETE FROM {} WHERE {}",
            Self::table_ref(row)?,
            where_sql
        ))
    }

    fn replace_key_values(
        row: &RowData,
        key_cols: &[String],
    ) -> anyhow::Result<HashMap<String, ColValue>> {
        if key_cols.is_empty() {
            bail!("oracle replace requires non-empty key columns");
        }

        let after = row.require_after()?;
        let mut key_values = HashMap::new();
        for key_col in key_cols {
            let key_value = after
                .get(key_col)
                .with_context(|| format!("oracle replace missing key column {}", key_col))?;
            if matches!(key_value, ColValue::None) {
                bail!("oracle replace key column {} is NULL", key_col);
            }
            key_values.insert(key_col.clone(), key_value.clone());
        }
        Ok(key_values)
    }

    pub(crate) fn escape_str(s: &str) -> String {
        s.replace('\'', "''")
    }

    pub(crate) fn to_oracle_literal(v: &ColValue) -> anyhow::Result<String> {
        Ok(match v {
            ColValue::None => "NULL".to_string(),
            ColValue::Bool(b) => {
                if *b {
                    "1".to_string()
                } else {
                    "0".to_string()
                }
            }
            ColValue::Tiny(n) => n.to_string(),
            ColValue::UnsignedTiny(n) => n.to_string(),
            ColValue::Short(n) => n.to_string(),
            ColValue::UnsignedShort(n) => n.to_string(),
            ColValue::Long(n) => n.to_string(),
            ColValue::UnsignedLong(n) => n.to_string(),
            ColValue::LongLong(n) => n.to_string(),
            ColValue::UnsignedLongLong(n) => n.to_string(),
            ColValue::Float(n) => n.to_string(),
            ColValue::Double(n) => n.to_string(),
            ColValue::Decimal(s) => s.to_string(),
            ColValue::String(s) => format!("'{}'", Self::escape_str(s)),
            ColValue::Time(s)
            | ColValue::Date(s)
            | ColValue::DateTime(s)
            | ColValue::Timestamp(s) => format!("'{}'", Self::escape_str(s)),
            ColValue::RawString(bytes) => format!("'{}'", hex::encode(bytes)),
            ColValue::Blob(bytes) => format!("hextoraw('{}')", hex::encode(bytes)),
            ColValue::Bit(n) => n.to_string(),
            ColValue::Set(n) => n.to_string(),
            ColValue::Enum(n) => n.to_string(),
            ColValue::Set2(s) | ColValue::Enum2(s) | ColValue::Json2(s) => {
                format!("'{}'", Self::escape_str(s))
            }
            ColValue::Json(bytes) => format!("'{}'", Self::escape_str(&hex::encode(bytes))),
            ColValue::Json3(v) => format!("'{}'", Self::escape_str(&v.to_string())),
            ColValue::MongoDoc(doc) => format!("'{}'", Self::escape_str(&doc.to_string())),
            ColValue::Year(y) => y.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn build_update_sql_escapes_strings_and_sorts_cols() {
        let mut before = HashMap::new();
        before.insert("ID".to_string(), ColValue::Long(1));

        let mut after = HashMap::new();
        after.insert("VAL".to_string(), ColValue::String("O'Reilly".to_string()));
        after.insert("ID".to_string(), ColValue::Long(1));

        let row = RowData::new(
            "APE_DTS".to_string(),
            "GDBO_ORA_CDC_BASIC".to_string(),
            RowType::Update,
            Some(before),
            Some(after),
        );

        let sql = OracleSinker::build_update_sql(&row).unwrap();
        assert_eq!(
            sql,
            "UPDATE APE_DTS.GDBO_ORA_CDC_BASIC SET ID=1,VAL='O''Reilly' WHERE ID=1"
        );
    }

    #[test]
    fn build_delete_sql_uses_is_null_for_none() {
        let mut before = HashMap::new();
        before.insert("ID".to_string(), ColValue::None);

        let row = RowData::new(
            "APE_DTS".to_string(),
            "T".to_string(),
            RowType::Delete,
            Some(before),
            None,
        );

        let sql = OracleSinker::build_delete_sql(&row).unwrap();
        assert_eq!(sql, "DELETE FROM APE_DTS.T WHERE ID IS NULL");
    }

    #[test]
    fn build_insert_sql_uses_current_oracle_schema_for_pg_public() {
        let mut after = HashMap::new();
        after.insert("id".to_string(), ColValue::Long(1));
        after.insert(
            "payload".to_string(),
            ColValue::String("before".to_string()),
        );

        let row = RowData::new(
            "public".to_string(),
            "t_gaussdb_oracle_to_oracle".to_string(),
            RowType::Insert,
            None,
            Some(after),
        );

        let sql = OracleSinker::build_insert_sql(&row, false, &[]).unwrap();
        assert_eq!(
            sql,
            "INSERT INTO T_GAUSSDB_ORACLE_TO_ORACLE (ID,PAYLOAD) VALUES (1,'before')"
        );
    }

    #[test]
    fn build_insert_sql_with_replace_deletes_by_key_before_insert() {
        let mut after = HashMap::new();
        after.insert("tenant_id".to_string(), ColValue::Long(8));
        after.insert("order_id".to_string(), ColValue::Long(1));
        after.insert("payload".to_string(), ColValue::String("after".to_string()));

        let row = RowData::new(
            "APE_DTS".to_string(),
            "T".to_string(),
            RowType::Insert,
            None,
            Some(after),
        );

        let key_cols = vec!["tenant_id".to_string(), "order_id".to_string()];
        let sql = OracleSinker::build_insert_sql(&row, true, &key_cols).unwrap();
        assert_eq!(
            sql,
            "DELETE FROM APE_DTS.T WHERE ORDER_ID=1 AND TENANT_ID=8;\nINSERT INTO APE_DTS.T (ORDER_ID,PAYLOAD,TENANT_ID) VALUES (1,'after',8)"
        );
    }
}
