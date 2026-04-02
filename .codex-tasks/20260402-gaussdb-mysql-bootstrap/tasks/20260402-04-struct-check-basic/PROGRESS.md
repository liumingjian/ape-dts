# Progress Log

## Context Recovery Block

- **Task**: `MySQL -> GaussDBMySQL struct + check basic`
- **Shape**: `single-full`
- **Truth file**: `.codex-tasks/20260402-gaussdb-mysql-bootstrap/tasks/20260402-04-struct-check-basic/TODO.csv`

## 2026-04-02

- Child opened immediately after child 3 (`snapshot basic`) passed.
- Immediate focus:
  - locate the existing reusable `MySQL-compatible` struct/check execution path
  - identify which pieces still assume `gaussdb_mysql == mysql://`
  - keep scope limited to basic struct/check before any CDC expansion
- Exploration findings so far:
  - existing reusable tests:
    - `mysql_to_mysql/struct/basic_test`
    - `mysql_to_mysql/check/basic_test`
    - `pg_to_gaussdb/struct_tests.rs` / `check_tests.rs` as cross-engine examples
  - current runtime split:
    - `MysqlStructSinker` / `MysqlChecker` assume a MySQL pool
    - `PgStructSinker` / `PgChecker` assume a Postgres pool
    - child 3 already proved `GaussDBMySQL` can require `DbType::GaussDBMySQL + postgres://`
  - struct-side probe result:
    - `SHOW CREATE DATABASE` and `SHOW CREATE TABLE` both work against `postgres://.../jyp_test_m`
    - evidence: `raw/20260402_show_create_probe.log`
    - returned DDL is MySQL-compatible in shape, but includes GaussDB-specific extras such as:
      - `SET search_path = ...`
      - `USING ubtree`
      - `WITH (storage_type=USTORE)`
      - `TABLESPACE pg_default`
- Working hypothesis for implementation:
  - struct apply path can likely reuse `PgStructSinker` for `GaussDBMySQL + postgres://`, because MySQL-compatible DDL strings already execute successfully on the target through a Postgres pool
  - struct validation likely needs a dedicated pg-wire `SHOW CREATE ...` fetcher plus normalization, rather than the current `MysqlStructCheckFetcher`
  - check basic can likely reuse child 3's target simple-query row fetch path, but the runtime `MysqlCheck -> MysqlChecker` branch still needs a `GaussDBMySQL + postgres://` adaptation
- Implementation landed:
  - `dt-task/src/task_util.rs`
    - `MysqlStruct` / `MysqlCheck` now route `DbType::GaussDBMySQL + postgres://` to pg meta-manager and pg pool creation
  - `dt-task/src/sinker_util.rs`
    - `MysqlStruct` now reuses `PgStructSinker` for pg-wire `GaussDBMySQL`
    - `MysqlCheck` now reuses `PgChecker` for pg-wire `GaussDBMySQL`
  - `dt-connector/src/sinker/pg/pg_checker.rs`
    - added `db_type/url/connection_auth`
    - `GaussDBMySQL` target fetch now uses `tokio-postgres simple_query` plus literal select SQL built by `RdbQueryBuilder::new_for_pg_compatible`
    - destination struct fetch now propagates `db_type` into `PgStructFetcher`
  - `dt-tests/tests/test_runner/rdb_struct_test_runner.rs`
    - for `GaussDBMySQL + postgres://`, destination validation now issues `SHOW CREATE DATABASE/TABLE` through pg-wire `simple_query`
- First runtime validation exposed only one fixture mismatch:
  - actual target collation in `SHOW CREATE` is `utf8mb4_0900_ai_ci`
  - `expect_ddl.sql` still expected `utf8mb4_general_ci`
  - captured exact DDL with direct `SHOW CREATE DATABASE/TABLE` probes, then aligned the fixture
- Validation completed:
  - `cargo test -p dt-connector -p dt-task -p dt-tests --no-run` PASS
  - `cargo test -p dt-tests --test integration_test -- mysql_to_gaussdb_mysql::struct_tests::test::struct_basic_test --nocapture` PASS
  - `cargo test -p dt-tests --test integration_test -- mysql_to_gaussdb_mysql::check_tests::test::check_basic_test --nocapture` PASS
- Evidence written to:
  - `raw/20260402_child4_compile_no_run.log`
  - `raw/20260402_show_create_probe.log`
  - `raw/20260402_mysql_to_gaussdb_mysql_struct_basic_test.log`
  - `raw/20260402_mysql_to_gaussdb_mysql_struct_basic_test_run2.log`
  - `raw/20260402_mysql_to_gaussdb_mysql_check_basic_test.log`
- Child 4 is now closed.
