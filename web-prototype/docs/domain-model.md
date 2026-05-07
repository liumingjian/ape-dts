# ape-dts UI-Relevant Domain Model

This document freezes the user-facing concepts extracted from the Rust backend
(`dt-common`, `dt-task`, `docs/`) so the prototype UI fields are grounded in
real capabilities rather than invention.

> Generated from a codebase exploration at commit `d22f4cce` (branch
> `codex/oracle-gaussdboracle-logminer-cdc`). Re-run the exploration whenever
> the Rust side changes shape.

## 1. Task config schema (INI sections → wizard steps)

| Section | Key fields (example) | Maps to wizard step |
|---|---|---|
| `[runtime]` | `log_level=info`, `log_dir=./logs`, `log4rs_file=./log4rs.yaml` | Implicit (global defaults) |
| `[extractor]` | `db_type`, `extract_type`, `url`, `batch_size`, `parallel_size`, `binlog_filename`, `binlog_position`, `server_id`, `gtid_set` | Step 1 实例来源 |
| `[sinker]` | `db_type`, `sink_type`, `url`, `batch_size`, `replace`, `disable_foreign_key_checks`, `transaction_isolation` | Step 1 实例来源 + Step 3 |
| `[filter]` | `do_dbs=db_1,db_2*`, `ignore_dbs`, `do_tbs=db.tbl_*`, `do_events=insert,update,delete`, `do_ddls=create_table,alter_table`, `ignore_cols` (JSON), `where_conditions` (JSON) | Step 3 迁移对象 + Step 4 数据加工 |
| `[router]` | `db_map=src:dst`, `tb_map=src_db.tbl:dst_db.tbl`, `col_map` (JSON), `topic_map=*.*:topic` | Step 4 数据加工 |
| `[parallelizer]` | `parallel_type=snapshot`, `parallel_size=8` | Step 5 高级设置 |
| `[pipeline]` | `buffer_size=16000`, `buffer_memory_mb=200`, `checkpoint_interval_secs=10`, `max_rps=1000`, `counter_time_window_secs=10` | Step 5 高级设置 |
| `[resumer]` | `resume_type=from_log|from_target|from_db`, `log_dir`, `url`, `table_full_name` | Step 5 高级设置 |
| `[processor]` | `lua_code_file=./script.lua` / `lua_code=...inline...` | Step 4 数据加工 |
| `[global]` | `task_id=cdc_task_1` | Step 1 实例来源 (task name) |
| `[metrics]` | `http_host=127.0.0.1`, `http_port=9090`, `workers=2`, `labels=k1:v1,k2:v2` | Step 5 高级设置 |

File anchors: `dt-common/src/config/extractor_config.rs:37-100`,
`dt-common/src/config/config_enums.rs:169-231`.

## 2. Task types matrix

| Task type | Extract | Sink | Typical source → target |
|---|---|---|---|
| **Snapshot** | `snapshot` | `write` | mysql/pg ↔ mysql/pg; mysql/pg/mongo → kafka/starrocks/clickhouse/doris/tidb |
| **CDC** | `cdc` | `write` | mysql/pg/mongo/redis same-engine; mysql/pg → kafka/starrocks/clickhouse/doris/tidb |
| **Check** | `snapshot` | `check` | mysql/pg/mongo ↔ mysql/pg/mongo (data validation) |
| **Struct** | `struct` | `struct` | mysql/pg → mysql/pg/starrocks/clickhouse/doris |

Anchor: `dt-common/src/config/config_enums.rs:259-307`.

## 3. Connector catalog

| Engine | URL scheme | Notes |
|---|---|---|
| MySQL | `mysql://` | SSL toggle |
| PostgreSQL | `postgres://` | Schema-aware |
| Oracle | `oracle://` | LogMiner-based CDC |
| MongoDB | `mongodb://` | Replica set supported |
| Redis | `redis://` | PSYNC-based, repl_port |
| Kafka | host:port | Topic mapping mandatory |
| StarRocks / ClickHouse / Doris | inherits RDB | Sink-only OLAP targets |
| TiDB | `mysql://` | MySQL wire-compatible |
| GaussDB (Pg/MySQL/Oracle modes) | matches mode | Multi-mode |
| Foxlake | `s3://` | Cloud storage target |

Anchor: `dt-common/src/config/config_enums.rs:18-48`.

## 4. Parallelizer strategies

