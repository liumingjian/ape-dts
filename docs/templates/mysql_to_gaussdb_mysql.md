# MySQL -> GaussDB (MySQL-compatible) templates

> GaussDB MySQL-compatible mode is configured as `db_type=gaussdb_mysql`.
> Current delivery is **target-first** only and has been validated for:
>
> - `snapshot`
> - `struct`
> - `check`
> - `cdc` (DML only)
>
> Scope guard:
>
> - supported in this phase: `MySQL -> GaussDBMySQL`
> - not in this phase: `GaussDBMySQL -> MySQL`
> - CDC boundary: DML only (no DDL CDC)
>
> Recommended local test contract:
>
> - source MySQL: local Docker
> - sink GaussDB MySQL-compatible database: set in `dt-tests/tests/.env.local`
>   - `gaussdb_mysql_sinker_without_auth_url`
>   - `gaussdb_mysql_sinker_username`
>   - `gaussdb_mysql_sinker_password`

> Important environment note:
>
> - In the current HCS environment, GaussDB compatibility mode is selected at **database creation time**, not by changing the listener protocol alone.
> - A validated example is `postgres://root@10.250.0.51:8000/jyp_test_m?sslmode=require`, where:
>   - `current_database()` = `jyp_test_m`
>   - `SHOW sql_compatibility;` = `M`
> - This means “GaussDB MySQL-compatible mode” may still be accessed through a `postgres://` endpoint.
> - Therefore, **wire protocol** and **SQL compatibility mode** must be modeled separately.
> - The current validated bootstrap implementation uses:
>   - `mysql://...` for the source MySQL 8 extractor
>   - `postgres://.../<mysql-compatible-db>` for the target GaussDB MySQL-compatible database

Refer to [config details](/docs/en/config.md) for explanations of common fields.

# Local MySQL 8 Source Setup

Recommended local source for `dt-tests`:

```bash
docker rm -f ape-dts-mysql8 >/dev/null 2>&1 || true
docker run -d --name ape-dts-mysql8 \
  -e MYSQL_ROOT_PASSWORD=123456 \
  -e MYSQL_DATABASE=test_db \
  -p 3311:3306 \
  mysql:8.0 \
  --server-id=11 \
  --log-bin=mysql-bin \
  --binlog-format=ROW \
  --binlog-row-image=FULL \
  --gtid-mode=ON \
  --enforce-gtid-consistency=ON \
  --log-slave-updates=ON \
  --default-authentication-plugin=mysql_native_password \
  --character-set-server=utf8mb4 \
  --collation-server=utf8mb4_unicode_ci

docker exec ape-dts-mysql8 \
  mysql -uroot -p123456 -e "SELECT VERSION();"
```

The current validated local source contract is:

- image: `mysql:8.0`
- port: `3311`
- username: `root`
- password: `123456`
- binlog prerequisites: `ROW` + `FULL` + `GTID ON`

# `dt-tests/tests/.env.local`

Use plain `KEY=value` lines only. Do not add `export`.

Recommended overrides for the **source MySQL 8** side and the **GaussDB MySQL-compatible target**:

```dotenv
# local mysql 8 source
mysql_extractor_without_auth_url=mysql://127.0.0.1:3311?ssl-mode=disabled
mysql_extractor_username=root
mysql_extractor_password=123456
mysql_extractor_url=mysql://root:123456@127.0.0.1:3311?ssl-mode=disabled
mysql_extractor_url_8_0=mysql://root:123456@127.0.0.1:3311?ssl-mode=disabled

# gaussdb mysql-compatible target (can still be reached via postgres://)
gaussdb_mysql_sinker_without_auth_url=postgres://<gaussdb-host>:8000/<mysql-compatible-db>?sslmode=require
gaussdb_mysql_sinker_username=<username>
gaussdb_mysql_sinker_password=<password>

```

Notes:

