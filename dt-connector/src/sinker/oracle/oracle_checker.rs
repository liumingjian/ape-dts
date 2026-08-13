use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::{bail, Context};
use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::oracle::OracleSqlPlusClient;
use crate::sinker::base_checker::{Checker, CheckerCommon, CheckerTbMeta};
use crate::sinker::oracle::oracle_name::{oracle_owner_expr, oracle_table_ref};
use crate::sinker::oracle::oracle_sinker::OracleSinker;
use dt_common::meta::col_value::ColValue;
use dt_common::meta::rdb_meta_manager::RdbMetaManager;
use dt_common::meta::rdb_tb_meta::RdbTbMeta;
use dt_common::meta::row_data::RowData;
use dt_common::meta::row_type::RowType;

#[derive(Clone)]
pub struct OracleChecker {
    pub client: OracleSqlPlusClient,
    pub meta_cache: Arc<Mutex<HashMap<String, RdbTbMeta>>>,
    pub common: CheckerCommon,
}

#[async_trait]
impl Checker for OracleChecker {
    fn common_mut(&mut self) -> &mut CheckerCommon {
        &mut self.common
    }

    async fn get_tb_meta_by_row(&mut self, row: &RowData) -> anyhow::Result<CheckerTbMeta> {
        let meta = self.get_or_load_meta(&row.schema, &row.tb).await?;
        Ok(CheckerTbMeta::Oracle(meta))
    }

    async fn fetch_batch(
        &self,
        tb_meta: &CheckerTbMeta,
        data: &[&RowData],
    ) -> anyhow::Result<Vec<RowData>> {
        let meta = tb_meta.basic();
        let mut out = Vec::with_capacity(data.len());
        for &row in data {
            if let Some(dst_row) = self.fetch_one(meta, row).await? {
                out.push(dst_row);
            }
        }
        Ok(out)
    }
}

impl OracleChecker {
    async fn get_or_load_meta(&self, schema: &str, tb: &str) -> anyhow::Result<RdbTbMeta> {
        let key = format!("{}.{}", schema, tb);

        {
            let cache = self.meta_cache.lock().await;
            if let Some(meta) = cache.get(&key) {
                return Ok(meta.clone());
            }
        }

        let meta = fetch_oracle_tb_meta(&self.client, schema, tb).await?;
        let mut cache = self.meta_cache.lock().await;
        cache.insert(key, meta.clone());
        Ok(meta)
    }

    async fn fetch_one(
        &self,
        meta: &RdbTbMeta,
        src_row: &RowData,
    ) -> anyhow::Result<Option<RowData>> {
        if meta.id_cols.is_empty() {
            bail!(
                "oracle checker requires non-empty id_cols for {}.{}",
                meta.schema,
                meta.tb
            );
        }

        let src_cols = match src_row.row_type {
            RowType::Insert | RowType::Update => src_row.require_after()?,
            RowType::Delete => src_row.require_before()?,
        };

        let mut key_values = HashMap::with_capacity(meta.id_cols.len());
        for col in meta.id_cols.iter() {
            let v = src_cols
                .get(col)
                .with_context(|| format!("oracle checker missing id col {} in src row", col))?
                .clone();
            key_values.insert(col.clone(), v);
        }

        let where_sql = OracleSinker::build_where_sql(&key_values)?;

        let mut select_cols: Vec<String> = src_cols.keys().cloned().collect();
        for col in meta.id_cols.iter() {
            if !select_cols.iter().any(|c| c == col) {
                select_cols.push(col.clone());
            }
        }
        select_cols.sort();

        let sql = format!(
            "SELECT {} FROM {} WHERE {}",
            OracleSinker::column_list(&select_cols)?,
            oracle_table_ref(&meta.schema, &meta.tb)?,
            where_sql
        );
        let lines = self.client.query_lines(&sql).await?;
        let Some(line) = lines.into_iter().next() else {
            return Ok(None);
        };

        let raw_values: Vec<&str> = line.split('|').map(|s| s.trim()).collect();
        if raw_values.len() != select_cols.len() {
            bail!(
                "oracle checker row column count mismatch for {}.{}: expected {}, got {}, line={}",
                meta.schema,
                meta.tb,
                select_cols.len(),
                raw_values.len(),
                line
            );
        }

        let mut after = HashMap::with_capacity(select_cols.len());
        for (idx, col) in select_cols.iter().enumerate() {
            let oracle_type = meta
                .col_origin_type_map
                .get(col)
                .map(|s| s.as_str())
                .context("oracle checker missing column type in meta")?;
            let expected = src_cols.get(col);
            let v =
                parse_oracle_value(raw_values[idx], oracle_type, expected).with_context(|| {
                    format!(
                        "oracle checker parse col failed: {}.{} col={} oracle_type={} raw={}",
                        meta.schema, meta.tb, col, oracle_type, raw_values[idx]
                    )
                })?;
            after.insert(col.clone(), v);
        }

        Ok(Some(RowData::new(
            meta.schema.clone(),
            meta.tb.clone(),
            RowType::Insert,
            None,
            Some(after),
        )))
    }
}

