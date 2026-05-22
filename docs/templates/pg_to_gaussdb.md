# Postgres -> GaussDB (PG-compatible) templates

> GaussDB PG-compatible mode is configured as `db_type=gaussdb_pg`.
> It reuses the existing Postgres (pg) snapshot/struct/check sinker paths.
>
> Struct check between `pg` and `gaussdb_pg` uses **logical structure equivalence**
> (cross-engine normalization), instead of requiring `pg_catalog` physical fields to be identical.
> For HA clusters, consider setting env `gaussdb_pg_candidate_hosts` to avoid VIP/LB mixing
> primary/standby during tests.

Refer to [config details](/docs/en/config.md) for explanations of common fields.

# Struct

```
[extractor]
extract_type=struct
db_type=pg
url=postgres://postgres:postgres@127.0.0.1:5433/postgres?options[statement_timeout]=10s
max_connections=10

[sinker]
sink_type=struct
db_type=gaussdb_pg
url=postgres://gaussdb:gaussdb@127.0.0.1:5436/postgres?options[statement_timeout]=10s
conflict_policy=interrupt
max_connections=10

[filter]
do_dbs=test_schema
ignore_dbs=
do_tbs=
ignore_tbs=
do_events=
do_structures=database,table,constraint,sequence,comment,index

[router]
db_map=
tb_map=
col_map=

[runtime]
log_level=info
log4rs_file=./log4rs.yaml
log_dir=./logs

[parallelizer]
parallel_type=serial

[pipeline]
checkpoint_interval_secs=10
buffer_size=100
```

# Snapshot

```
[extractor]
db_type=pg
extract_type=snapshot
url=postgres://postgres:postgres@127.0.0.1:5433/postgres?options[statement_timeout]=10s
batch_size=10000
max_connections=10

[sinker]
db_type=gaussdb_pg
sink_type=write
url=postgres://gaussdb:gaussdb@127.0.0.1:5436/postgres?options[statement_timeout]=10s
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

# Data check

```
[extractor]
db_type=pg
extract_type=snapshot
url=postgres://postgres:postgres@127.0.0.1:5433/postgres?options[statement_timeout]=10s
batch_size=10000

[sinker]
db_type=gaussdb_pg
sink_type=check
url=postgres://gaussdb:gaussdb@127.0.0.1:5436/postgres?options[statement_timeout]=10s
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
