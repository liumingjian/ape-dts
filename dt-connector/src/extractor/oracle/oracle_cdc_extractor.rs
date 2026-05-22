use std::{
    collections::HashMap,
    sync::{atomic::Ordering, Arc},
    time::{Duration, UNIX_EPOCH},
};

use anyhow::{bail, Context};
use async_trait::async_trait;

use crate::extractor::{base_extractor::BaseExtractor, resumer::recovery::Recovery};
use crate::oracle::OracleSqlPlusClient;
use crate::Extractor;
use dt_common::{
    config::{config_enums::DbType, connection_auth_config::ConnectionAuthConfig},
    log_info,
    meta::{col_value::ColValue, position::Position, row_data::RowData, row_type::RowType},
    rdb_filter::RdbFilter,
};

const DEFAULT_POLL_INTERVAL_MILLIS: u64 = 200;
const DEFAULT_POLL_BATCH_SIZE: usize = 200;

// Bootstrap CDC: store before/after row images as a single string joined by this delimiter.
// This delimiter must not contain `|` because sqlplus output uses `SET COLSEP '|'`.
const CDC_VALUE_SEP: &str = "<DT_SEP>";

const LOG_TABLE: &str = "APE_DTS_CDC_LOG";
const LOG_SEQ: &str = "APE_DTS_CDC_LOG_SEQ";

#[derive(Debug, Clone)]
struct OracleColumn {
    name: String,
    data_type: String,
}

pub struct OracleCdcExtractor {
    pub base_extractor: BaseExtractor,
    pub client: OracleSqlPlusClient,
    pub filter: RdbFilter,
    pub poll_interval_millis: u64,
    pub poll_batch_size: usize,
    pub start_change_id: u64,
    pub recovery: Option<Arc<dyn Recovery + Send + Sync>>,
}

#[async_trait]
impl Extractor for OracleCdcExtractor {
    async fn extract(&mut self) -> anyhow::Result<()> {
        log_info!(
            "OracleCdcExtractor starts (bootstrap trigger-based), poll_interval_millis={}, poll_batch_size={}, start_change_id={}",
            self.poll_interval_millis,
            self.poll_batch_size,
            self.start_change_id
        );

        self.extract_internal().await?;
        self.base_extractor.wait_task_finish().await
    }

