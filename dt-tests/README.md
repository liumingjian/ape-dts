# English | [中文](README_ZH.md)

# Run tests

## Manual MySQL to PostgreSQL red-line

For major engine changes, run the isolated Docker red-line with the real `dt-main` binary:

```bash
bash scripts/e2e/mysql_to_postgresql_redline.sh
```

It validates Snapshot, CDC readiness, INSERT, UPDATE, DELETE, and final full-table equality. Console and structure migration are intentionally excluded. See [the red-line specification](../docs/testing/mysql-to-postgresql-redline.md) for boundaries, deadlines, artifacts, and `KEEP_ENV=1` debugging.

```rust
#[tokio::test]
#[serial]
async fn cdc_basic_test() {
    TestBase::run_cdc_test("mysql_to_mysql/cdc/basic_test", 3000, 2000).await;
}
```

```
cargo test --package dt-tests --test integration_test -- mysql_to_mysql::cdc_tests::test::cdc_basic_test --nocapture 
```

- A test contains: 
  - task_config.ini
  - src_prepare.sql
  - dst_prepare.sql
  - src_test.sql
  - dst_test.sql
  - Notes for `*.sql` test files:
    - Multi-statement SQL must be terminated with `;` (the runner splits statements by `;`).
    - Avoid `--` inside string/JSON literals (the runner strips inline `-- ...`).

- Steps for running a test: 
  - 1, execute src_prepare.sql in source database.
  - 2, execute dst_prepare.sql in target database.
  - 3, start data sync task.
  - 4, sleep some milliseconds for task initialization (start_millis, you may change it based on source/target performance).
  - 5, execute src_test.sql in source database.
  - 6, execute dst_test.sql (if exists) in target database.
  - 7, poll: re-compare source and target every 200ms until they match, or until parse_millis
       elapses (a mismatch after that is a real failure, never retried away).

# Environment guards

Integration tests need live databases. Instead of panicking on a machine that never provisioned
them, every test entry point checks first and **skips with a printed reason** when:

- neither `dt-tests/tests/.env` nor `.env.local` exists (copy `.env.src` and fill it in), or
- the test's `task_config.ini` references an env var nobody defined, or
- the endpoint it points at refuses a TCP connection.

So `cargo test -p dt-tests` is safe to run anywhere: it reports what is missing instead of failing.

Set `DT_TESTS_STRICT_ENV=1` to turn every skip into a failure — use it wherever the environment is
supposed to be up (CI, the compose stack) so a broken stack cannot pass as green.

# The nightly matrix and its compose stack

`.github/workflows/e2e-tests.yml` runs `mysql_to_mysql`, `pg_to_pg`, `mongo_to_mongo` and
`redis_to_redis` every night (and on demand via *Run workflow*, which takes a comma-separated
subset). Each job starts one compose profile from `docker-compose.ci.yml` and copies `.env.ci`
to `tests/.env`, so the same stack is one command away locally:

```
docker compose -f dt-tests/docker-compose.ci.yml --profile mysql up --detach --wait
cp dt-tests/.env.ci dt-tests/tests/.env
```

Profiles: `mysql`, `pg`, `mongo`, `redis`, `clickhouse`. Nothing starts without one — that is
what keeps a redis job from booting five MySQL servers it never touches.

Two invariants the workflow relies on, worth keeping if you edit it:

- `DT_TESTS_STRICT_ENV=1` — in a nightly job a skip is indistinguishable from a pass, so every
  skip must be a failure. That in turn means a suite may only enter the matrix once its whole
  stack exists in `docker-compose.ci.yml`.
- `--test-threads 1` — the suites share databases and rely on `#[serial]`, which serialises
  within a process only. nextest gives every test its own process.

`.env.ci` and `.env.src` must define every `{placeholder}` any `task_config.ini` references;
`tests/test_runner/env_ci.rs` asserts it without touching a database, and the workflow runs it
before starting a single container. Adding a test that references a new endpoint therefore means
adding the key to both env files — and, if the nightly matrix should cover it, a service to
`docker-compose.ci.yml`.

On arm64 machines `docker-compose.override.local.yml` swaps MySQL 5.7 for 8.0 (5.7 has no arm64
image); CI runs amd64 and keeps 5.7, because `mysql_extractor_url` and `mysql_extractor_url_8_0`
are two different server versions on purpose. The redis 2.8 and `redislabs/*` module images are
amd64-only too, so the full redis profile is a CI-only stack.

# The matrix reports, it does not flatter

