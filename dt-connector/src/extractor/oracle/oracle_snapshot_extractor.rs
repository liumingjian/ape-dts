use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{bail, Context};
use async_trait::async_trait;

use crate::extractor::{base_extractor::BaseExtractor, resumer::recovery::Recovery};
use crate::oracle::OracleSqlPlusClient;
use crate::Extractor;
use dt_common::log_info;
use dt_common::meta::{
    col_value::ColValue, position::Position, row_data::RowData, row_type::RowType,
};
use dt_common::rdb_filter::RdbFilter;

#[derive(Debug, Clone)]
struct OracleColumn {
    name: String,
    data_type: String,
}

pub struct OracleSnapshotExtractor {
    pub base_extractor: BaseExtractor,
    pub client: OracleSqlPlusClient,
    pub filter: RdbFilter,
    pub batch_size: usize,
    pub parallel_size: usize,
    pub sample_interval: u64,
    pub schema: String,
    pub tb: String,
    pub recovery: Option<Arc<dyn Recovery + Send + Sync>>,
}

#[async_trait]
impl Extractor for OracleSnapshotExtractor {
    async fn extract(&mut self) -> anyhow::Result<()> {
        log_info!(
            "OracleSnapshotExtractor starts, schema: {}, tb: {}, batch_size: {}, parallel_size: {}",
            self.schema,
            self.tb,
            self.batch_size,
            self.parallel_size
        );

        self.extract_internal().await?;
        self.base_extractor.wait_task_finish().await
    }

    async fn close(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

impl OracleSnapshotExtractor {
    async fn extract_internal(&mut self) -> anyhow::Result<()> {
        let columns = self.fetch_columns().await?;
        if columns.is_empty() {
            bail!("oracle columns not found for {}.{}", self.schema, self.tb);
        }

        let ignore_cols = self.filter.get_ignore_cols(&self.schema, &self.tb);
        let where_condition = self
            .filter
            .get_where_condition(&self.schema, &self.tb)
            .cloned()
            .unwrap_or_default();

        let select_cols: Vec<&OracleColumn> = columns
            .iter()
            .filter(|c| !ignore_cols.is_some_and(|cols| cols.contains(&c.name)))
            .collect();
        if select_cols.is_empty() {
            bail!(
                "oracle all columns are ignored for {}.{}",
                self.schema,
                self.tb
            );
        }

        let select_sql = {
            let cols = select_cols
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>()
                .join(",");
            let mut sql = format!("SELECT {} FROM {}.{}", cols, self.schema, self.tb);
            if !where_condition.trim().is_empty() {
                sql.push_str(" WHERE ");
                sql.push_str(where_condition.trim());
            }
            sql.push_str(&format!(" ORDER BY {} ASC", select_cols[0].name));
            sql
        };

        let lines = self.client.query_lines(&select_sql).await?;
        let mut extracted = 0u64;
        for line in lines {
            extracted += 1;
            if self.sample_interval > 1 && extracted % self.sample_interval != 0 {
                continue;
            }

            let values: Vec<&str> = line.split('|').collect();
            if values.len() != select_cols.len() {
                bail!(
                    "oracle row column count mismatch for {}.{}: expected {}, got {}, line={}",
                    self.schema,
                    self.tb,
                    select_cols.len(),
                    values.len(),
                    line
                );
            }

            let mut after: HashMap<String, ColValue> = HashMap::with_capacity(select_cols.len());
            for (idx, col) in select_cols.iter().enumerate() {
                let raw = values[idx].trim();
                let v = Self::parse_col_value(raw, &col.data_type).with_context(|| {
                    format!(
                        "oracle parse col value failed: schema={}, tb={}, col={}, data_type={}, raw={}",
                        self.schema, self.tb, col.name, col.data_type, raw
                    )
                })?;
                after.insert(col.name.clone(), v);
            }

            let row_data = RowData::new(
                self.schema.clone(),
                self.tb.clone(),
                RowType::Insert,
                None,
                Some(after),
            );
            self.base_extractor
                .push_row(row_data, Position::None)
                .await?;
        }

        Ok(())
    }

    async fn fetch_columns(&self) -> anyhow::Result<Vec<OracleColumn>> {
        let owner = self.schema.to_uppercase();
        let table = self.tb.to_uppercase();
        let sql = format!(
            "SELECT column_name, data_type FROM all_tab_columns WHERE owner='{}' AND table_name='{}' ORDER BY column_id ASC",
            owner.replace('\'', "''"),
            table.replace('\'', "''"),
        );
        let lines = self.client.query_lines(&sql).await?;
        let mut cols = Vec::new();
        for line in lines {
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() < 2 {
                continue;
            }
            cols.push(OracleColumn {
                name: parts[0].trim().to_string(),
                data_type: parts[1].trim().to_string(),
            });
        }
        Ok(cols)
    }

    fn parse_col_value(raw: &str, data_type: &str) -> anyhow::Result<ColValue> {
        if raw.is_empty() || raw == "<NULL>" {
            return Ok(ColValue::None);
        }

        let ty = data_type.trim().to_uppercase();
        match ty.as_str() {
            "NUMBER" => {
                if raw.contains('.') {
                    Ok(ColValue::Decimal(raw.to_string()))
                } else if let Ok(v) = raw.parse::<i64>() {
                    Ok(ColValue::LongLong(v))
                } else {
                    Ok(ColValue::Decimal(raw.to_string()))
                }
            }
            "FLOAT" | "BINARY_FLOAT" | "BINARY_DOUBLE" => Ok(ColValue::Double(
                raw.parse::<f64>()
                    .with_context(|| format!("invalid float: {}", raw))?,
            )),
            "DATE" => Ok(ColValue::DateTime(raw.to_string())),
            "TIMESTAMP" | "TIMESTAMP(6)" => Ok(ColValue::Timestamp(raw.to_string())),
            _ => Ok(ColValue::String(raw.to_string())),
        }
    }
}
