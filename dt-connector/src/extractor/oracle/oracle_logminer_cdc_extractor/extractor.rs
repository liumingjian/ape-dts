use std::{
    sync::Arc,
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
    meta::{position::Position, row_data::RowData, row_type::RowType},
    rdb_filter::RdbFilter,
};

use super::{logminer, sql_parser};

const DEFAULT_POLL_INTERVAL_MILLIS: u64 = 200;
const DEFAULT_POLL_BATCH_SIZE: usize = 200;

pub struct OracleLogMinerCdcExtractor {
    pub base_extractor: BaseExtractor,
    pub client: OracleSqlPlusClient,
    pub filter: RdbFilter,
    pub poll_interval_millis: u64,
    pub poll_batch_size: usize,
    pub start_scn: u64,
    pub recovery: Option<Arc<dyn Recovery + Send + Sync>>,
}

#[async_trait]
impl Extractor for OracleLogMinerCdcExtractor {
    async fn extract(&mut self) -> anyhow::Result<()> {
        log_info!(
            "OracleLogMinerCdcExtractor starts (logminer), poll_interval_millis={}, poll_batch_size={}, start_scn={}",
            self.poll_interval_millis,
            self.poll_batch_size,
            self.start_scn
        );

        self.extract_internal().await?;
        self.base_extractor.wait_task_finish().await
    }

