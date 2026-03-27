# GaussDB (PG-compatible) -> Postgres templates

> GaussDB PG-compatible mode is configured as `db_type=gaussdb_pg`.
> For CDC, ape-dts uses GaussDB logical decoding plugin `mppdb_decoding` (JSON output).
>
> For HA clusters, you can set env `gaussdb_pg_candidate_hosts` (comma-separated `host[:port]`) so
> the CDC client can auto-select a read-write primary (`pg_is_in_recovery=false`) and prefer HA
> port (`port+1`) for replication streaming.

Refer to [config details](/docs/en/config.md) for explanations of common fields.

# Snapshot

```
[extractor]
db_type=gaussdb_pg
extract_type=snapshot
url=postgres://gaussdb:gaussdb@127.0.0.1:5436/postgres?options[statement_timeout]=10s
batch_size=10000
max_connections=10

[sinker]
db_type=pg
sink_type=write
url=postgres://postgres:postgres@127.0.0.1:5433/postgres?options[statement_timeout]=10s
batch_size=200
replace=true
max_connections=10

[filter]
do_dbs=
ignore_dbs=
do_tbs=test_schema.a,test_schema.b
ignore_tbs=
do_events=insert

[router]
db_map=
tb_map=
col_map=

[parallelizer]
parallel_type=snapshot
parallel_size=8

[pipeline]
buffer_size=16000
checkpoint_interval_secs=10

[runtime]
log_level=info
log4rs_file=./log4rs.yaml
log_dir=./logs
```

# CDC (mppdb_decoding / JSON)

```
[extractor]
db_type=gaussdb_pg
extract_type=cdc
url=postgres://gaussdb:gaussdb@127.0.0.1:5436/postgres?options[statement_timeout]=10s

# required
slot_name=ape_test_gaussdb

# optional (empty means auto from slot confirmed_flush_lsn / restart_lsn)
start_lsn=

# recommended: reuse slot for HA/restarts; set true only when you explicitly want to reset slot
recreate_slot_if_exists=false
keepalive_interval_secs=10
heartbeat_interval_secs=1
heartbeat_tb=heartbeat_db.ape_dts_heartbeat
start_time_utc=
end_time_utc=

[filter]
do_dbs=
ignore_dbs=
do_tbs=test_schema.a,test_schema.b
ignore_tbs=
do_events=insert,update,delete

[sinker]
db_type=pg
sink_type=write
url=postgres://postgres:postgres@127.0.0.1:5433/postgres?options[statement_timeout]=10s
batch_size=200
replace=true
max_connections=10

[router]
tb_map=
col_map=
db_map=

[parallelizer]
parallel_type=rdb_merge
parallel_size=8

[pipeline]
buffer_size=16000
checkpoint_interval_secs=1

[runtime]
log_dir=./logs
log_level=info
log4rs_file=./log4rs.yaml
```

- [extractor]

| Config                  | Description                                                 | Example                   | Default |
| :---------------------- | :---------------------------------------------------------- | :------------------------ | :------ |
| slot_name               | logical replication slot name (plugin: mppdb_decoding)       | ape_test_gaussdb          | -       |
| start_lsn               | starting LSN, empty means use slot confirmed_flush_lsn/restart_lsn | 0/406DE430           | empty   |
| recreate_slot_if_exists | whether to drop+recreate slot if it already exists (may require re-snapshot) | false | false   |
| keepalive_interval_secs | keepalive interval (StandbyStatusUpdate)                     | 10                        | 10      |
| heartbeat_interval_secs | heartbeat write interval                                     | 1                         | 10      |
| heartbeat_tb            | heartbeat table name                                         | heartbeat_db.ape_dts_heartbeat | empty |
| start_time_utc          | optional start time (UTC) to bound the stream                | 2025-01-01 00:00:00       | empty   |
| end_time_utc            | optional end time (UTC) to stop the task                     | 2025-01-01 01:00:00       | empty   |

Notes:

- If `wal_sender_timeout` is smaller than `keepalive_interval_secs`, ape-dts may auto-adjust the
  keepalive interval to avoid server-side timeout.

# Data check

```
[extractor]
db_type=gaussdb_pg
extract_type=snapshot
url=postgres://gaussdb:gaussdb@127.0.0.1:5436/postgres?options[statement_timeout]=10s
batch_size=10000

[sinker]
db_type=pg
sink_type=check
url=postgres://postgres:postgres@127.0.0.1:5433/postgres?options[statement_timeout]=10s
batch_size=200

[filter]
do_dbs=
ignore_dbs=
do_tbs=test_schema.a,test_schema.b
ignore_tbs=
do_events=insert

[router]
db_map=
tb_map=
col_map=

[parallelizer]
parallel_type=rdb_check
parallel_size=8

[pipeline]
buffer_size=16000
checkpoint_interval_secs=10

[runtime]
log_level=info
log4rs_file=./log4rs.yaml
log_dir=./logs
```
