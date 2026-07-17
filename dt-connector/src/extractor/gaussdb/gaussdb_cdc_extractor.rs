use std::{
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, UNIX_EPOCH},
};

use anyhow::{bail, Context};
use async_trait::async_trait;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use futures::{Sink, SinkExt, Stream, StreamExt};
use postgres_types::PgLsn;
use sqlx::{Pool, Postgres};
use tokio::{sync::Mutex, time::Instant};

use crate::{
    extractor::{
        base_extractor::BaseExtractor,
        gaussdb::{gaussdb_cdc_client::GaussDBCdcClient, gaussdb_json_decoder::GaussDBJsonDecoder},
        resumer::recovery::Recovery,
    },
    Extractor,
};
use dt_common::{
    config::{config_enums::DbType, connection_auth_config::ConnectionAuthConfig},
    error::Error,
    log_error, log_info, log_warn,
    meta::{dt_data::DtData, position::Position, row_data::RowData, syncer::Syncer},
    rdb_filter::RdbFilter,
    utils::time_util::TimeUtil,
};

const SECS_FROM_1970_TO_2000: i64 = 946_684_800;
const GAUSSDB_STANDBY_STATUS_UPDATE_LEN: usize = 65;
const KEEPALIVE_SEND_TIMEOUT_SECS: u64 = 5;
const MIN_KEEPALIVE_INTERVAL_SECS: u64 = 1;
const WAL_SENDER_TIMEOUT_KEEPALIVE_DIVISOR: u64 = 2;

#[derive(Clone, Copy, Debug)]
enum GaussDBClockUnit {
    MicrosSince2000,
    MillisSince2000,
}

pub struct GaussDBCdcExtractor {
    pub base_extractor: BaseExtractor,
    pub conn_pool: Pool<Postgres>,
    pub filter: RdbFilter,
    pub url: String,
    pub connection_auth: ConnectionAuthConfig,
    pub slot_name: String,
    pub start_lsn: String,
    pub recreate_slot_if_exists: bool,
    pub keepalive_interval_secs: u64,
    pub heartbeat_interval_secs: u64,
    pub heartbeat_tb: String,
    pub syncer: Arc<Mutex<Syncer>>,
    pub recovery: Option<Arc<dyn Recovery + Send + Sync>>,
    // Sticky endpoint to reduce standby probe noise on reconnect.
    // Records the last successful (host, sql_port) endpoint.
    pub last_success_endpoint: Option<(String, u16)>,
}

#[async_trait]
impl Extractor for GaussDBCdcExtractor {
    async fn extract(&mut self) -> anyhow::Result<()> {
        if let Some(recovery) = &self.recovery {
            if let Some(position) = recovery.get_cdc_resume_position().await {
                if let Position::PgCdc { lsn, .. } = &position {
                    self.start_lsn = lsn.to_owned();
                    log_info!("cdc recovery from lsn:[{}]", lsn);
                    self.base_extractor
                        .push_dt_data(DtData::Heartbeat {}, position)
                        .await?;
                } else {
                    log_warn!("position:{} is not a valid pg cdc position", position);
                }
            }
        }

        log_info!(
            "GaussDBCdcExtractor starts, slot_name: {}, start_lsn: {}, keepalive_interval_secs: {}, heartbeat_interval_secs: {}, heartbeat_tb: {}",
            self.slot_name,
            self.start_lsn,
            self.keepalive_interval_secs,
            self.heartbeat_interval_secs,
            self.heartbeat_tb,
        );

        self.extract_internal().await?;
        self.base_extractor.wait_task_finish().await
    }

