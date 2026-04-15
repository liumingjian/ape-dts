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

        // Bootstrap: snapshot only (INSERT). Update/Delete will be handled when CDC support is added.
        if !data.iter().all(|r| matches!(r.row_type, RowType::Insert)) {
            bail!("oracle sinker only supports INSERT for now (bootstrap snapshot)");
        }

        let mut sqls = Vec::with_capacity(data.len() + 1);
        let mut data_size = 0u64;
        for row in &data {
            data_size += row.data_size as u64;
            sqls.push(Self::build_insert_sql(row)?);
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
    fn build_insert_sql(row: &RowData) -> anyhow::Result<String> {
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

    fn escape_str(s: &str) -> String {
        s.replace('\'', "''")
    }

    fn to_oracle_literal(v: &ColValue) -> anyhow::Result<String> {
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

