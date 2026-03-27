use std::{collections::HashSet, time::Duration};

use futures::{Sink, Stream};
use url::Url;

use anyhow::bail;
use openssl::ssl::{SslConnector, SslMethod, SslVerifyMode};
use postgres_openssl::MakeTlsConnector;
use postgres_types::PgLsn;
use tokio_postgres::{
    Client, NoTls, SimpleQueryMessage::Row, SimpleQueryRow,
};

use dt_common::{
    config::connection_auth_config::ConnectionAuthConfig, error::Error, log_info, log_warn,
};

pub struct GaussDBCdcClient {
    pub url: String,
    pub connection_auth: ConnectionAuthConfig,
    pub slot_name: String,
    pub start_lsn: String,
    pub recreate_slot_if_exists: bool,
}

impl GaussDBCdcClient {
    fn candidate_endpoints(base_host: &str, base_port: u16) -> Vec<(String, u16)> {
        let mut endpoints = Vec::new();
        let mut seen = HashSet::<String>::new();

        if let Ok(raw) = std::env::var("gaussdb_pg_candidate_hosts") {
            for candidate in raw.split(',').map(|s| s.trim()) {
                if candidate.is_empty() {
                    continue;
                }

                let (host, port) = match candidate.rsplit_once(':') {
                    Some((h, p))
                        if !h.is_empty() && !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) =>
                    {
                        (h.trim(), p.parse::<u16>().unwrap_or(base_port))
                    }
                    _ => (candidate, base_port),
                };

                let key = format!("{}:{}", host, port);
                if seen.insert(key) {
                    endpoints.push((host.to_string(), port));
                }
            }
        }

        let base_key = format!("{}:{}", base_host, base_port);
        if seen.insert(base_key) {
            endpoints.push((base_host.to_string(), base_port));
        }

