# Progress Log

## Context Recovery Block

- **Task**: `MySQL -> GaussDBMySQL cdc resilience + negatives`
- **Shape**: `single-full`
- **Truth file**: `.codex-tasks/20260410-gaussdb-mysql-cdc-expansion/tasks/20260410-03-cdc-resilience-negatives/TODO.csv`

## 2026-04-13

- Child 3 opened after child 2 (`cdc type-matrix`) validated PASS.
- Next: implement mysql cdc resume test (from_log) by generalizing dt-tests runner checkpoint parsing beyond PgCdc.

- Implemented and validated:
  - Added `mysql_to_gaussdb_mysql/cdc/resume_test` fixtures and `cdc_resume_test` entry.
  - Generalized dt-tests `run_cdc_resume_test` to assert recovery based on `Position` variants (`MysqlCdc` / `PgCdc`).
  - Fixed dt-tests `TestConfigUtil::update_file_paths_in_task_config` to not rewrite empty `resumer.config_file` to `{project_root}/` (directory), which previously crashed LogRecovery.
  - Validation command:
    - `cargo test -p dt-tests --test integration_test -- mysql_to_gaussdb_mysql::cdc_tests::test::cdc_resume_test --nocapture`
  - Evidence:
    - `raw/cdc_resume_test.pass.20260413.log`