async fn fetch_oracle_tb_meta(
    client: &OracleSqlPlusClient,
    schema: &str,
    tb: &str,
) -> anyhow::Result<RdbTbMeta> {
    let owner = oracle_owner_expr(schema)?;
    let table = escape_sql_literal(&tb.to_uppercase());

    let col_sql = format!(
        "SELECT column_name, data_type, nullable, column_id \
         FROM all_tab_columns \
         WHERE owner={} AND table_name='{}' \
         ORDER BY column_id ASC",
        owner, table
    );
    let col_lines = client.query_lines(&col_sql).await?;
    if col_lines.is_empty() {
        bail!("oracle meta: columns not found for {}.{}", schema, tb);
    }

    let mut cols = Vec::new();
    let mut nullable_cols = HashSet::new();
    let mut col_origin_type_map = HashMap::new();

    for line in col_lines {
        let parts: Vec<&str> = line.split('|').map(|s| s.trim()).collect();
        if parts.len() < 3 {
            continue;
        }
        let name = parts[0].to_string();
        let data_type = parts[1].to_string();
        let nullable = parts[2].eq_ignore_ascii_case("Y");

        cols.push(name.clone());
        if nullable {
            nullable_cols.insert(name.clone());
        }
        col_origin_type_map.insert(name.clone(), data_type);
    }

    if cols.is_empty() {
        bail!("oracle meta: empty column list for {}.{}", schema, tb);
    }

    let key_sql = format!(
        "SELECT cons.constraint_name, cons.constraint_type, cols.column_name, cols.position \
         FROM all_constraints cons \
         JOIN all_cons_columns cols \
           ON cons.owner=cols.owner AND cons.constraint_name=cols.constraint_name \
         WHERE cons.owner={} AND cons.table_name='{}' \
           AND cons.constraint_type IN ('P','U') \
         ORDER BY cons.constraint_name ASC, cols.position ASC",
        owner, table
    );
    let key_lines = client.query_lines(&key_sql).await?;

    let mut tmp: HashMap<(String, String), Vec<(usize, String)>> = HashMap::new();
    for line in key_lines {
        let parts: Vec<&str> = line.split('|').map(|s| s.trim()).collect();
        if parts.len() < 4 {
            continue;
        }
        let constraint_name = parts[0].to_string();
        let constraint_type = parts[1].to_string();
        let col = parts[2].to_string();
        let pos = parts[3].parse::<usize>().unwrap_or(0);
        tmp.entry((constraint_name, constraint_type))
            .or_default()
            .push((pos, col));
    }

    let mut key_map: HashMap<String, Vec<String>> = HashMap::new();
    for ((name, ty), mut entries) in tmp {
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        let cols = entries.into_iter().map(|(_, c)| c).collect::<Vec<_>>();
        if ty.eq_ignore_ascii_case("P") {
            key_map.insert(
                dt_common::meta::rdb_meta_manager::RDB_PRIMARY_KEY_FLAG.to_string(),
                cols,
            );
        } else if ty.eq_ignore_ascii_case("U") {
            key_map.insert(name, cols);
        }
    }

    let (order_cols, partition_col, id_cols) =
        RdbMetaManager::parse_rdb_cols(&key_map, &cols, &nullable_cols)?;

    Ok(RdbTbMeta {
        schema: schema.to_string(),
        tb: tb.to_string(),
        cols,
        nullable_cols,
        col_origin_type_map,
        key_map,
        order_cols,
        partition_col,
        id_cols,
        ..Default::default()
    })
}

