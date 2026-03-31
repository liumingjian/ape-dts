use std::cmp;
use std::sync::Arc;

use anyhow::bail;
use sqlx::{query, MySql, Pool, Postgres};
use tokio::time::Instant;

use crate::sinker::base_sinker::BaseSinker;
use dt_common::{
    config::config_enums::ConflictPolicyEnum, error::Error, log_error, log_info,
    meta::struct_meta::struct_data::StructData, monitor::monitor::Monitor, rdb_filter::RdbFilter,
    utils::limit_queue::LimitedQueue,
};

pub struct BaseStructSinker {}

pub enum DBConnPool {
    MySQL(Pool<MySql>),
    PostgreSQL(Pool<Postgres>),
}

impl BaseStructSinker {
    pub async fn sink_structs(
        conn_pool: &DBConnPool,
        conflict_policy: &ConflictPolicyEnum,
        data: Vec<StructData>,
        filter: &RdbFilter,
        monitor: &Arc<Monitor>,
        monitor_interval: u64,
    ) -> anyhow::Result<()> {
        let monitor_interval_secs = if monitor_interval > 0 {
            monitor_interval
        } else {
            10
        };
        let mut rts = LimitedQueue::new(cmp::min(100, data.len()));
        let mut last_monitor_time = Instant::now();

        let mut data_len = 0;
        for mut struct_data in data {
            data_len += 1;
            for (_, sql) in struct_data.statement.to_sqls(filter)?.iter() {
                log_info!("ddl begin: {}", sql);
                let start_time = Instant::now();
                match Self::execute(conn_pool, sql).await {
                    Ok(()) => {
                        log_info!("ddl succeed");
                    }

                    Err(error) => {
                        log_error!("ddl failed, error: {}", error);
                        match conflict_policy {
                            ConflictPolicyEnum::Interrupt => bail! {error},
                            ConflictPolicyEnum::Ignore => {}
                        }
                    }
                }
                rts.push((start_time.elapsed().as_millis() as u64, 1));
                if last_monitor_time.elapsed().as_secs() >= monitor_interval_secs {
                    BaseSinker::update_serial_monitor(monitor, data_len as u64, 0).await?;
                    BaseSinker::update_monitor_rt(monitor, &rts).await?;
                    rts.clear();
                    data_len = 0;
                    last_monitor_time = Instant::now();
                }
            }
        }

        if data_len > 0 {
            BaseSinker::update_serial_monitor(monitor, data_len as u64, 0).await?;
            BaseSinker::update_monitor_rt(monitor, &rts).await?;
        }
        Ok(())
    }

    async fn execute(pool: &DBConnPool, sql: &str) -> anyhow::Result<()> {
        match pool {
            DBConnPool::MySQL(pool) => match query(sql).execute(pool).await {
                Ok(_) => Ok(()),
                Err(error) => bail! {Error::SqlxError(error)},
            },
            DBConnPool::PostgreSQL(pool) => {
                let normalized = Self::normalize_pg_ddl(sql);
                match query(&normalized).execute(pool).await {
                    Ok(_) => Ok(()),
                    Err(error) => bail! {Error::SqlxError(error)},
                }
            }
        }
    }

    /// Normalize a few GaussDB-specific Postgres-compatible DDL variants so they can run on
    /// upstream PostgreSQL as-is.
    ///
    /// Example: GaussDB `pg_indexes.indexdef` can include `USING ubtree ... WITH (storage_type=USTORE)`,
    /// which PostgreSQL doesn't support. For cross-engine struct sync (GaussDB -> PG), convert them
    /// into compatible forms.
    fn normalize_pg_ddl(sql: &str) -> String {
        // Fast path: only touch statements that look like GaussDB-only variants.
        if !sql.contains("ubtree") && !sql.contains("USTORE") && !sql.contains("ustore") {
            return sql.to_string();
        }

        let mut out = sql.to_string();

        // `ubtree` is a GaussDB access method; map it to `btree` for upstream Postgres.
        for pat in [
            "USING ubtree",
            "USING UBTREE",
            "using ubtree",
            "using UBTREE",
        ] {
            if out.contains(pat) {
                out = out.replace(pat, "USING btree");
            }
        }

        // Remove GaussDB ustore index parameters that upstream Postgres does not recognize.
        for pat in [
            "WITH (storage_type=USTORE)",
            "WITH (storage_type=ustore)",
            "with (storage_type=USTORE)",
            "with (storage_type=ustore)",
        ] {
            if out.contains(pat) {
                out = out.replace(pat, "");
            }
        }

        out
    }
}