The first nights are red, and that is the intended outcome. `DT_TESTS_STRICT_ENV=1` plus a suite
that has not run anywhere since 2026-07 means the baseline debt surfaces all at once: run
[32145904273](https://github.com/liumingjian/ape-dts/actions/runs/32145904273) had redis green,
mongo 19/21, mysql 56/72 and pg 56/71 with two timeouts. Every remaining failure is triaged into
a ticket (#94 fixture semicolons, #95 MySQL 5.7 vs 8.0 fixtures, #96 pg schema dependencies,
#97 checker expectation drift, #98 pg parallel/resume, #99 cdc-to-sql, #100 the mysql remainder).

Relaxing strict mode or deleting the failing cases would make the matrix green and worthless. If
you are tempted to do either, fix the ticket instead.

Tests that need more than databases are `#[ignore]`d and run explicitly:

```
cargo test -p dt-tests --test gaussdb_snapshot_cdc_e2e -- --ignored   # needs console + playwright + gaussdb
```

# Config
- All database urls are configured in ./tests/.env file and referenced in task_config.ini of tests.

```
[extractor]
url={mysql_extractor_url}

[sinker]
url={mysql_sinker_url}
```

# Init test env

- Examples work in docker. [prerequisites](/docs/en/tutorial/prerequisites.md)

# Postgres
[Prepare Postgres instances](/docs/en/tutorial/pg_to_pg.md)

## To run [Two-way data sync](/docs/en/cdc/two_way.md) tests
- pg_to_pg::cdc_tests::test::cycle_

- You need to create 3 Postgres instances, and set wal_level = logical for each one.


## To run [charset tests](../dt-tests/tests/pg_to_pg/snapshot/charset_euc_cn_test)
- Create database "postgres_euc_cn" in both source and target.

```
CREATE DATABASE postgres_euc_cn
  ENCODING 'EUC_CN'
  LC_COLLATE='C'
  LC_CTYPE='C'
  TEMPLATE template0;
```

# GaussDB (PG-compatible)

- Test suites:
  - `pg_to_gaussdb/*` (snapshot/struct/check)
  - `gaussdb_to_pg/*` (snapshot/cdc/check)
- Config:
  - Use `db_type=gaussdb_pg` in test `task_config.ini`
  - Override URLs in `dt-tests/tests/.env.local` (recommended):
    - `gaussdb_pg_extractor_without_auth_url`, `gaussdb_pg_extractor_username`, `gaussdb_pg_extractor_password`
    - `gaussdb_pg_sinker_without_auth_url`, `gaussdb_pg_sinker_username`, `gaussdb_pg_sinker_password`
- Notes:
  - MVP requires GaussDB MD5 auth (e.g. `password_encryption_type=1`)
  - CDC requires `wal_level=logical` and `mppdb_decoding` availability

# MySQL
[Prepare MySQL instances](/docs/en/tutorial/mysql_to_mysql.md)

## To run [Two-way data sync](/docs/en/cdc/two_way.md) tests
- mysql_to_mysql::cdc_tests::test::cycle_

- You need to create 3 Postgres instances

# Mongo
[Prepare Mongo instances](/docs/en/tutorial/mongo_to_mongo.md)

# Kafka
[Prepare Kafka instances](/docs/en/tutorial/mysql_to_kafka_consumer.md)

# StarRocks
[Prepare StarRocks instances](/docs/en/tutorial/mysql_to_starrocks.md)

For old version: 2.5.4

```
docker run -itd --name some-starrocks-2.5.4 \
-p 9031:9030 \
-p 8031:8030 \
-p 8041:8040 \
starrocks/allin1-ubuntu:2.5.4
```

# Doris
[Prepare Doris instances](/docs/en/tutorial/mysql_to_doris.md)

# Redis
[Prepare Redis instances](/docs/en/tutorial/redis_to_redis.md)

## More versions
- Data format varies in different redis versions, we support 2.8 - 7.*, rebloom, rejson.
- redis:7.0
- redis:6.0
- redis:6.2
- redis:5.0
- redis:4.0
- redis:2.8.22
- redislabs/rebloom:2.6.3
- redislabs/rejson:2.6.4
- Can not deploy 2.8,rebloom,rejson on mac, you may deploy them in EKS(amazon)/AKS(azure)/ACK(alibaba), refer to: dt-tests/k8s/redis.

### Source

```
docker run --name src-redis-7-0 \
    -p 6380:6379 \
    -d redis:7.0 redis-server \
    --requirepass 123456 \
    --save 60 1 \
    --loglevel warning

docker run --name src-redis-6-2 \
    -p 6381:6379 \
    -d redis:6.2 redis-server \
    --requirepass 123456 \
    --save 60 1 \
    --loglevel warning

docker run --name src-redis-6-0 \
    -p 6382:6379 \
    -d redis:6.0 redis-server \
    --requirepass 123456 \
    --save 60 1 \
    --loglevel warning

docker run --name src-redis-5-0 \
    -p 6383:6379 \
    -d redis:5.0 redis-server \
    --requirepass 123456 \
    --save 60 1 \
    --loglevel warning

docker run --name src-redis-4-0 \
    -p 6384:6379 \
    -d redis:4.0 redis-server \
    --requirepass 123456 \
    --save 60 1 \
    --loglevel warning
```

### Target

```
docker run --name dst-redis-7-0 \
    -p 6390:6379 \
    -d redis:7.0 redis-server \
    --requirepass 123456 \
    --save 60 1 \
    --loglevel warning

docker run --name dst-redis-6-2 \
    -p 6391:6379 \
    -d redis:6.2 redis-server \
    --requirepass 123456 \
    --save 60 1 \
    --loglevel warning

docker run --name dst-redis-6-0 \
    -p 6392:6379 \
    -d redis:6.0 redis-server \
    --requirepass 123456 \
    --save 60 1 \
    --loglevel warning

docker run --name dst-redis-5-0 \
    -p 6393:6379 \
    -d redis:5.0 redis-server \
    --requirepass 123456 \
    --save 60 1 \
    --loglevel warning

docker run --name dst-redis-4-0 \
    -p 6394:6379 \
    -d redis:4.0 redis-server \
    --requirepass 123456 \
    --save 60 1 \
    --loglevel warning
```