fn parse_oracle_value(
    raw: &str,
    oracle_type: &str,
    expected: Option<&ColValue>,
) -> anyhow::Result<ColValue> {
    if raw.is_empty() || raw == "<NULL>" {
        return Ok(ColValue::None);
    }

    if let Some(expected) = expected {
        return parse_with_expected(raw, expected);
    }

    let ty = oracle_type.trim().to_uppercase();
    if ty.starts_with("NUMBER") {
        if raw.contains('.') {
            return Ok(ColValue::Decimal(raw.to_string()));
        }
        return Ok(ColValue::LongLong(
            raw.parse::<i64>()
                .with_context(|| format!("invalid number: {}", raw))?,
        ));
    }
    if ty.contains("FLOAT") || ty.contains("BINARY_FLOAT") || ty.contains("BINARY_DOUBLE") {
        return Ok(ColValue::Double(
            raw.parse::<f64>()
                .with_context(|| format!("invalid float: {}", raw))?,
        ));
    }
    if ty.starts_with("DATE") {
        return Ok(ColValue::DateTime(raw.to_string()));
    }
    if ty.starts_with("TIMESTAMP") {
        return Ok(ColValue::Timestamp(raw.to_string()));
    }

    Ok(ColValue::String(raw.to_string()))
}

fn parse_with_expected(raw: &str, expected: &ColValue) -> anyhow::Result<ColValue> {
    Ok(match expected {
        ColValue::None => ColValue::String(raw.to_string()),
        ColValue::Bool(_) => ColValue::Bool(raw != "0"),
        ColValue::Tiny(_) => ColValue::Tiny(raw.parse::<i8>()?),
        ColValue::UnsignedTiny(_) => ColValue::UnsignedTiny(raw.parse::<u8>()?),
        ColValue::Short(_) => ColValue::Short(raw.parse::<i16>()?),
        ColValue::UnsignedShort(_) => ColValue::UnsignedShort(raw.parse::<u16>()?),
        ColValue::Long(_) => ColValue::Long(raw.parse::<i32>()?),
        ColValue::UnsignedLong(_) => ColValue::UnsignedLong(raw.parse::<u32>()?),
        ColValue::LongLong(_) => ColValue::LongLong(raw.parse::<i64>()?),
        ColValue::UnsignedLongLong(_) => ColValue::UnsignedLongLong(raw.parse::<u64>()?),
        ColValue::Float(_) => ColValue::Float(raw.parse::<f32>()?),
        ColValue::Double(_) => ColValue::Double(raw.parse::<f64>()?),
        ColValue::Decimal(_) => ColValue::Decimal(raw.to_string()),
        ColValue::Time(_) => ColValue::Time(raw.to_string()),
        ColValue::Date(_) => ColValue::Date(raw.to_string()),
        ColValue::DateTime(_) => ColValue::DateTime(raw.to_string()),
        ColValue::Timestamp(_) => ColValue::Timestamp(raw.to_string()),
        ColValue::Year(_) => ColValue::Year(raw.parse::<u16>()?),
        ColValue::String(_) => ColValue::String(raw.to_string()),
        ColValue::RawString(_) => ColValue::String(raw.to_string()),
        ColValue::Blob(_) => ColValue::String(raw.to_string()),
        ColValue::Bit(_) => ColValue::Bit(raw.parse::<u64>()?),
        ColValue::Set(_) => ColValue::Set(raw.parse::<u64>()?),
        ColValue::Enum(_) => ColValue::Enum(raw.parse::<u32>()?),
        ColValue::Set2(_) => ColValue::Set2(raw.to_string()),
        ColValue::Enum2(_) => ColValue::Enum2(raw.to_string()),
        ColValue::Json(_) => ColValue::Json(raw.as_bytes().to_vec()),
        ColValue::Json2(_) => ColValue::Json2(raw.to_string()),
        ColValue::Json3(_) => ColValue::Json2(raw.to_string()),
        ColValue::MongoDoc(_) => ColValue::String(raw.to_string()),
        // a check never compares against a value the source did not carry
        ColValue::Unavailable => ColValue::Unavailable,
    })
}

fn escape_sql_literal(s: &str) -> String {
    s.replace('\'', "''")
}
