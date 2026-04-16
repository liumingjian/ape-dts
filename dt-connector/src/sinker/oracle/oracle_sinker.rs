use std::sync::Arc;

use anyhow::{bail, Context};
use async_trait::async_trait;

use crate::oracle::OracleSqlPlusClient;
use crate::sinker::base_sinker::BaseSinker;
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
                RowType::Insert => Self::build_insert_sql(row)?,
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
    pub(crate) fn build_insert_sql(row: &RowData) -> anyhow::Result<String> {
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

        Ok(format!(
            "INSERT INTO {}.{} ({}) VALUES ({})",
            row.schema,
            row.tb,
            cols.join(","),
            values.join(",")
        ))
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
            set_pairs.push(format!("{}={}", col, Self::to_oracle_literal(v)?));
        }

        let where_sql = Self::build_where_sql(before)?;

        Ok(format!(
            "UPDATE {}.{} SET {} WHERE {}",
            row.schema,
            row.tb,
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
            "DELETE FROM {}.{} WHERE {}",
            row.schema, row.tb, where_sql
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
                clauses.push(format!("{} IS NULL", col));
            } else {
                clauses.push(format!("{}={}", col, Self::to_oracle_literal(v)?));
            }
        }
        Ok(clauses.join(" AND "))
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
}
