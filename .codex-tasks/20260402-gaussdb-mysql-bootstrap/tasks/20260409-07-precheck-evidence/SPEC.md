# Child Spec

## Title

MySQL -> GaussDBMySQL precheck + real-environment evidence

## Parent Epic

- `.codex-tasks/20260402-gaussdb-mysql-bootstrap/EPIC.md`

## Goal

Close the last `PARTIAL` gap in the first-wave `GaussDBMySQL` bootstrap by making precheck
work against the validated real target shape:

- source: local MySQL 8
- target: `DbType::GaussDBMySQL`
- wire protocol: `postgres://.../<mysql-compatible-db>`

Then archive explicit automated and real-environment evidence.

## Constraints

- Keep the scope limited to `MySQL -> GaussDBMySQL`
- Do not expand to CDC runtime implementation
- Preserve the corrected connection model:
  - `GaussDBMySQL` is a SQL-compatibility mode, not a guaranteed MySQL wire endpoint
- Keep the environment clean after precheck tests

## Acceptance

- `cargo test -p dt-tests --test integration_test -- mysql_to_gaussdb_mysql::precheck_tests::test::struct_supported_basic_test --nocapture`
- `cargo test -p dt-tests --test integration_test -- mysql_to_gaussdb_mysql::precheck_tests::test::db_not_exists_test --nocapture`
- Evidence archived under this child `raw/`
- Tracker updated so `MySQL -> GaussDBMySQL（首波）` precheck is no longer `PARTIAL`