    async fn close(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

impl OracleLogMinerCdcExtractor {
    async fn extract_internal(&mut self) -> anyhow::Result<()> {
        let username = self.require_username()?;
        let captured = self.captured_tables()?;
        self.validate_captured_tables(&username, &captured)?;

        let redo_logs = logminer::redo_log_files(&self.client).await?;
        let poll_interval = self.poll_interval();
        let poll_batch_size = self.poll_batch_size();
        let mut cursor = logminer::LogMinerCursor::new(self.init_last_scn().await?);

        let mut idle_ticks: u64 = 0;
        loop {
            if self.should_stop()? {
                return Ok(());
            }

            let outcome = self
                .poll_once(&captured, &redo_logs, &cursor, poll_batch_size)
                .await?;
            match outcome {
                PollOutcome::Idle { next_cursor } => {
                    idle_ticks += 1;
                    self.maybe_log_idle(idle_ticks, cursor.scn, captured.len(), "no new scn");
                    cursor = next_cursor;
                    tokio::time::sleep(poll_interval).await;
                }
                PollOutcome::Rows { next_cursor, rows } => {
                    idle_ticks = 0;
                    self.process_rows(rows).await?;
                    cursor = next_cursor;
                }
            }
        }
    }

    fn require_username(&self) -> anyhow::Result<String> {
        match &self.client.connection_auth {
            ConnectionAuthConfig::Basic { username, .. } => Ok(username.to_uppercase()),
            ConnectionAuthConfig::NoAuth => {
                bail!("oracle logminer cdc requires basic auth username")
            }
        }
    }

    fn captured_tables(&self) -> anyhow::Result<Vec<(String, String)>> {
        if self.filter.do_tbs.is_empty() {
            bail!("oracle logminer cdc requires [filter].do_tbs to be set (explicit tables only)");
        }

        let mut out: Vec<(String, String)> = self.filter.do_tbs.iter().cloned().collect();
        out.sort();

        for (schema, tb) in &out {
            if RdbFilter::is_pattern(schema, &DbType::Oracle)
                || RdbFilter::is_pattern(tb, &DbType::Oracle)
            {
                bail!(
                    "oracle logminer cdc does not support pattern do_tbs yet: {}.{}",
                    schema,
                    tb
                );
            }
        }
        Ok(out)
    }

    fn validate_captured_tables(
        &self,
        username: &str,
        captured: &[(String, String)],
    ) -> anyhow::Result<()> {
        for (schema, _tb) in captured {
            if schema.to_uppercase() != username {
                bail!(
                    "oracle logminer cdc only supports tables in current user schema (expected {}, got {})",
                    username,
                    schema
                );
            }
        }
        Ok(())
    }

    fn poll_interval(&self) -> Duration {
        Duration::from_millis(std::cmp::max(
            1,
            if self.poll_interval_millis > 0 {
                self.poll_interval_millis
            } else {
                DEFAULT_POLL_INTERVAL_MILLIS
            },
        ))
    }

    fn poll_batch_size(&self) -> usize {
        if self.poll_batch_size > 0 {
            self.poll_batch_size
        } else {
            DEFAULT_POLL_BATCH_SIZE
        }
    }

    async fn init_last_scn(&self) -> anyhow::Result<u64> {
        if self.start_scn > 0 {
            return Ok(self.start_scn);
        }
        logminer::current_scn(&self.client).await
    }

    fn should_stop(&mut self) -> anyhow::Result<bool> {
        if self.base_extractor.cancel_token.is_cancelled() {
            return Ok(true);
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

        Ok(self.base_extractor.time_filter.ended)
    }

    async fn poll_once(
        &self,
        captured: &[(String, String)],
        redo_logs: &[String],
        cursor: &logminer::LogMinerCursor,
        limit: usize,
    ) -> anyhow::Result<PollOutcome> {
        let current_scn = logminer::current_scn(&self.client).await?;
        if current_scn <= cursor.scn && !cursor.has_row_position() {
            return Ok(PollOutcome::Idle {
                next_cursor: cursor.clone(),
            });
        }

        let end_scn = current_scn;
        let rows = logminer::fetch_logmnr_rows_in_range(
            &self.client,
            redo_logs,
            cursor,
            end_scn,
            captured,
            limit,
        )
        .await?;

        Ok(rows_poll_outcome(cursor, rows))
    }

    async fn process_rows(&mut self, rows: Vec<logminer::LogMinerRow>) -> anyhow::Result<()> {
        for row in rows {
            let row_type = row_type_from_operation(&row.operation)?;
            if self.filter.filter_event(&row.schema, &row.tb, &row_type) {
                continue;
            }

            let ignore_cols = self.filter.get_ignore_cols(&row.schema, &row.tb);
            let (before, after) =
                sql_parser::row_images_from_logminer(&row_type, &row.sql_redo, &row.sql_undo)
                    .with_context(|| format!("logminer row image parse failed: scn={}", row.scn))?;
            let row_data = RowData::new(row.schema, row.tb, row_type, before, after);
            let row_data = apply_ignore_cols(row_data, ignore_cols);

            self.base_extractor
                .push_row(row_data, Position::None)
                .await?;
        }
        Ok(())
    }

    fn maybe_log_idle(&self, idle_ticks: u64, last_scn: u64, captured: usize, reason: &str) {
        if idle_ticks == 1 || idle_ticks % 300 == 0 {
            log_info!(
                "oracle logminer cdc idle ({}), last_scn={}, captured_tables={}",
                reason,
                last_scn,
                captured
            );
        }
    }
}

fn row_type_from_operation(op: &str) -> anyhow::Result<RowType> {
    match op {
        "INSERT" => Ok(RowType::Insert),
        "UPDATE" => Ok(RowType::Update),
        "DELETE" => Ok(RowType::Delete),
        other => bail!("unknown oracle logminer operation '{}'", other),
    }
}

fn apply_ignore_cols(
    mut row_data: RowData,
    ignore_cols: Option<&std::collections::HashSet<String>>,
) -> RowData {
    let Some(ignore) = ignore_cols else {
        return row_data;
    };

    if let Some(before) = &mut row_data.before {
        for col in ignore {
            before.remove(col);
        }
    }
    if let Some(after) = &mut row_data.after {
        for col in ignore {
            after.remove(col);
        }
    }
    row_data
}

enum PollOutcome {
    Idle {
        next_cursor: logminer::LogMinerCursor,
    },
    Rows {
        next_cursor: logminer::LogMinerCursor,
        rows: Vec<logminer::LogMinerRow>,
    },
}

fn rows_poll_outcome(
    cursor: &logminer::LogMinerCursor,
    rows: Vec<logminer::LogMinerRow>,
) -> PollOutcome {
    if rows.is_empty() {
        return PollOutcome::Idle {
            next_cursor: cursor.clone(),
        };
    }

    let next_cursor = logminer::LogMinerCursor::from_row(rows.last().unwrap());
    PollOutcome::Rows { next_cursor, rows }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn logminer_row(scn: u64, rs_id: &str, ssn: u64) -> logminer::LogMinerRow {
        logminer::LogMinerRow {
            scn,
            rs_id: rs_id.to_string(),
            ssn,
            operation: "INSERT".to_string(),
            schema: "APE_SRC".to_string(),
            tb: "CDC_SMOKE".to_string(),
            sql_redo: String::new(),
            sql_undo: String::new(),
        }
    }

    #[test]
    fn empty_logminer_poll_preserves_cursor_position() {
        let cursor = logminer::LogMinerCursor {
            scn: 42,
            rs_id: "0x001".to_string(),
            ssn: 7,
        };

        match rows_poll_outcome(&cursor, Vec::new()) {
            PollOutcome::Idle { next_cursor } => {
                assert_eq!(next_cursor, cursor);
            }
            PollOutcome::Rows { .. } => panic!("empty poll must stay idle"),
        }
    }

    #[test]
    fn non_empty_logminer_poll_advances_to_last_ordered_row() {
        let cursor = logminer::LogMinerCursor::new(42);

        match rows_poll_outcome(
            &cursor,
            vec![logminer_row(45, "0x001", 1), logminer_row(45, "0x001", 2)],
        ) {
            PollOutcome::Rows { next_cursor, rows } => {
                assert_eq!(
                    next_cursor,
                    logminer::LogMinerCursor {
                        scn: 45,
                        rs_id: "0x001".to_string(),
                        ssn: 2
                    }
                );
                assert_eq!(rows.len(), 2);
            }
            PollOutcome::Idle { .. } => panic!("non-empty poll must return rows"),
        }
    }
}
