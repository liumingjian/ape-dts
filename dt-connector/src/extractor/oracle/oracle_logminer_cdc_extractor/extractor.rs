use std::{
    sync::{
        atomic::Ordering,
        Arc,
    },
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
        let mut last_scn = self.init_last_scn().await?;

        let mut idle_ticks: u64 = 0;
        loop {
            if self.should_stop()? {
                return Ok(());
            }

            let outcome = self
                .poll_once(&captured, &redo_logs, last_scn, poll_batch_size)
                .await?;
            match outcome {
                PollOutcome::Idle { end_scn } => {
                    idle_ticks += 1;
                    self.maybe_log_idle(idle_ticks, last_scn, captured.len(), "no new scn");
                    last_scn = end_scn;
                    tokio::time::sleep(poll_interval).await;
                }
                PollOutcome::Rows { end_scn, rows } => {
                    idle_ticks = 0;
                    self.process_rows(rows).await?;
                    last_scn = end_scn;
                }
            }
        }
    }

    fn require_username(&self) -> anyhow::Result<String> {
        match &self.client.connection_auth {
            ConnectionAuthConfig::Basic { username, .. } => Ok(username.to_uppercase()),
            ConnectionAuthConfig::NoAuth => bail!("oracle logminer cdc requires basic auth username"),
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
        if self.base_extractor.shut_down.load(Ordering::Acquire) {
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
        last_scn: u64,
        limit: usize,
    ) -> anyhow::Result<PollOutcome> {
        let end_scn = logminer::current_scn(&self.client).await?;
        if end_scn <= last_scn {
            return Ok(PollOutcome::Idle { end_scn });
        }

        let start_scn = last_scn + 1;
        logminer::start_logminer_session(&self.client, redo_logs, start_scn, end_scn).await?;
        let rows = logminer::fetch_logmnr_rows(&self.client, captured, limit).await?;
        logminer::end_logminer_session(&self.client).await?;

        if rows.is_empty() {
            return Ok(PollOutcome::Idle { end_scn });
        }
        Ok(PollOutcome::Rows { end_scn, rows })
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
    Idle { end_scn: u64 },
    Rows {
        end_scn: u64,
        rows: Vec<logminer::LogMinerRow>,
    },
}
