# Progress Log

## Context Recovery Block

- **Task**: `GaussDBPg non-CDC type matrix e2e`
- **Shape**: `single-full`
- **Truth file**: `.codex-tasks/20260402-gaussdbpg-quality-coverage/tasks/20260402-03-non-cdc-type-matrix-e2e/TODO.csv`

## 2026-04-02

- Child opened after child 2 (`type contract + codecs`) passed.
- First directional decision locked:
  - `PG -> GaussDBPg snapshot` should use canonical PG types
  - `GaussDBPg -> PG check` should use the GaussDB-specific aliases from child 2
- Immediate implementation goal:
  - create the two dt-tests entry points
  - scaffold first-wave fixture SQL around:
    - timestamp / smalldatetime
    - smallint / tinyint
    - varchar / nvarchar2
    - text / clob
    - bytea / blob
- Scaffolding landed:
  - `dt-tests/tests/pg_to_gaussdb/snapshot_tests.rs`
    - added `type_matrix_test`
  - `dt-tests/tests/gaussdb_to_pg/check_tests.rs`
    - added `type_matrix_test`
  - new fixture directories:
    - `dt-tests/tests/pg_to_gaussdb/snapshot/type_matrix_test/`
    - `dt-tests/tests/gaussdb_to_pg/check/type_matrix_test/`
- Validation so far:
  - `cargo test -p dt-tests --test integration_test --no-run` PASS
  - runtime attempt:
    - `cargo test -p dt-tests --test integration_test -- pg_to_gaussdb::snapshot_tests::test::type_matrix_test --nocapture`
    - result: FAIL due sandbox network restriction, not due SQL/codec logic
    - observed error:
      - `error communicating with database: Operation not permitted (os error 1)`
      - candidate probe failed for all remote GaussDB endpoints
- Evidence:
  - `compile_no_run.log`
  - `raw/20260402_pg_to_gaussdb_type_matrix_test.log`
- Current blocker:
  - real-environment execution cannot proceed inside the current restricted sandbox because remote GaussDB connections are denied at the OS/sandbox layer

- Runtime validation resumed after network access was restored.
- `PG -> GaussDBPg snapshot` findings:
  - initial diff showed `clob_col` mismatch because the current GaussDB environment treats `''` as `NULL`
  - fixture was adjusted to use `NULL` explicitly for the nullable second row instead of an empty string sentinel
  - validation passed:
    - `cargo test -p dt-tests --test integration_test -- pg_to_gaussdb::snapshot_tests::test::type_matrix_test --nocapture`
  - evidence:
    - `raw/20260402_1610_pg_to_gaussdb_snapshot_type_matrix_test.log`
- `GaussDBPg -> PG check` findings:
  - first runtime failure moved from SQL syntax to a single diff on `tiny_col`
  - real-node probe on the RW primary (`10.250.0.30`) showed that GaussDB `tinyint` is stored with `pg_type.typname = int1` while `format_type(...) = tinyint`
  - core fix landed in the PG type layer:
    - `int1 -> int2 -> PgValueType::Int16`
    - keep `blob` extraction as `text` and decode from hex
  - validation passed:
    - `cargo test -p dt-common gaussdb_type_matrix -- --nocapture`
    - `cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::check_tests::test::type_matrix_test --nocapture`
  - evidence:
    - `raw/20260402_tinyint_probe.log`
    - `raw/20260402_1605_gaussdb_to_pg_check_type_matrix_test.log`
- Child closeout:
  - both non-CDC directional tests now pass in the real environment
  - child 3 can be marked `DONE`