| Strategy | Applies to | Behavior |
|---|---|---|
| `snapshot` | snapshot (mysql/pg/mongo) | Partition buffered records, one thread per partition |
| `rdb_merge` | CDC (mysql/pg) | Merge DML into insert+delete, parallel write (eventual consistency) |
| `rdb_partition` | Snapshot (mysql/pg) | Split via `partition_cols` |
| `rdb_check` | check tasks | Like snapshot; serial fallback when PK/UK absent |
| `mongo` | CDC (mongo) | Mongo-specific merge |
| `redis` | Redis tasks | Batch or serial per `[sinker] batch_size` |
| `serial` | any | Single-threaded |
| `table` | parallel table extraction | Multiple tables in parallel |

Default: `serial`, `parallel_size=1`. Typical: `snapshot` + `parallel_size=8`.

## 5. Filter & router rules

Filters:
- `do_dbs` / `ignore_dbs` / `do_tbs` / `ignore_tbs` with wildcards `* ?`
- `do_events=insert,update,delete` / `do_ddls=create_table,alter_table`
- `ignore_cols` (JSON per table), `where_conditions` (snapshot WHERE clause JSON)
- `ignore_cmds` (redis command blacklist)

Routers:
- `db_map=test_db:prod_db`
- `tb_map=db.tbl1:db.tbl2` (higher priority than db_map)
- `col_map` (JSON per table)
- `topic_map` (kafka): `*.*:default_topic`, `db.*:topic2`, `db.tb:topic3`

## 6. Resume-from-breakpoint

| `resume_type` | Storage | State recorded |
|---|---|---|
| `from_log` | Local `position.log`, `finished.log` | (db, table, order_col, value) snapshot; binlog pos / LSN / timestamp / change-stream token for CDC |
| `from_target` | `apecloud_metadata.apedts_task_position` in target | Same shape, written to target DB |
| `from_db` | External MySQL/PG | Same shape, isolated store |

Checkpoint interval default 10s. Overhead ≈ 150 bytes × 2 × tables.

## 7. Metrics (Prometheus, optional feature)

Emitted when the `metrics` Cargo feature is enabled; default `/metrics` on
`127.0.0.1:9090`.

| Component | Metric | Unit |
|---|---|---|
| Extractor | `extractor_rps_avg` / `_max` / `_min` | rows/sec |
| Extractor | `extractor_bps_avg` / `_max` / `_min` | bytes/sec |
| Extractor | `extractor_pushed_rps_avg` (post-filter) | rows/sec |
| Sinker | `sinker_rt_per_query_avg` / `_sum` / `_max` | μs |
| Sinker | `sinker_record_count_avg_by_sec` / `_max_by_sec` | rows/sec |
| Sinker | `sinker_bps_avg_by_sec` / `_max_by_sec` | bytes/sec |
| Sinker | `sinker_records_per_query_avg` | rows |
| Pipeline | `pipeline_record_size_avg` | bytes |
| Pipeline | `pipeline_buffer_size_avg` / `_max` | records |
| Pipeline | `pipeline_sinked_count_latest` | cumulative records |

The Dashboard and Task Detail monitoring tab use **these names verbatim** so
time-series axes remain faithful to the real backend.

## 8. Lua ETL hook

Configured via `[processor] lua_code_file=./script.lua` or inline
`lua_code=...`.

Per-row variables:

| Variable | Present | Semantics |
|---|---|---|
| `schema` | always | DB / schema name |
| `tb` | always | Table name |
| `row_type` | always | `"insert"`, `"update"`, `"delete"`, `""` (filter out) |
| `before` | update/delete | Lua table keyed by column name |
| `after` | insert/update | Lua table keyed by column name |

Supported ops: add/drop/modify columns, rename schema/table/column, filter
rows (set `row_type = ""`). Limitations: MySQL/PG sources only; DML only;
binary cells (blob, varbinary) immutable.

---

Mapping summary for the 7-step wizard:

1. **实例来源** → `[extractor]` + `[sinker]` (engine, URL, credentials) + `[global].task_id` + sync-mode cards (snapshot / cdc / snapshot+cdc)
2. **测试连接** → runtime validation of both URLs
3. **选择迁移对象&设置迁移** → `[filter].do_dbs/do_tbs`, conflict strategy (insert/replace/ignore), sync types (struct/data/index), flow-control
4. **数据加工** → per-object `[filter].do_events/where/ignore_cols` + `[router].col_map` + `[processor].lua_code`
5. **高级设置** → `[parallelizer]` + `[pipeline]` + `[resumer]` + `[metrics]`
6. **预检查** → precheck checklist (based on real dt-precheck items)
7. **任务确认** → generated INI preview + start mode (immediate / scheduled)
