# Task Specification

## Task Shape

- **Shape**: `single-full`

## Goals

- 新增 `dt-tests` 用例：`gaussdb_to_pg` CDC `resume_test`（进程重启从 checkpoint LSN 恢复）。
- 验证点：
  - Phase A：CDC 正常同步 DML1，并在 `position.log` 出现 `checkpoint_position` 的 `PgCdc` LSN。
  - Phase B：kill/restart 后日志出现 `cdc recovery from lsn:[...]`，执行 DML2，目标端最终一致。

## Non-Goals

- 不证明严格“不丢不重”（只要求从 checkpoint 继续、最终一致且证据可定位）。

## Constraints

- 使用 `[resumer] resume_type=from_log`，log_dir 必须唯一且在两次启动间复用。
- 不引入新配置项。

## Deliverables

- `dt-tests/tests/gaussdb_to_pg/cdc/resume_test/`（sql + task_config.ini）
- `dt-tests` runner 扩展（新增 `run_cdc_resume_test`）
- `gaussdb_to_pg::cdc_resume_test` 测试入口

## Done-When

- [ ] `cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::cdc_tests::test::cdc_resume_test --nocapture` 通过

## Final Validation Command

```bash
cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::cdc_tests::test::cdc_resume_test --nocapture
```

