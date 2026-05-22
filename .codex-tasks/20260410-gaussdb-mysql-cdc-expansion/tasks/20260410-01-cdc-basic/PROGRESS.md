# Progress Log

## Context Recovery Block

- **Task**: `MySQL -> GaussDBMySQL cdc basic`
- **Shape**: `single-full`
- **Truth file**: `.codex-tasks/20260410-gaussdb-mysql-cdc-expansion/tasks/20260410-01-cdc-basic/TODO.csv`

## 2026-04-10

- Child opened as the first execution step after bootstrap completion.
- Initial target:
  - create the minimum `cdc_basic_test` path for `mysql_to_gaussdb_mysql`
  - validate whether existing source `MysqlCdcExtractor` plus target pg-wire sink path is already sufficient
- Initial exploration focus:
  - compare against `mysql_to_mysql/cdc/basic_test`
  - locate whether CDC-specific sink / compare / cleanup still assume mysql wire on the target
- Exploration results:
  - Test harness:
    - `RdbTestRunner::run_cdc_test` runs `src_test.sql` DML (insert/update/delete) and does a single final compare.
    - For `DbType::GaussDBMySQL`, destination fetch uses `simple_query` (text-based) to reduce protocol assumptions.
  - Runtime reuse is expected to be high:
    - source side: existing `MysqlCdcExtractor` (binlog) path
    - sink side: `SinkerConfig::Mysql` + `DbType::GaussDBMySQL + postgres://` routes to `PgSinker` (already validated in snapshot/check)
- First child scope decision:
  - start with a minimal one-table DML CDC test (`cdc basic`)
  - keep DDL/resume/failover out of the first iteration
- Implementation (minimal chain) landed:
  - `dt-tests/tests/mysql_to_gaussdb_mysql/cdc_tests.rs` added `cdc_basic_test`
  - `dt-tests/tests/mysql_to_gaussdb_mysql/cdc/basic_test/` added minimal DDL/DML/cleanup fixtures
  - build verification:
    - `cargo test -p dt-tests --test integration_test --no-run` PASS

- Runtime failure found (first real run):
  - Symptom: pipeline `sinked_records` stayed at `0`, final compare saw dst row count `0`.
  - Root cause: MySQL CDC (binlog) decodes text-like columns as `ColValue::RawString(Vec<u8>)`;
    pg-wire sink path previously bound `RawString` as `bytea`, causing `VARCHAR` inserts to fail for
    `DbType::GaussDBMySQL`.
  - Fix:
    - `dt-common/src/meta/adaptor/sqlx_ext.rs`:
      bind `ColValue::RawString` as UTF-8 text when possible; fallback to a stable hex string.
    - `dt-connector/src/sinker/pg/pg_sinker.rs`:
      print full error chain for batch insert failures (`{:#}`) for easier diagnosis.

- Validation:
  - Command:
    - `cargo test -p dt-tests --test integration_test -- mysql_to_gaussdb_mysql::cdc_tests::test::cdc_basic_test --nocapture`
  - Result: PASS
  - Evidence:
    - `.codex-tasks/20260410-gaussdb-mysql-cdc-expansion/tasks/20260410-01-cdc-basic/raw/cdc_basic_test.run2.log`
  - Notes:
    - CDC compare is final-state based; intermediate state can briefly show `dst_data count: 3`
      before the delete event is applied, then converges to the expected final `2` rows.
