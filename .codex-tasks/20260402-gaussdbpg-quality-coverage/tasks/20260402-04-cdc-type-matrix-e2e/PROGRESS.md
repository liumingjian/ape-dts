# Progress Log

## Context Recovery Block

- **Task**: `GaussDBPg CDC type matrix + fail-fast evidence`
- **Shape**: `single-full`
- **Truth file**: `.codex-tasks/20260402-gaussdbpg-quality-coverage/tasks/20260402-04-cdc-type-matrix-e2e/TODO.csv`

## 2026-04-02

- Child opened immediately after child 3 (`non-CDC type matrix e2e`) closed.
- Starting assumptions:
  - reuse the first-wave alias set from child 2/3
  - reuse the existing `gaussdb_to_pg` CDC harness (`basic/resume/failover`) instead of inventing a new runner path
  - keep fail-fast scope narrow at first: decoder-level DDL / unknown-op evidence before broader malformed-payload coverage
- Immediate next actions:
  - add `gaussdb_to_pg::cdc_tests::test::cdc_type_matrix_test`
  - scaffold `dt-tests/tests/gaussdb_to_pg/cdc/type_matrix_test/`
  - add focused unit tests for fail-fast error classification in `gaussdb_json_decoder`

- Scaffolding landed:
  - `dt-tests/tests/gaussdb_to_pg/cdc_tests.rs`
    - added `cdc_type_matrix_test`
  - new fixture directory:
    - `dt-tests/tests/gaussdb_to_pg/cdc/type_matrix_test/`
  - validation:
    - `cargo test -p dt-tests --test integration_test --no-run` PASS
- Decoder fail-fast coverage landed:
  - kept DDL/object-like `op_type` fail-fast assertions
  - added unknown `op_type` fail-fast assertions with raw-sample guidance
  - added direct alias parsing coverage for:
    - `tinyint`
    - `smalldatetime`
    - `nvarchar2`
    - `clob`
    - `blob`
  - validation:
    - `cargo test -p dt-connector gaussdb_json_decoder -- --nocapture` PASS
- Real-environment CDC validation:
  - first runtime failed on a single `blob_col` mismatch:
    - source value: binary bytes `[0, 161, 255]`
    - destination value: ASCII bytes for hex text `"00A1FF"`
  - root cause:
    - `mppdb_decoding` emits GaussDB alias types such as `blob` / `tinyint` / `smalldatetime`
    - the CDC decoder only special-cased canonical PG names (`bytea`, `int2`, `timestamp`, etc.), so `blob` was falling back to `String`
  - fix:
    - extend `gaussdb_json_decoder` alias parsing for:
      - `blob -> ColValue::Blob`
      - `tinyint/int1 -> ColValue::Short`
      - `smalldatetime -> ColValue::DateTime`
      - `nvarchar2/clob -> ColValue::String`
  - rerun validation:
    - `cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::cdc_tests::test::cdc_type_matrix_test --nocapture` PASS
  - evidence:
    - `raw/20260402_1625_gaussdb_to_pg_cdc_type_matrix_test.log`
    - `raw/20260402_1635_gaussdb_to_pg_cdc_type_matrix_test.log`
- Child closeout:
  - first-wave `GaussDBPg -> PG` CDC type matrix is now covered in real environment
  - fail-fast decoder evidence exists for DDL/object-like and unknown `op_type`
