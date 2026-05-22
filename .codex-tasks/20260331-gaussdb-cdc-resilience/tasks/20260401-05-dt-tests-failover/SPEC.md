# Task Specification

## Task Shape

- **Shape**: `single-full`

## Goals

- 新增 `dt-tests` 用例：`gaussdb_to_pg` CDC `failover_test`，把你给的主备切换步骤融合进集成测试流程中（通过环境变量显式开启，避免在无真实环境时误跑）。
- 验证点：
  - Phase A：CDC 正常同步 DML1，目标端数据一致。
  - Failover：通过 `ssh -> su - Ruby -> source gauss_env_file -> cm_ctl switchover -n 2 ...` 切到 node2，并轮询 `cm_ctl query -Cv` 直到 Primary 为 node2。
  - Phase B：等待 dt-main 日志出现切主后 `replication streaming started: <new_primary>:<sql_port+1>`，执行 DML2，目标端最终一致。
- 无污染：
  - 测试结束 best-effort 切回原主（若原主不是 node2）。
  - 清理测试表（沿用现有 prepare/clean SQL；不强制 drop slot）。

## Constraints

- 敏感信息仅来自环境变量（不写入 git，不打印明文口令）。
- 仅在显式设置 `ENABLE_GAUSSDB_FAILOVER_TEST=1` 时执行切主逻辑，否则测试直接跳过。

## Done-When

- [ ] `cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::cdc_tests::test::cdc_failover_test --nocapture` 通过（在真实环境开启开关后）
- [ ] 归档关键日志片段到本任务 `raw/`（脱敏）

