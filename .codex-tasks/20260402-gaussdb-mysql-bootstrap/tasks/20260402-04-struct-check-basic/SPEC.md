# Child Spec

## Title

MySQL -> GaussDBMySQL struct + check basic

## Parent Epic

- `.codex-tasks/20260402-gaussdb-mysql-bootstrap/EPIC.md`

## Goal

On top of the corrected `postgres:// + sql_compatibility=M` connection model and the now-green
snapshot basic path, extend the first-wave `GaussDBMySQL` support to cover:

1. minimal struct sync (`schema/table/index/constraint/comment` scope aligned with existing MySQL-compatible paths)
2. minimal check validation (`MySQL -> GaussDBMySQL`) with real-environment evidence

## Constraints

- Keep scope limited to `MySQL -> GaussDBMySQL`
- Reuse existing MySQL-compatible struct/check machinery where possible
- Preserve the corrected runtime model:
  - target may be `DbType::GaussDBMySQL`
  - wire protocol may still be `postgres://`
  - RW candidate resolution may be required on the target side
- Do not expand to CDC in this child

## Acceptance

- `cargo test -p dt-tests --test integration_test -- mysql_to_gaussdb_mysql::struct_tests::test::struct_basic_test --nocapture`
- `cargo test -p dt-tests --test integration_test -- mysql_to_gaussdb_mysql::check_tests::test::check_basic_test --nocapture`
- Evidence archived under this child `raw/`

## Initial Hypothesis

- Snapshot basic already proved that data write + compare can work against `jyp_test_m`
- Struct and check will likely need the same two compatibility layers:
  - target-side pg-wire handling
  - target-side read/metadata/check compatibility when db type is `GaussDBMySQL`