        endpoints
    }

    fn parse_duration_setting(value: &str) -> Option<Duration> {
        let v = value.trim();
        if v.is_empty() {
            return None;
        }
        if v == "0" {
            return Some(Duration::from_secs(0));
        }

        let mut split_at = 0;
        for (idx, ch) in v.char_indices() {
            if ch.is_ascii_digit() || ch == '.' {
                split_at = idx + ch.len_utf8();
                continue;
            }
            break;
        }
        let (num, unit) = v.split_at(split_at);
        let num: f64 = num.parse().ok()?;
        let unit = unit.trim().to_ascii_lowercase();

        let millis_per_unit = match unit.as_str() {
            "ms" => 1_f64,
            "s" | "" => 1000_f64,
            "min" => 60_000_f64,
            "h" => 3_600_000_f64,
            "d" => 86_400_000_f64,
            _ => return None,
        };

        let millis = (num * millis_per_unit).round();
        if millis.is_sign_negative() {
            return None;
        }
        Some(Duration::from_millis(millis as u64))
    }

    fn first_row_value<'a>(row: &'a SimpleQueryRow, cols: &[&str]) -> Option<&'a str> {
        for col in cols {
            match row.try_get(*col) {
                Ok(Some(v)) => return Some(v),
                Ok(None) => continue,
                Err(_) => continue,
            }
        }
        None
    }

    pub async fn connect(
        &mut self,
    ) -> anyhow::Result<(
        impl Stream<Item = Result<bytes::Bytes, tokio_postgres::Error>>
            + Sink<bytes::Bytes, Error = tokio_postgres::Error>,
        String,
        Option<Duration>,
    )> {
        let url_info = Url::parse(&self.url)?;
        let base_host = url_info.host_str().unwrap().to_string();
        let base_port = url_info
            .port()
            .ok_or_else(|| anyhow::anyhow!("missing port in gaussdb url: {}", self.url))?;
        let dbname = url_info.path().trim_start_matches('/');
        let username = if let ConnectionAuthConfig::Basic { username, .. } = &self.connection_auth {
            username.to_string()
        } else {
            url_info.username().to_string()
        };
        let password = if let ConnectionAuthConfig::Basic {
            password: Some(password),
            ..
        } = &self.connection_auth
        {
            password.to_string()
        } else {
            url_info.password().unwrap_or_default().to_string()
        };
        let prefer_no_ssl = url_info
            .query_pairs()
            .any(|(k, v)| k == "sslmode" && v.eq_ignore_ascii_case("disable"));

        // If `gaussdb_pg_candidate_hosts` is provided (typically in tests), treat it as an HA set
        // and connect to whichever endpoint is currently read-write (pg_is_in_recovery=false).
        let candidates = Self::candidate_endpoints(&base_host, base_port);
        let mut last_err: Option<anyhow::Error> = None;
        for (candidate_host, candidate_base_port) in candidates {
            // GaussDB uses a replication connection (replication=database) for streaming,
            // but may reject running regular SQL on that connection. So we:
            // 1) use a normal SQL connection to manage slot and determine start_lsn
            // 2) use a replication connection only for START_REPLICATION streaming
            let sql_client = match Self::connect_sql(
                &candidate_host,
                candidate_base_port,
                dbname,
                &username,
                &password,
                prefer_no_ssl,
            )
            .await
            {
                Ok(v) => v,
                Err(e) => {
                    log_warn!(
                        "gaussdb cdc candidate sql connect failed: {}:{}, error: {}",
                        candidate_host,
                        candidate_base_port,
                        e
                    );
                    last_err = Some(e);
                    continue;
                }
            };

            let wal_sender_timeout = match self.fetch_wal_sender_timeout(&sql_client).await {
                Ok(v) => v,
                Err(e) => {
                    log_warn!(
                        "gaussdb cdc candidate wal_sender_timeout probe failed: {}:{}, error: {}",
                        candidate_host,
                        candidate_base_port,
                        e
                    );
                    last_err = Some(e);
                    continue;
                }
            };

            if let Err(e) = self.precheck_logical_decoding(&sql_client).await {
                log_warn!(
                    "gaussdb cdc candidate precheck failed: {}:{}, error: {}",
                    candidate_host,
                    candidate_base_port,
                    e
                );
                last_err = Some(e);
                continue;
            }

            let start_lsn = match self.prepare_slot(&sql_client).await {
                Ok(v) => v,
                Err(e) => {
                    log_warn!(
                        "gaussdb cdc candidate slot prepare failed: {}:{}, error: {}",
                        candidate_host,
                        candidate_base_port,
                        e
                    );
                    last_err = Some(e);
                    continue;
                }
            };

            // GaussDB logical replication often requires the HA port (usually base_port + 1). When
            // connecting to a non-HA port, the server may hang instead of returning an error. So we
            // always try HA port first, then fall back to base port with a short timeout.
            let ha_port = candidate_base_port.saturating_add(1);
            let ports = if ha_port != candidate_base_port {
                vec![ha_port, candidate_base_port]
            } else {
                vec![candidate_base_port]
            };

            for (idx, port) in ports.into_iter().enumerate() {
                let connect_and_start = async {
                    let client = Self::connect_replication(
                        &candidate_host,
                        port,
                        dbname,
                        &username,
                        &password,
                        prefer_no_ssl,
                    )
                    .await?;
                    self.start_replication_stream(&client, &start_lsn).await
                };

                // Align with flink-cdc gaussdb connector operational behavior:
                // - Prefer HA port and allow slower startup (connectTimeout=60)
                // - Still keep a short timeout for non-HA fallback to avoid hanging indefinitely
                let timeout = if idx == 0 && port == ha_port {
                    Duration::from_secs(60)
                } else {
                    Duration::from_secs(8)
                };

                match tokio::time::timeout(timeout, connect_and_start).await {
                    Ok(Ok(stream)) => return Ok((stream, start_lsn, wal_sender_timeout)),
                    Ok(Err(e)) => {
                        log_warn!(
                            "gaussdb cdc candidate replication connect/start failed: {}:{}, error: {}",
                            candidate_host,
                            port,
                            e
                        );
                        last_err = Some(e);
                        continue;
                    }
                    Err(_) => {
                        let e = anyhow::anyhow!(
                            "gaussdb replication connect timed out: {}:{}",
                            candidate_host,
                            port
                        );
                        log_warn!("{}", e);
                        last_err = Some(e);
                        continue;
                    }
                }
            }
        }

        Err(last_err.unwrap_or_else(|| {
            anyhow::anyhow!("gaussdb replication connect failed: no candidate endpoint succeeded")
        }))
    }

    async fn fetch_wal_sender_timeout(&self, client: &Client) -> anyhow::Result<Option<Duration>> {
        let res = client.simple_query("SHOW wal_sender_timeout").await?;
        let value = res.iter().find_map(|m| match m {
            Row(row) => Self::first_row_value(row, &["wal_sender_timeout"]).map(|v| v.to_string()),
            _ => None,
        });
        Ok(value.and_then(|v| Self::parse_duration_setting(&v)))
    }

    async fn precheck_logical_decoding(&self, client: &Client) -> anyhow::Result<()> {
        let wal_level = client.simple_query("SHOW wal_level").await?;
        let wal_level = wal_level.iter().find_map(|m| match m {
            Row(row) => Self::first_row_value(row, &["wal_level"]).map(|v| v.to_string()),
            _ => None,
        });

        let wal_level = wal_level.unwrap_or_default();
        if !wal_level.eq_ignore_ascii_case("logical") {
            bail! {Error::ExtractorError(format!(
                "GaussDB logical decoding requires wal_level=logical, but got wal_level={}",
                wal_level
            ))};
        }

        let in_recovery = client
            .simple_query("SELECT pg_is_in_recovery() AS in_recovery")
            .await?;
        let in_recovery = in_recovery.iter().find_map(|m| match m {
            Row(row) => Self::first_row_value(row, &["in_recovery"]).map(|v| v.to_string()),
            _ => None,
        });
        let in_recovery = in_recovery.unwrap_or_default();
        if in_recovery.eq_ignore_ascii_case("t") || in_recovery.eq_ignore_ascii_case("true") {
            bail! {Error::ExtractorError(
                "GaussDB is in recovery/standby mode (pg_is_in_recovery=true), logical decoding is not supported".to_string()
            )};
        }

        Ok(())
    }

    async fn connect_sql(
        host: &str,
        port: u16,
        dbname: &str,
        username: &str,
        password: &str,
        prefer_no_ssl: bool,
    ) -> anyhow::Result<Client> {
        let connect_no_ssl = || async move {
            let conn_info = format!(
                "host={} port={} dbname={} user={} password={} sslmode=disable",
                host, port, dbname, username, password
            );
            let (client, connection) = tokio_postgres::connect(&conn_info, NoTls).await?;
            let host = host.to_string();
            tokio::spawn(async move {
                log_info!(
                    "gaussdb sql connection starts (ssl=off, host={}, port={})",
                    host,
                    port
                );
                if let Err(e) = connection.await {
                    log_info!(
                        "gaussdb sql connection drops (ssl=off, host={}, port={}), error: {}",
                        host,
                        port,
                        e
                    );
                }
            });
            Ok::<_, anyhow::Error>(client)
        };
        let connect_ssl = || async move {
            let conn_info = format!(
                "host={} port={} dbname={} user={} password={} sslmode=require",
                host, port, dbname, username, password
            );

            let mut builder = SslConnector::builder(SslMethod::tls())?;
            builder.set_verify(SslVerifyMode::NONE);
            let connector = MakeTlsConnector::new(builder.build());

            let (client, connection) = tokio_postgres::connect(&conn_info, connector).await?;
            let host = host.to_string();
            tokio::spawn(async move {
                log_info!(
                    "gaussdb sql connection starts (ssl=on, host={}, port={})",
                    host,
                    port
                );
                if let Err(e) = connection.await {
                    log_info!(
                        "gaussdb sql connection drops (ssl=on, host={}, port={}), error: {}",
                        host,
                        port,
                        e
                    );
                }
            });
            Ok::<_, anyhow::Error>(client)
        };

        if prefer_no_ssl {
            match connect_no_ssl().await {
                Ok(v) => Ok(v),
                Err(e) => {
                    if e.to_string().contains("SSL off") {
                        connect_ssl().await
                    } else {
                        Err(e)
                    }
                }
            }
        } else {
            match connect_ssl().await {
                Ok(v) => Ok(v),
                Err(e) => {
                    if e.to_string().contains("SSL on") {
                        connect_no_ssl().await
                    } else {
                        Err(e)
                    }
                }
            }
        }
    }

    async fn connect_replication(
        host: &str,
        port: u16,
        dbname: &str,
        username: &str,
        password: &str,
        prefer_no_ssl: bool,
    ) -> anyhow::Result<Client> {
        let connect_no_ssl = || async move {
            let conn_info = format!(
                "host={} port={} dbname={} user={} password={} replication=database sslmode=disable",
                host, port, dbname, username, password
            );
            let (client, connection) = tokio_postgres::connect(&conn_info, NoTls).await?;
            let host = host.to_string();
            tokio::spawn(async move {
                log_info!(
                    "gaussdb replication connection starts (ssl=off, host={}, port={})",
                    host,
                    port
                );
                if let Err(e) = connection.await {
                    log_info!(
                        "gaussdb replication connection drops (ssl=off, host={}, port={}), error: {}",
                        host,
                        port,
                        e
                    );
                }
            });
            Ok::<_, anyhow::Error>(client)
        };
        let connect_ssl = || async move {
            let conn_info = format!(
                "host={} port={} dbname={} user={} password={} replication=database sslmode=require",
                host, port, dbname, username, password
            );

            let mut builder = SslConnector::builder(SslMethod::tls())?;
            builder.set_verify(SslVerifyMode::NONE);
            let connector = MakeTlsConnector::new(builder.build());

            let (client, connection) = tokio_postgres::connect(&conn_info, connector).await?;
            let host = host.to_string();
            tokio::spawn(async move {
                log_info!(
                    "gaussdb replication connection starts (ssl=on, host={}, port={})",
                    host,
                    port
                );
                if let Err(e) = connection.await {
                    log_info!(
                        "gaussdb replication connection drops (ssl=on, host={}, port={}), error: {}",
                        host,
                        port,
                        e
                    );
                }
            });
            Ok::<_, anyhow::Error>(client)
        };

        if prefer_no_ssl {
            match connect_no_ssl().await {
                Ok(v) => Ok(v),
                Err(e) => {
                    if e.to_string().contains("SSL off") {
                        connect_ssl().await
                    } else {
                        Err(e)
                    }
                }
            }
        } else {
            match connect_ssl().await {
                Ok(v) => Ok(v),
                Err(e) => {
                    if e.to_string().contains("SSL on") {
                        connect_no_ssl().await
                    } else {
                        Err(e)
                    }
                }
            }
        }
    }

    async fn check_slot_status(&self, client: &Client) -> anyhow::Result<(bool, String)> {
        let query = format!(
            "SELECT * FROM {} WHERE slot_name = '{}'",
            "pg_catalog.pg_replication_slots", self.slot_name
        );
        let res = client.simple_query(&query).await?;
        let slot_exists = res.len() > 1;
        log_info!("slot: {} exists: {}", self.slot_name, slot_exists);

        let mut confirmed_flush_lsn = String::new();
        if slot_exists {
            confirmed_flush_lsn = res
                .iter()
                .find_map(|m| match m {
                    Row(row) => Self::first_row_value(
                        row,
                        &["confirmed_flush_lsn", "confirmed_flush", "restart_lsn"],
                    )
                    .map(|v| v.to_string()),
                    _ => None,
                })
                .unwrap_or_default();
            log_info!(
                "slot confirmed_flush_lsn(confirmed_flush/restart_lsn): {}",
                confirmed_flush_lsn
            );
        }

        Ok((slot_exists, confirmed_flush_lsn))
    }

    async fn prepare_slot(&mut self, client: &Client) -> anyhow::Result<String> {
        let mut start_lsn = self.start_lsn.clone();

        let (slot_exists, confirmed_flush_lsn) = self.check_slot_status(client).await?;
        let mut create_slot = !slot_exists;

        if slot_exists {
            if confirmed_flush_lsn.is_empty() {
                create_slot = true;
                log_warn!("slot exists but confirmed_flush_lsn is empty, will recreate slot");
            } else if start_lsn.is_empty() {
                log_warn!("start_lsn is empty, will use confirmed_flush_lsn");
                start_lsn = confirmed_flush_lsn;
            } else {
                match (
                    confirmed_flush_lsn.parse::<PgLsn>(),
                    start_lsn.parse::<PgLsn>(),
                ) {
                    (Ok(actual_lsn), Ok(input_lsn)) => {
                        if input_lsn < actual_lsn {
                            log_warn!(
                                "start_lsn: {} is older than confirmed_flush_lsn: {}, will use confirmed_flush_lsn",
                                start_lsn,
                                confirmed_flush_lsn
                            );
                            start_lsn = confirmed_flush_lsn;
                        }
                    }
                    _ => {
                        log_warn!(
                            "invalid lsn format, start_lsn: {}, confirmed_flush_lsn: {}",
                            start_lsn,
                            confirmed_flush_lsn
                        );
                        start_lsn = confirmed_flush_lsn;
                    }
                }
            }
        }

        if create_slot || self.recreate_slot_if_exists {
            if slot_exists {
                let query = format!(
                    "SELECT {} ('{}')",
                    "pg_drop_replication_slot", self.slot_name
                );
                log_info!("execute: {}", query);
                client.simple_query(&query).await?;
            }

            // GaussDB: create logical replication slot via SQL function.
            let query = format!(
                "SELECT * FROM {}('{}', '{}')",
                "pg_create_logical_replication_slot", self.slot_name, "mppdb_decoding"
            );
            log_info!("execute: {}", query);
            let res = client.simple_query(&query).await?;

            start_lsn = res
                .iter()
                .find_map(|m| match m {
                    Row(row) => Self::first_row_value(row, &["lsn", "xlog_position"])
                        .map(|v| v.to_string()),
                    _ => None,
                })
                .ok_or_else(|| {
                    Error::ExtractorError(format!(
                        "failed to parse start_lsn from pg_create_logical_replication_slot result, query: {}",
                        query
                    ))
                })?;
            log_info!("slot created, returned start_lsn: {}", start_lsn);
        }

        Ok(start_lsn)
    }

    async fn start_replication_stream(
        &self,
        client: &Client,
        start_lsn: &str,
    ) -> anyhow::Result<
        impl Stream<Item = Result<bytes::Bytes, tokio_postgres::Error>>
            + Sink<bytes::Bytes, Error = tokio_postgres::Error>,
    > {
        // Only send replication commands on this connection.
        // Slot options align with the reference flink-cdc gaussdb connector:
        // - include-xids=false: omit transaction ids
        // - skip-empty-xacts=true: skip empty transactions
        let query = format!(
            "START_REPLICATION SLOT {} LOGICAL {} (\"include-xids\" 'false', \"skip-empty-xacts\" 'true')",
            self.slot_name, start_lsn
        );
        log_info!("execute: {}", query);

        let copy_stream = client.copy_both_simple::<bytes::Bytes>(&query).await?;
        Ok(copy_stream)
    }
}