- Do not assume `gaussdb_mysql_sinker_without_auth_url` will always be a `mysql://...` URL.
- In the current HCS environment, the validated target contract is `postgres://.../<mysql-compatible-db>`.
- The `gaussdb_mysql_sinker_*` names are historical test placeholder names; they describe the task role, not the wire protocol.
- Keep `gaussdb_mysql_*` only in the gitignored `.env.local`.
- Validated automated entry points:
  - `cargo test -p dt-tests --test integration_test -- mysql_to_gaussdb_mysql::snapshot_tests::test::smoke_test --nocapture`
  - `cargo test -p dt-tests --test integration_test -- mysql_to_gaussdb_mysql::snapshot_tests::test::snapshot_basic_test --nocapture`
  - `cargo test -p dt-tests --test integration_test -- mysql_to_gaussdb_mysql::struct_tests::test::struct_basic_test --nocapture`
  - `cargo test -p dt-tests --test integration_test -- mysql_to_gaussdb_mysql::check_tests::test::check_basic_test --nocapture`
  - `cargo test -p dt-tests --test integration_test -- mysql_to_gaussdb_mysql::cdc_tests::test::cdc_basic_test --nocapture`
  - `cargo test -p dt-tests --test integration_test -- mysql_to_gaussdb_mysql::cdc_tests::test::cdc_type_matrix_test --nocapture`
  - `cargo test -p dt-tests --test integration_test -- mysql_to_gaussdb_mysql::cdc_tests::test::cdc_resume_test --nocapture`

# Snapshot

```ini
[extractor]
db_type=mysql
extract_type=snapshot
url=mysql://127.0.0.1:3307?ssl-mode=disabled
username=root
password=123456
batch_size=10000

[sinker]
db_type=gaussdb_mysql
sink_type=write
url=postgres://gaussdb-user:gaussdb-pass@gaussdb-host:8000/mysql_mode_db?sslmode=require
batch_size=200
replace=true

[filter]
do_dbs=
ignore_dbs=
do_tbs=test_db.*
ignore_tbs=
do_events=insert

[router]
db_map=
tb_map=
col_map=

[parallelizer]
parallel_type=snapshot
parallel_size=4

[pipeline]
buffer_size=16000
checkpoint_interval_secs=10

[runtime]
log_level=info
log4rs_file=./log4rs.yaml
log_dir=./logs
```

# CDC

```ini
[extractor]
db_type=mysql
extract_type=cdc
url=mysql://127.0.0.1:3311?ssl-mode=disabled
username=root
password=123456
server_id=2100
heartbeat_interval_secs=1
heartbeat_tb=heartbeat_db.ape_dts_heartbeat

[filter]
do_tbs=test_db.*
do_events=insert,update,delete

[sinker]
db_type=gaussdb_mysql
sink_type=write
url=postgres://gaussdb-user:gaussdb-pass@gaussdb-host:8000/mysql_mode_db?sslmode=require
batch_size=2
disable_foreign_key_checks=true

[pipeline]
buffer_size=4
checkpoint_interval_secs=1

[runtime]
log_level=info
log4rs_file=./log4rs.yaml
log_dir=./logs
```

# Struct

```ini
[extractor]
db_type=mysql
extract_type=struct
url=mysql://127.0.0.1:3311?ssl-mode=disabled
username=root
password=123456

[sinker]
db_type=gaussdb_mysql
sink_type=struct
url=postgres://gaussdb-user:gaussdb-pass@gaussdb-host:8000/mysql_mode_db?sslmode=require
conflict_policy=interrupt

[filter]
do_dbs=test_db
ignore_dbs=
do_tbs=
ignore_tbs=
do_events=
do_structures=database,table,constraint,comment,index

[router]
db_map=
tb_map=
col_map=

[parallelizer]
parallel_type=serial

[pipeline]
checkpoint_interval_secs=10
buffer_size=100

[runtime]
log_level=info
log4rs_file=./log4rs.yaml
log_dir=./logs
```

# Check

```ini
[extractor]
db_type=mysql
extract_type=snapshot
url=mysql://127.0.0.1:3311?ssl-mode=disabled
username=root
password=123456
batch_size=10000

[sinker]
db_type=gaussdb_mysql
sink_type=check
url=postgres://gaussdb-user:gaussdb-pass@gaussdb-host:8000/mysql_mode_db?sslmode=require
batch_size=200

[filter]
do_dbs=
ignore_dbs=
do_tbs=test_db.*
ignore_tbs=
do_events=insert

[router]
db_map=
tb_map=
col_map=

[parallelizer]
parallel_type=rdb_check
parallel_size=4

[pipeline]
buffer_size=16000
checkpoint_interval_secs=10

[runtime]
log_level=info
log4rs_file=./log4rs.yaml
log_dir=./logs
```
