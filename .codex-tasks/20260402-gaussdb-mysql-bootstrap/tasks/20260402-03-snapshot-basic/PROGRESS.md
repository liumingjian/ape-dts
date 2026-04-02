# Progress Log

## Context Recovery Block

- **Task**: `MySQL → GaussDBMySQL snapshot basic`
- **Shape**: `single-full`
- **Truth file**: `.codex-tasks/20260402-gaussdb-mysql-bootstrap/tasks/20260402-03-snapshot-basic/TODO.csv`

## 2026-04-02

- Child opened after protocol/mode realignment was completed in child 6.
- Immediate focus:
  - make `gaussdb_mysql + postgres://` work for the minimal snapshot path
  - keep the first acceptance surface small: source DML -> target write -> compare rows
- Validation and implementation timeline:
  - `cargo test -p dt-common -p dt-task -p dt-tests --no-run` PASS
    - evidence: `raw/20260402_compile_no_run.log`
  - first live smoke attempt reached sink stage but failed on target insert SQL:
    - `PgSinker` was still emitting PostgreSQL-style quoted identifiers and typed placeholders
    - blocker example: `INSERT INTO "gaussdb_mysql_smoke"."smoke_basic"...$1::int4...`
  - fixed minimal write path by:
    - routing `DbType::GaussDBMySQL + postgres://` sink writes through `PgSinker`
    - teaching `RdbQueryBuilder` a pg-wire-compatible MySQL-mode branch:
      - keep postgres bind markers (`$1`, `$2`, ...)
      - drop postgres type casts for placeholders/extract expressions
      - keep MySQL-style replace/escaping semantics for `DbType::GaussDBMySQL`
  - second live smoke attempt confirmed write success but compare failed on target fetch:
    - runtime task finished with `sinked_records=2`
    - `sqlx` fetch path hit server-side failure on `jyp_test_m`:
      - `invalid memory alloc request size ... in resowner.cpp`
  - narrowed the issue with live probes:
    - direct `psql SELECT ... FROM gaussdb_mysql_smoke.smoke_basic ORDER BY id ASC` succeeded
    - `PREPARE ... AS SELECT ...` is rejected in this MySQL-compatible database
    - conclusion: target validation must avoid the prepared/extended query path
  - implemented target compare fallback for `GaussDBMySQL`:
    - use `tokio-postgres simple_query` with SSL/NoTLS fallback based on URL/env
    - parse returned text rows through `PgColValueConvertor::from_str`
  - a later smoke retry exposed a second environment-specific blocker:
    - fixed sink URL could still land on a read-only standby after cluster role changes
    - extended `maybe_rewrite_gaussdb_primary_urls()` so `GaussDBMySQL + postgres://` also participates in candidate-first RW resolution
  - final live validations:
    - `cargo test -p dt-tests --test integration_test -- mysql_to_gaussdb_mysql::snapshot_tests::test::smoke_test --nocapture` PASS
      - evidence: `raw/20260402_mysql_to_gaussdb_mysql_smoke_test_run3.log`
    - `cargo test -p dt-tests --test integration_test -- mysql_to_gaussdb_mysql::snapshot_tests::test::snapshot_basic_test --nocapture` PASS
      - evidence: `raw/20260402_mysql_to_gaussdb_mysql_snapshot_basic_test.log`
- Final state:
  - local MySQL 8 source -> live `jyp_test_m` target snapshot basic is now green
  - target RW candidate rewrite selected `10.250.0.30:8000/jyp_test_m`
  - compare phase matched 2/2 rows end to end
- Residual note for future type-matrix work:
  - `updated_at` currently comes back from the target simple-query path as textual data (`String`) rather than `DateTime`
  - snapshot basic is still correct because row comparison falls back to normalized string equality, but child 4+/quality work should tighten this typing behavior
