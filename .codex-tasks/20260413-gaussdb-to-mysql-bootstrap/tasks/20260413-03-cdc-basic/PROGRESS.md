# Progress Log

## Context Recovery Block

- **Task**: `GaussDB→MySQL cdc basic`
- **Shape**: `single-full`
- **Truth file**: `.codex-tasks/20260413-gaussdb-to-mysql-bootstrap/tasks/20260413-03-cdc-basic/TODO.csv`
- **Current milestone**: `complete`
- **Current status**: `DONE`
- **Last completed**: `4 - 回写 child 与 parent 进展`
- **Current artifact**: `validated cdc fixture and tracker updates`
- **Key context**:
  - `gaussdb_to_mysql::cdc_tests::test::cdc_basic_test` 已具备自动化入口并通过真实环境验证。
  - router 复用既有约定：`db_map=public:gaussdb_to_mysql_cdc_dst`。
  - CDC 夹具执行 insert/update/delete，最终状态为 1 行（id=2,val=c），并在对比阶段验证源/目标一致。
- **Known issues**:
  - GaussDB CDC 启动与共享 HA 环境存在抖动，当前 `start_millis=60000`、`parse_millis=30000` 已足够支撑 basic。
- **Next action**: 进入 child 4：收口 docs/tracker/e2e（把本 epic 证据入口补进文档）。

## 2026-04-13

- Child 3 opened under `20260413-gaussdb-to-mysql-bootstrap`.
- Added `dt-tests/tests/gaussdb_to_mysql/cdc/basic_test` fixture and `cdc_basic_test` entry.
- Validation:
  - `cargo test -p dt-tests --test integration_test --no-run`
  - `cargo test -p dt-tests --test integration_test -- gaussdb_to_mysql::cdc_tests::test::cdc_basic_test --nocapture`
- Result: PASS
  - CDC streaming started and DML replicated to MySQL target
  - final compare: `public.gaussdb_to_mysql_cdc_basic` == `gaussdb_to_mysql_cdc_dst.gaussdb_to_mysql_cdc_basic` (1 row)
  - cleanup: gaussdb cdc slot dropped successfully