    async fn close(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

impl GaussDBCdcExtractor {
    async fn extract_internal(&mut self) -> anyhow::Result<()> {
        self.start_heartbeat(self.base_extractor.shut_down.clone())?;

        let decoder = GaussDBJsonDecoder::default();
        let mut last_receive_lsn: Option<PgLsn> = None;
        let mut reconnect_attempt: u64 = 0;
        let mut reconnect_backoff = Duration::from_millis(500);
        let mut allow_slot_recreate = self.recreate_slot_if_exists;

        loop {
            if self.base_extractor.shut_down.load(Ordering::Acquire) {
                return Ok(());
            }

            let mut cdc_client = GaussDBCdcClient {
                url: self.url.clone(),
                connection_auth: self.connection_auth.clone(),
                slot_name: self.slot_name.clone(),
                start_lsn: self.start_lsn.clone(),
                recreate_slot_if_exists: allow_slot_recreate,
                last_success_endpoint: self.last_success_endpoint.clone(),
            };

            let (stream, actual_start_lsn, wal_sender_timeout, selected_endpoint) =
                match cdc_client.connect().await {
                    Ok(v) => v,
                    Err(e) => {
                        reconnect_attempt += 1;
                        log_warn!(
                            "gaussdb cdc connect failed, will retry (attempt {}): {}",
                            reconnect_attempt,
                            e
                        );
                        TimeUtil::sleep_millis(reconnect_backoff.as_millis() as u64).await;
                        let next_ms =
                            std::cmp::min(reconnect_backoff.as_millis() as u64 * 2, 10_000);
                        reconnect_backoff = Duration::from_millis(next_ms);
                        continue;
                    }
                };

            self.last_success_endpoint = Some(selected_endpoint);
            allow_slot_recreate = false;
            reconnect_attempt = 0;
            reconnect_backoff = Duration::from_millis(500);

            tokio::pin!(stream);

            let start_lsn: PgLsn = actual_start_lsn.parse().map_err(|_| {
                anyhow::anyhow!("invalid start_lsn from slot: {}", actual_start_lsn)
            })?;
            let mut last_receive_lsn_session = last_receive_lsn
                .map(|v| std::cmp::max(v, start_lsn))
                .unwrap_or(start_lsn);

            let keepalive_interval_secs = Self::effective_keepalive_interval_secs(
                self.keepalive_interval_secs,
                wal_sender_timeout,
            );
            if let Some(timeout) = wal_sender_timeout {
                if self.keepalive_interval_secs > 0 {
                    if keepalive_interval_secs < self.keepalive_interval_secs {
                        log_info!(
                            "gaussdb wal_sender_timeout={}s is smaller than keepalive_interval_secs={}, adjusting keepalive_interval_secs to {}s",
                            timeout.as_secs(),
                            self.keepalive_interval_secs,
                            keepalive_interval_secs
                        );
                    } else {
                        log_info!(
                            "gaussdb wal_sender_timeout={}s, keepalive_interval_secs={}s",
                            timeout.as_secs(),
                            keepalive_interval_secs
                        );
                    }
                } else {
                    log_info!(
                        "gaussdb wal_sender_timeout={}s, periodic keepalive disabled",
                        timeout.as_secs(),
                    );
                }
            }

            let mut keepalive_ticker = if keepalive_interval_secs > 0 {
                let mut ticker =
                    tokio::time::interval(Duration::from_secs(keepalive_interval_secs));
                ticker.tick().await;
                Some(ticker)
            } else {
                None
            };

            let mut server_clock_unit: Option<GaussDBClockUnit> = None;

            tokio::time::sleep(Duration::from_millis(10)).await;
            let send_res = self
                .send_keepalive_status_update(
                    &mut stream,
                    start_lsn,
                    last_receive_lsn_session,
                    true,
                    server_clock_unit,
                )
                .await;
            if let Err(e) = send_res {
                log_warn!(
                    "gaussdb keepalive status update send failed, will reconnect: {}",
                    e
                );
                last_receive_lsn = Some(last_receive_lsn_session);
                continue;
            }

            loop {
                if self.base_extractor.shut_down.load(Ordering::Acquire) {
                    let _ = self
                        .send_keepalive_status_update(
                            &mut stream,
                            start_lsn,
                            last_receive_lsn_session,
                            true,
                            server_clock_unit,
                        )
                        .await;
                    return Ok(());
                }

                if !self.base_extractor.time_filter.ended
                    && self.base_extractor.time_filter.end_timestamp != u32::MAX
                {
                    let now_sec = UNIX_EPOCH.elapsed()?.as_secs() as u32;
                    if now_sec >= self.base_extractor.time_filter.end_timestamp {
                        self.base_extractor.time_filter.ended = true;
                    }
                }
                if self.base_extractor.time_filter.ended {
                    let _ = self
                        .send_keepalive_status_update(
                            &mut stream,
                            start_lsn,
                            last_receive_lsn_session,
                            true,
                            server_clock_unit,
                        )
                        .await;
                    return Ok(());
                }

                let next = if let Some(ticker) = keepalive_ticker.as_mut() {
                    tokio::select! {
                        v = stream.next() => v,
                        _ = ticker.tick() => {
                            log_info!("gaussdb keepalive tick: sending status update");
                            if let Err(e) = self.send_keepalive_status_update(
                                &mut stream,
                                start_lsn,
                                last_receive_lsn_session,
                                true,
                                server_clock_unit,
                            ).await {
                                log_warn!("gaussdb keepalive status update failed, will reconnect: {}", e);
                                break;
                            }
                            continue;
                        }
                    }
                } else {
                    stream.next().await
                };

                match next {
                    Some(Ok(copy_data)) => match Self::parse_replication_msg(copy_data)? {
                        GaussDBReplicationMessage::XLogData {
                            wal_end,
                            timestamp,
                            data,
                        } => {
                            last_receive_lsn_session = PgLsn::from(wal_end);
                            let lsn = last_receive_lsn_session.to_string();
                            let ts_micro = timestamp;
                            let position = Position::PgCdc {
                                lsn: lsn.clone(),
                                timestamp: Position::format_timestamp_millis(
                                    ts_micro / 1000 + SECS_FROM_1970_TO_2000 * 1000,
                                ),
                            };

                            let ts_sec = (ts_micro / 1_000_000 + SECS_FROM_1970_TO_2000) as u32;
                            BaseExtractor::update_time_filter(
                                &mut self.base_extractor.time_filter,
                                ts_sec,
                                &position,
                            );

                            let data = std::str::from_utf8(data.as_ref()).with_context(|| {
                                format!("invalid utf8 wal data at lsn: {}", lsn)
                            })?;

                            for line in data.lines() {
                                if line.trim().is_empty() {
                                    continue;
                                }
                                match decoder.decode_message(line) {
                                    Ok(items) => {
                                        for item in items {
                                            match item {
                                                DtData::Begin { .. } => {}
                                                DtData::Dml { row_data } => {
                                                    if self.filter_row(&row_data) {
                                                        self.base_extractor
                                                            .record_extracted_metrics(
                                                                1,
                                                                row_data.data_size as u64,
                                                            );
                                                        continue;
                                                    }
                                                    self.base_extractor
                                                        .push_row(row_data, position.clone())
                                                        .await?;
                                                }
                                                other => {
                                                    self.base_extractor
                                                        .push_dt_data(other, position.clone())
                                                        .await?;
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        let category = Self::classify_decode_error(&e);
                                        let raw_snippet = Self::truncate_for_log(line, 200);
                                        log_error!(
                                            "Failed to decode mppdb_decoding message at lsn {}: category={} error={} raw_snippet={}",
                                            lsn,
                                            category,
                                            e,
                                            raw_snippet
                                        );
                                        bail! {Error::ExtractorError(format!(
                                            "mppdb_decoding json decode failed (category={}): {}",
                                            category,
                                            e
                                        ))};
                                    }
                                }
                            }
                        }
                        GaussDBReplicationMessage::PrimaryKeepAlive {
                            server_lsn,
                            clock,
                            reply_requested,
                        } => {
                            let server_lsn = PgLsn::from(server_lsn);
                            if server_lsn > last_receive_lsn_session {
                                last_receive_lsn_session = server_lsn;
                            }
                            if server_clock_unit.is_none() {
                                server_clock_unit = Some(Self::infer_clock_unit(clock));
                                log_info!(
                                    "gaussdb replication keepalive clock unit inferred: {:?} (clock={})",
                                    server_clock_unit.unwrap(),
                                    clock
                                );
                            }
                            if reply_requested {
                                log_info!(
                                    "gaussdb keepalive reply requested: server_lsn={}",
                                    server_lsn
                                );
                                if let Err(e) = self
                                    .send_keepalive_status_update(
                                        &mut stream,
                                        start_lsn,
                                        last_receive_lsn_session,
                                        false,
                                        server_clock_unit,
                                    )
                                    .await
                                {
                                    log_warn!(
                                        "gaussdb keepalive reply send failed, will reconnect: {}",
                                        e
                                    );
                                    break;
                                }
                            }
                        }
                        GaussDBReplicationMessage::Unknown { tag, len } => {
                            log_info!("received unknown replication data: tag={} len={}", tag, len);
                        }
                    },
                    Some(Err(e)) => {
                        if self.base_extractor.shut_down.load(Ordering::Acquire) {
                            return Ok(());
                        }
                        log_warn!("gaussdb replication stream closed, will reconnect: {}", e);
                        break;
                    }
                    None => {
                        log_warn!("gaussdb replication stream ended, will reconnect");
                        break;
                    }
                }
            }

            last_receive_lsn = Some(last_receive_lsn_session);
            reconnect_attempt += 1;
            log_warn!(
                "gaussdb replication disconnected, will reconnect in {} ms (attempt {})",
                reconnect_backoff.as_millis(),
                reconnect_attempt
            );
            TimeUtil::sleep_millis(reconnect_backoff.as_millis() as u64).await;
            let next_ms = std::cmp::min(reconnect_backoff.as_millis() as u64 * 2, 10_000);
            reconnect_backoff = Duration::from_millis(next_ms);
            continue;
        }
    }

    async fn send_keepalive_status_update<S>(
        &mut self,
        stream: &mut Pin<&mut S>,
        start_lsn: PgLsn,
        last_receive_lsn: PgLsn,
        force_reply: bool,
        server_clock_unit: Option<GaussDBClockUnit>,
    ) -> anyhow::Result<()>
    where
        S: Stream<Item = Result<Bytes, tokio_postgres::Error>>
            + Sink<Bytes, Error = tokio_postgres::Error>,
    {
        let flush_lsn = self.current_flush_lsn(start_lsn).await;
        let receive_lsn = if last_receive_lsn > flush_lsn {
            last_receive_lsn
        } else {
            flush_lsn
        };

        // Postgres epoch is 2000-01-01T00:00:00Z
        let pg_epoch = UNIX_EPOCH + Duration::from_secs(SECS_FROM_1970_TO_2000 as u64);
        let clock = match server_clock_unit.unwrap_or(GaussDBClockUnit::MicrosSince2000) {
            GaussDBClockUnit::MicrosSince2000 => pg_epoch.elapsed()?.as_micros() as i64,
            GaussDBClockUnit::MillisSince2000 => pg_epoch.elapsed()?.as_millis() as i64,
        };

        let msg = Self::build_gaussdb_standby_status_update(
            receive_lsn,
            flush_lsn,
            flush_lsn,
            clock,
            force_reply,
        )?;
        tokio::time::timeout(
            Duration::from_secs(KEEPALIVE_SEND_TIMEOUT_SECS),
            stream.send(msg),
        )
        .await
        .with_context(|| {
            format!(
                "keepalive status update to gaussdb timed out after {}s, receive_lsn: {}, flush_lsn: {}",
                KEEPALIVE_SEND_TIMEOUT_SECS, receive_lsn, flush_lsn
            )
        })?
        .with_context(|| {
            format!(
                "keepalive status update to gaussdb failed, receive_lsn: {}, flush_lsn: {}",
                receive_lsn, flush_lsn
            )
        })?;
        Ok(())
    }

    fn effective_keepalive_interval_secs(
        configured_secs: u64,
        wal_sender_timeout: Option<Duration>,
    ) -> u64 {
        let Some(timeout) = wal_sender_timeout else {
            return configured_secs;
        };
        if configured_secs == 0 {
            return 0;
        }

        let timeout_secs = timeout.as_secs();
        if timeout_secs == 0 {
            return configured_secs;
        }

        let recommended = std::cmp::max(
            MIN_KEEPALIVE_INTERVAL_SECS,
            timeout_secs / WAL_SENDER_TIMEOUT_KEEPALIVE_DIVISOR,
        );
        std::cmp::min(configured_secs, recommended)
    }

    async fn current_flush_lsn(&self, start_lsn: PgLsn) -> PgLsn {
        let committed_position = self.syncer.lock().await.committed_position.clone();
        match committed_position {
            Position::PgCdc { lsn, .. } if !lsn.is_empty() => match lsn.parse::<PgLsn>() {
                Ok(v) => v,
                Err(_) => {
                    log_warn!(
                        "invalid committed lsn in syncer: {}, will use start_lsn",
                        lsn
                    );
                    start_lsn
                }
            },
            _ => start_lsn,
        }
    }

    fn build_gaussdb_standby_status_update(
        received: PgLsn,
        flushed: PgLsn,
        applied: PgLsn,
        clock: i64,
        force_reply: bool,
    ) -> anyhow::Result<Bytes> {
        // Message format is GaussDB-specific. It matches the official JDBC driver (`gsjdbc4.jar`)
        // implementation (`org.postgresql.core.v3.replication.V3PGReplicationStream`):
        // - total 65 bytes
        // - little-endian
        // - extra fields compared to PostgreSQL standby_status_update
        let received_u64: u64 = received.into();
        let flushed_u64: u64 = flushed.into();
        let applied_u64: u64 = applied.into();
        // In the driver, reply is requested when:
        // - forceUpdateStatus() is used; OR
        // - received LSN is INVALID_LSN (0)
        let reply_requested = force_reply || received_u64 == 0;

        let mut buf = BytesMut::with_capacity(GAUSSDB_STANDBY_STATUS_UPDATE_LEN);
        buf.put_u8(b'r');
        buf.put_i64_le(i64::MAX);
        buf.put_u64_le(received_u64);
        buf.put_u64_le(flushed_u64);
        buf.put_i64_le(i64::MAX);
        buf.put_u64_le(applied_u64);
        buf.put_i32_le(i32::MAX);
        buf.put_i32_le(i32::MAX);
        buf.put_i64_le(clock);
        buf.put_u8(if reply_requested { 1 } else { 0 });
        buf.put_i32_le(0);
        buf.put_u8(1);
        buf.put_u8(1);
        buf.put_u8(1);

        if buf.len() != GAUSSDB_STANDBY_STATUS_UPDATE_LEN {
            bail! {Error::ExtractorError(format!(
                "gaussdb standby status update length mismatch: expected {}, got {}",
                GAUSSDB_STANDBY_STATUS_UPDATE_LEN,
                buf.len()
            ))};
        }
        Ok(buf.freeze())
    }

    fn infer_clock_unit(clock: i64) -> GaussDBClockUnit {
        // As of 2026:
        // - microseconds since 2000-01-01 is ~8e14
        // - milliseconds since 2000-01-01 is ~8e11
        // Use a coarse threshold to infer what GaussDB sends on the wire.
        let abs = clock.wrapping_abs() as u64;
        if abs > 10_000_000_000_000_u64 {
            GaussDBClockUnit::MicrosSince2000
        } else {
            GaussDBClockUnit::MillisSince2000
        }
    }

    fn parse_replication_msg(copy_data: Bytes) -> anyhow::Result<GaussDBReplicationMessage> {
        if copy_data.is_empty() {
            return Ok(GaussDBReplicationMessage::Unknown {
                tag: "empty".to_string(),
                len: 0,
            });
        }

        let mut buf = copy_data;
        let tag = buf.get_u8();
        match tag {
            b'w' => {
                if buf.remaining() < 8 + 8 + 8 {
                    bail! {Error::ExtractorError(format!(
                        "invalid xlogdata message: len={} < {}",
                        buf.remaining() + 1,
                        1 + 8 + 8 + 8
                    ))};
                }
                let _wal_start = buf.get_u64();
                let wal_end = buf.get_u64();
                let timestamp = buf.get_i64();
                let data = buf.copy_to_bytes(buf.remaining());
                Ok(GaussDBReplicationMessage::XLogData {
                    wal_end,
                    timestamp,
                    data,
                })
            }
            b'k' => {
                if buf.remaining() < 8 + 4 + 4 + 8 + 1 {
                    bail! {Error::ExtractorError(format!(
                        "invalid keepalive message: len={} < {}",
                        buf.remaining() + 1,
                        1 + 8 + 4 + 4 + 8 + 1
                    ))};
                }
                let server_lsn = buf.get_u64_le();
                let _ = buf.get_i32_le();
                let _ = buf.get_i32_le();
                let clock = buf.get_i64_le();
                let reply_requested = buf.get_u8() != 0;
                Ok(GaussDBReplicationMessage::PrimaryKeepAlive {
                    server_lsn,
                    clock,
                    reply_requested,
                })
            }
            other => Ok(GaussDBReplicationMessage::Unknown {
                tag: format!("0x{:02X}", other),
                len: buf.remaining() + 1,
            }),
        }
    }

    fn filter_row(&mut self, row_data: &RowData) -> bool {
        let schema = &row_data.schema;
        let tb = &row_data.tb;
        let filtered = self.filter.filter_event(schema, tb, &row_data.row_type);
        if filtered {
            return !self.base_extractor.is_data_marker_info(schema, tb);
        }
        filtered
    }

    fn start_heartbeat(&mut self, shut_down: Arc<AtomicBool>) -> anyhow::Result<()> {
        let schema_tb = self.base_extractor.precheck_heartbeat(
            self.heartbeat_interval_secs,
            &self.heartbeat_tb,
            DbType::GaussDBPg,
        );
        if schema_tb.len() != 2 {
            return Ok(());
        }

        self.filter.add_ignore_tb(&schema_tb[0], &schema_tb[1]);

        let (slot_name, heartbeat_interval_secs, syncer, conn_pool) = (
            self.slot_name.clone(),
            self.heartbeat_interval_secs,
            self.syncer.clone(),
            self.conn_pool.clone(),
        );
        tokio::spawn(async move {
            let mut start_time = Instant::now();
            while !shut_down.load(Ordering::Acquire) {
                if start_time.elapsed().as_secs() >= heartbeat_interval_secs {
                    Self::heartbeat(
                        &slot_name,
                        &schema_tb[0],
                        &schema_tb[1],
                        &syncer,
                        &conn_pool,
                    )
                    .await
                    .unwrap_or_else(|e| log_warn!("gaussdb heartbeat failed: {}", e));
                    start_time = Instant::now();
                }
                TimeUtil::sleep_millis(1000 * heartbeat_interval_secs).await;
            }
        });
        log_info!("heartbeat started");
        Ok(())
    }

    async fn heartbeat(
        slot_name: &str,
        schema: &str,
        tb: &str,
        syncer: &Arc<Mutex<Syncer>>,
        conn_pool: &Pool<Postgres>,
    ) -> anyhow::Result<()> {
        let (received_lsn, received_timestamp) =
            if let Position::PgCdc { lsn, timestamp } = &syncer.lock().await.received_position {
                (lsn.clone(), timestamp.clone())
            } else {
                (String::new(), String::new())
            };

        let (flushed_lsn, flushed_timestamp) =
            if let Position::PgCdc { lsn, timestamp } = &syncer.lock().await.committed_position {
                (lsn.clone(), timestamp.clone())
            } else {
                (String::new(), String::new())
            };

        // GaussDB does not reliably support Postgres `ON CONFLICT` syntax.
        // Use UPDATE-first then INSERT fallback.
        let update_sql = format!(
            r#"UPDATE "{}"."{}"
               SET update_timestamp = now(),
                   received_lsn = $1,
                   received_timestamp = $2,
                   flushed_lsn = $3,
                   flushed_timestamp = $4
               WHERE slot_name = $5"#,
            schema, tb
        );
        let updated = sqlx::query(&update_sql)
            .bind(&received_lsn)
            .bind(&received_timestamp)
            .bind(&flushed_lsn)
            .bind(&flushed_timestamp)
            .bind(slot_name)
            .execute(conn_pool)
            .await?;
        if updated.rows_affected() == 0 {
            let insert_sql = format!(
                r#"INSERT INTO "{}"."{}" (slot_name, update_timestamp, received_lsn, received_timestamp, flushed_lsn, flushed_timestamp)
                   VALUES ($1, now(), $2, $3, $4, $5)"#,
                schema, tb
            );
            sqlx::query(&insert_sql)
                .bind(slot_name)
                .bind(&received_lsn)
                .bind(&received_timestamp)
                .bind(&flushed_lsn)
                .bind(&flushed_timestamp)
                .execute(conn_pool)
                .await?;
        }
        Ok(())
    }

    fn classify_decode_error(err: &anyhow::Error) -> &'static str {
        let msg = err.to_string();
        if msg.contains("unsupported op_type") {
            return "unsupported_op_type";
        }
        if msg.contains("missing field") {
            return "missing_field";
        }
        if msg.contains("failed to parse mppdb_decoding json") || msg.contains("invalid escape") {
            return "json_parse";
        }
        "unknown"
    }

    fn truncate_for_log(s: &str, max_bytes: usize) -> String {
        if s.len() <= max_bytes {
            return s.to_string();
        }

        let mut end = 0;
        for (idx, ch) in s.char_indices() {
            if idx + ch.len_utf8() > max_bytes {
                break;
            }
            end = idx + ch.len_utf8();
        }
        format!("{}...(truncated,len={})", &s[..end], s.len())
    }
}

enum GaussDBReplicationMessage {
    XLogData {
        wal_end: u64,
        timestamp: i64,
        data: Bytes,
    },
    PrimaryKeepAlive {
        server_lsn: u64,
        clock: i64,
        reply_requested: bool,
    },
    Unknown {
        tag: String,
        len: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_gaussdb_standby_status_update_matches_gsjdbc4_layout() {
        let received = PgLsn::from(0x0102030405060708_u64);
        let flushed = PgLsn::from(0x1112131415161718_u64);
        let applied = PgLsn::from(0x2122232425262728_u64);
        let clock = 0x0A0B0C0D0E0F1011_i64;

        let msg = GaussDBCdcExtractor::build_gaussdb_standby_status_update(
            received, flushed, applied, clock, true,
        )
        .unwrap();

        assert_eq!(msg.len(), GAUSSDB_STANDBY_STATUS_UPDATE_LEN);
        assert_eq!(msg[0], b'r');
        assert_eq!(&msg[1..9], &i64::MAX.to_le_bytes());
        assert_eq!(&msg[9..17], &0x0102030405060708_u64.to_le_bytes());
        assert_eq!(&msg[17..25], &0x1112131415161718_u64.to_le_bytes());
        assert_eq!(&msg[25..33], &i64::MAX.to_le_bytes());
        assert_eq!(&msg[33..41], &0x2122232425262728_u64.to_le_bytes());
        assert_eq!(&msg[41..45], &i32::MAX.to_le_bytes());
        assert_eq!(&msg[45..49], &i32::MAX.to_le_bytes());
        assert_eq!(&msg[49..57], &clock.to_le_bytes());
        assert_eq!(msg[57], 1); // forceUpdateStatus() => request reply
        assert_eq!(&msg[58..62], &0_i32.to_le_bytes());
        assert_eq!(msg[62], 1);
        assert_eq!(msg[63], 1);
        assert_eq!(msg[64], 1);

        let msg = GaussDBCdcExtractor::build_gaussdb_standby_status_update(
            PgLsn::from(0_u64),
            PgLsn::from(0_u64),
            PgLsn::from(0_u64),
            0,
            false,
        )
        .unwrap();
        assert_eq!(msg[57], 1); // INVALID_LSN(0) => request reply

        let msg = GaussDBCdcExtractor::build_gaussdb_standby_status_update(
            PgLsn::from(1_u64),
            PgLsn::from(1_u64),
            PgLsn::from(1_u64),
            0,
            false,
        )
        .unwrap();
        assert_eq!(msg[57], 0);
    }

    #[test]
    fn parse_replication_keepalive_is_little_endian() {
        let mut buf = BytesMut::new();
        buf.put_u8(b'k');
        buf.put_u64_le(0x0102030405060708_u64);
        buf.put_i32_le(1);
        buf.put_i32_le(2);
        buf.put_i64_le(3);
        buf.put_u8(1);

        match GaussDBCdcExtractor::parse_replication_msg(buf.freeze()).unwrap() {
            GaussDBReplicationMessage::PrimaryKeepAlive {
                server_lsn,
                clock,
                reply_requested,
            } => {
                assert_eq!(server_lsn, 0x0102030405060708_u64);
                assert_eq!(clock, 3);
                assert!(reply_requested);
            }
            other => panic!("unexpected message: {:?}", std::mem::discriminant(&other)),
        }
    }

    #[test]
    fn parse_replication_xlogdata_is_big_endian() {
        let mut buf = BytesMut::new();
        buf.put_u8(b'w');
        buf.put_u64(0x0102030405060708_u64);
        buf.put_u64(0x1112131415161718_u64);
        buf.put_i64(0x0A0B0C0D0E0F1011_i64);
        buf.put_slice(b"payload");

        match GaussDBCdcExtractor::parse_replication_msg(buf.freeze()).unwrap() {
            GaussDBReplicationMessage::XLogData {
                wal_end,
                timestamp,
                data,
            } => {
                assert_eq!(wal_end, 0x1112131415161718_u64);
                assert_eq!(timestamp, 0x0A0B0C0D0E0F1011_i64);
                assert_eq!(data, Bytes::from_static(b"payload"));
            }
            other => panic!("unexpected message: {:?}", std::mem::discriminant(&other)),
        }
    }

    #[test]
    fn gaussdb_keepalive_interval_respects_wal_sender_timeout() {
        assert_eq!(
            GaussDBCdcExtractor::effective_keepalive_interval_secs(
                10,
                Some(Duration::from_secs(6)),
            ),
            3
        );
        assert_eq!(
            GaussDBCdcExtractor::effective_keepalive_interval_secs(2, Some(Duration::from_secs(6)),),
            2
        );
        assert_eq!(
            GaussDBCdcExtractor::effective_keepalive_interval_secs(
                10,
                Some(Duration::from_millis(500)),
            ),
            10
        );
        assert_eq!(
            GaussDBCdcExtractor::effective_keepalive_interval_secs(0, Some(Duration::from_secs(6)),),
            0
        );
        assert_eq!(
            GaussDBCdcExtractor::effective_keepalive_interval_secs(10, None),
            10
        );
    }
}
