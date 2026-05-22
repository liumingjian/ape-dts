# Progress Log

## Context Recovery Block

- **Task**: `MySQL -> GaussDBMySQL cdc type-matrix`
- **Shape**: `single-full`
- **Truth file**: `.codex-tasks/20260410-gaussdb-mysql-cdc-expansion/tasks/20260410-02-cdc-type-matrix/TODO.csv`

## 2026-04-10

- Child 2 opened after child 1 (`cdc basic`) validated PASS.

## 2026-04-13

- Attempted to validate the real-env path:
  - Command: `cargo test -p dt-tests --test integration_test -- mysql_to_gaussdb_mysql::cdc_tests::test::cdc_type_matrix_test --nocapture`
  - Result: **PASS**
  - Evidence: `raw/cdc_type_matrix_test.pass.20260413.log`
  - Fix applied before re-run:
    - GaussDB MySQL-compatible mode `TIMESTAMP` is timezone-sensitive in text output. For deterministic compare (MySQL connection runs in UTC), dt-tests now runs `SET TIME ZONE 'UTC'` for the GaussDB simple_query compare session.