    async fn close(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

impl OracleCdcExtractor {
    fn require_username(&self) -> anyhow::Result<String> {
        match &self.client.connection_auth {
            ConnectionAuthConfig::Basic { username, .. } => Ok(username.to_uppercase()),
            ConnectionAuthConfig::NoAuth => bail!("oracle cdc requires basic auth username"),
        }
    }

    fn captured_tables(&self) -> anyhow::Result<Vec<(String, String)>> {
        if self.filter.do_tbs.is_empty() {
            bail!("oracle cdc requires [filter].do_tbs to be set (explicit tables only)");
        }

        let mut out: Vec<(String, String)> = self.filter.do_tbs.iter().cloned().collect();
        out.sort();

        for (schema, tb) in &out {
            if RdbFilter::is_pattern(schema, &DbType::Oracle)
                || RdbFilter::is_pattern(tb, &DbType::Oracle)
            {
                bail!(
                    "oracle cdc bootstrap does not support pattern do_tbs yet: {}.{}",
                    schema,
                    tb
                );
            }
        }

        Ok(out)
    }

    async fn extract_internal(&mut self) -> anyhow::Result<()> {
        let username = self.require_username()?;
        let captured = self.captured_tables()?;
        for (schema, _tb) in &captured {
            if schema.to_uppercase() != username {
                bail!(
                    "oracle cdc bootstrap only supports tables in current user schema (expected {}, got {})",
                    username,
                    schema
                );
            }
        }

        self.ensure_log_table_and_seq().await?;

        let mut columns_cache: HashMap<(String, String), Vec<OracleColumn>> = HashMap::new();
        for (schema, tb) in &captured {
            let cols = self.fetch_columns(schema, tb).await?;
            if cols.is_empty() {
                bail!("oracle cdc columns not found for {}.{}", schema, tb);
            }
            self.ensure_trigger(schema, tb, &cols).await?;
            columns_cache.insert((schema.clone(), tb.clone()), cols);
        }

        // Start from the latest log entry by default, so pre-existing logs from other runs don't
        // interfere with test stability.
        let mut last_change_id = if self.start_change_id > 0 {
            self.start_change_id
        } else {
            self.fetch_max_change_id().await?
        };

        let poll_interval = Duration::from_millis(std::cmp::max(
            1,
            if self.poll_interval_millis > 0 {
                self.poll_interval_millis
            } else {
                DEFAULT_POLL_INTERVAL_MILLIS
            },
        ));
        let poll_batch_size = if self.poll_batch_size > 0 {
            self.poll_batch_size
        } else {
            DEFAULT_POLL_BATCH_SIZE
        };

        let mut idle_ticks: u64 = 0;

        loop {
            if self.base_extractor.shut_down.load(Ordering::Acquire) {
                return Ok(());
            }

            // Best-effort end_time_utc support (mirror GaussDB CDC behavior).
            if !self.base_extractor.time_filter.ended
                && self.base_extractor.time_filter.end_timestamp != u32::MAX
            {
                let now_sec = UNIX_EPOCH.elapsed()?.as_secs() as u32;
                if now_sec >= self.base_extractor.time_filter.end_timestamp {
                    self.base_extractor.time_filter.ended = true;
                }
            }
            if self.base_extractor.time_filter.ended {
                return Ok(());
            }

            let lines = self
                .fetch_log_rows(&captured, last_change_id, poll_batch_size)
                .await?;

            if lines.is_empty() {
                idle_ticks += 1;
                // Reduce log spam while still showing liveness.
                if idle_ticks == 1 || idle_ticks % 300 == 0 {
                    log_info!(
                        "oracle cdc idle (no new rows), last_change_id={}, captured_tables={}",
                        last_change_id,
                        captured.len()
                    );
                }
                tokio::time::sleep(poll_interval).await;
                continue;
            }
            idle_ticks = 0;

            for line in lines {
                // change_id | tb_name | op_type | before_data | after_data
                let parts: Vec<&str> = line.split('|').collect();
                if parts.len() < 5 {
                    bail!("oracle cdc log row parse failed, line={}", line);
                }

                let change_id: u64 = parts[0]
                    .trim()
                    .parse()
                    .with_context(|| format!("invalid change_id in line: {}", line))?;
                let tb = parts[1].trim().to_string();
                let op = parts[2].trim();
                let before_raw = parts[3].trim();
                let after_raw = parts[4].trim();

                // We only create triggers for explicit tables, so resolve schema by lookup.
                let schema = captured
                    .iter()
                    .find(|(_s, t)| t.eq_ignore_ascii_case(&tb))
                    .map(|(s, _)| s.clone())
                    .with_context(|| format!("unexpected table in oracle cdc log: {}", tb))?;

                let row_type = match op {
                    "I" => RowType::Insert,
                    "U" => RowType::Update,
                    "D" => RowType::Delete,
                    other => bail!("unknown oracle cdc op_type '{}', line={}", other, line),
                };

                if self.filter.filter_event(&schema, &tb, &row_type) {
                    last_change_id = change_id;
                    continue;
                }

                let cols = columns_cache
                    .get(&(schema.clone(), tb.clone()))
                    .with_context(|| {
                        format!("oracle cdc columns cache missing for {}.{}", schema, tb)
                    })?;

                let ignore_cols = self.filter.get_ignore_cols(&schema, &tb);

                let before = Self::parse_row_image(before_raw, cols, ignore_cols)?;
                let after = Self::parse_row_image(after_raw, cols, ignore_cols)?;

                let (before, after) = match row_type {
                    RowType::Insert => (None, after),
                    RowType::Update => (before, after),
                    RowType::Delete => (before, None),
                };

                let row_data = RowData::new(schema, tb, row_type, before, after);
                self.base_extractor
                    .push_row(row_data, Position::None)
                    .await?;

                last_change_id = change_id;
            }
        }
    }

    async fn ensure_log_table_and_seq(&self) -> anyhow::Result<()> {
        let create_table = format!(
            r#"
DECLARE
  v_cnt INTEGER;
BEGIN
  SELECT COUNT(*) INTO v_cnt FROM user_tables WHERE table_name = '{log_table}';
  IF v_cnt = 0 THEN
    EXECUTE IMMEDIATE 'CREATE TABLE {log_table} (
      CHANGE_ID NUMBER PRIMARY KEY,
      TB_NAME VARCHAR2(128) NOT NULL,
      OP_TYPE CHAR(1) NOT NULL,
      BEFORE_DATA VARCHAR2(4000),
      AFTER_DATA VARCHAR2(4000),
      CREATED_AT DATE DEFAULT SYSDATE NOT NULL
    )';
  END IF;
END;
/
"#,
            log_table = LOG_TABLE
        );
        self.client.exec(&create_table).await?;

        let create_seq = format!(
            r#"
DECLARE
  v_cnt INTEGER;
BEGIN
  SELECT COUNT(*) INTO v_cnt FROM user_sequences WHERE sequence_name = '{log_seq}';
  IF v_cnt = 0 THEN
    EXECUTE IMMEDIATE 'CREATE SEQUENCE {log_seq} START WITH 1 INCREMENT BY 1 NOCACHE';
  END IF;
END;
/
"#,
            log_seq = LOG_SEQ
        );
        self.client.exec(&create_seq).await?;
        Ok(())
    }

    async fn fetch_max_change_id(&self) -> anyhow::Result<u64> {
        let lines = self
            .client
            .query_lines(&format!("SELECT NVL(MAX(change_id), 0) FROM {}", LOG_TABLE))
            .await?;
        let first = lines.first().cloned().unwrap_or_default();
        if first.trim().is_empty() {
            return Ok(0);
        }
        Ok(first.trim().parse::<u64>().unwrap_or(0))
    }

    async fn fetch_columns(&self, _schema: &str, tb: &str) -> anyhow::Result<Vec<OracleColumn>> {
        let table = tb.to_uppercase().replace('\'', "''");
        let sql = format!(
            "SELECT column_name, data_type FROM user_tab_columns WHERE table_name='{}' ORDER BY column_id ASC",
            table
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

    fn trigger_name(tb: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let base = format!("DTCDC_{}", tb.to_uppercase());
        if base.len() <= 30 {
            return base;
        }
        let mut hasher = DefaultHasher::new();
        tb.to_uppercase().hash(&mut hasher);
        let suffix = format!("{:x}", hasher.finish() & 0xfffff);
        let keep = 30usize.saturating_sub("DTCDC__".len() + suffix.len());
        let mut prefix = tb.to_uppercase();
        prefix.truncate(keep);
        format!("DTCDC_{}_{}", prefix, suffix)
    }

    fn col_ref(record: &str, col: &str) -> String {
        let escaped = col.replace('"', "\"\"");
        format!(r#"{record}."{col}""#, record = record, col = escaped)
    }

    fn to_char_expr(col_ref: &str, data_type: &str) -> String {
        let ty = data_type.trim().to_uppercase();
        match ty.as_str() {
            "DATE" => format!("TO_CHAR({}, 'YYYY-MM-DD HH24:MI:SS')", col_ref),
            "TIMESTAMP" | "TIMESTAMP(6)" => {
                format!("TO_CHAR({}, 'YYYY-MM-DD HH24:MI:SS.FF6')", col_ref)
            }
            _ => format!("TO_CHAR({})", col_ref),
        }
    }

    fn normalize_value_expr(expr: &str) -> String {
        // Avoid breaking sqlplus row parsing (COLSEP='|') and keep output one-line.
        // NOTE: bootstrap only; not a full escaping strategy.
        format!(
            "REPLACE(REPLACE(REPLACE({}, '|', '<PIPE>'), CHR(10), '<LF>'), CHR(13), '<CR>')",
            expr
        )
    }

    fn build_row_image_expr(columns: &[OracleColumn], record: &str) -> anyhow::Result<String> {
        if columns.is_empty() {
            bail!("oracle cdc: empty column list");
        }

        let mut exprs = Vec::with_capacity(columns.len());
        for col in columns {
            let col_ref = Self::col_ref(record, &col.name);
            let to_char = Self::to_char_expr(&col_ref, &col.data_type);
            let normalized = Self::normalize_value_expr(&to_char);
            exprs.push(format!("NVL({}, '<NULL>')", normalized));
        }

        let mut out = String::new();
        for (idx, expr) in exprs.iter().enumerate() {
            if idx > 0 {
                out.push_str(" || '");
                out.push_str(CDC_VALUE_SEP);
                out.push_str("' || ");
            }
            out.push_str(expr);
        }
        Ok(out)
    }

    async fn ensure_trigger(
        &self,
        _schema: &str,
        tb: &str,
        columns: &[OracleColumn],
    ) -> anyhow::Result<()> {
        let trigger = Self::trigger_name(tb);
        let before_expr = Self::build_row_image_expr(columns, ":OLD")?;
        let after_expr = Self::build_row_image_expr(columns, ":NEW")?;

        let tb_upper = tb.to_uppercase().replace('\'', "''");

        let ddl = format!(
            r#"
CREATE OR REPLACE TRIGGER {trigger}
AFTER INSERT OR UPDATE OR DELETE ON {tb}
FOR EACH ROW
DECLARE
  v_before VARCHAR2(4000);
  v_after  VARCHAR2(4000);
BEGIN
  IF INSERTING THEN
    v_after := {after_expr};
    INSERT INTO {log_table} (CHANGE_ID, TB_NAME, OP_TYPE, BEFORE_DATA, AFTER_DATA)
      VALUES ({log_seq}.NEXTVAL, '{tb_upper}', 'I', NULL, v_after);
  ELSIF UPDATING THEN
    v_before := {before_expr};
    v_after := {after_expr};
    INSERT INTO {log_table} (CHANGE_ID, TB_NAME, OP_TYPE, BEFORE_DATA, AFTER_DATA)
      VALUES ({log_seq}.NEXTVAL, '{tb_upper}', 'U', v_before, v_after);
  ELSIF DELETING THEN
    v_before := {before_expr};
    INSERT INTO {log_table} (CHANGE_ID, TB_NAME, OP_TYPE, BEFORE_DATA, AFTER_DATA)
      VALUES ({log_seq}.NEXTVAL, '{tb_upper}', 'D', v_before, NULL);
  END IF;
END;
/
"#,
            trigger = trigger,
            tb = tb,
            log_table = LOG_TABLE,
            log_seq = LOG_SEQ,
            tb_upper = tb_upper,
            before_expr = before_expr,
            after_expr = after_expr,
        );

        self.client.exec(&ddl).await?;
        Ok(())
    }

    async fn fetch_log_rows(
        &self,
        captured: &[(String, String)],
        last_change_id: u64,
        batch_size: usize,
    ) -> anyhow::Result<Vec<String>> {
        // captured is explicit, so we can filter by TB_NAME IN (...)
        let mut tbs = captured
            .iter()
            .map(|(_, t)| t.to_uppercase())
            .collect::<Vec<_>>();
        tbs.sort();
        tbs.dedup();

        let tb_in = tbs
            .iter()
            .map(|t| format!("'{}'", t.replace('\'', "''")))
            .collect::<Vec<_>>()
            .join(",");

        // Oracle 11g: use ORDER BY in subquery + ROWNUM in outer query.
        let sql = format!(
            "SELECT change_id, tb_name, op_type, before_data, after_data FROM (SELECT change_id, tb_name, op_type, before_data, after_data FROM {log_table} WHERE change_id > {last} AND tb_name IN ({tb_in}) ORDER BY change_id ASC) WHERE ROWNUM <= {limit}",
            log_table = LOG_TABLE,
            last = last_change_id,
            tb_in = tb_in,
            limit = batch_size
        );

        let mut lines = self.client.query_lines(&sql).await?;

        // `sqlplus` can occasionally return transient EOF errors when container is under load.
        // Keep this extractor resilient by retrying a few times.
        if lines.is_empty() {
            return Ok(lines);
        }

        // Filter out accidental header/empty lines (defensive).
        lines.retain(|s| !s.trim().is_empty());
        Ok(lines)
    }

    fn parse_row_image(
        raw: &str,
        cols: &[OracleColumn],
        ignore_cols: Option<&std::collections::HashSet<String>>,
    ) -> anyhow::Result<Option<HashMap<String, ColValue>>> {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed == "<NULL>" {
            return Ok(None);
        }

        let values: Vec<&str> = trimmed.split(CDC_VALUE_SEP).collect();
        if values.len() != cols.len() {
            bail!(
                "oracle cdc row image column count mismatch: expected {}, got {}, raw={}",
                cols.len(),
                values.len(),
                raw
            );
        }

        let mut out = HashMap::with_capacity(cols.len());
        for (idx, col) in cols.iter().enumerate() {
            if ignore_cols.is_some_and(|set| set.contains(&col.name)) {
                continue;
            }
            let v =
                Self::parse_col_value(values[idx].trim(), &col.data_type).with_context(|| {
                    format!(
                        "oracle cdc parse col value failed: col={}, data_type={}, raw={}",
                        col.name, col.data_type, values[idx]
                    )
                })?;
            out.insert(col.name.clone(), v);
        }
        Ok(Some(out))
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
