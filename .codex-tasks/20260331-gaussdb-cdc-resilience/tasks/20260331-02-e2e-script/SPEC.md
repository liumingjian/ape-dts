# Task Specification

## Task Shape

- **Shape**: `single-full`

## Goals

- 扩展 `scripts/e2e/gaussdb_to_pg_cdc.sh`：
  - `TEST_RESUME=1`：脚本内 kill+restart，并用 position.log/default.log 断言恢复
  - `TEST_FAILOVER=1`：自动执行 `cm_ctl switchover` 切主并验证任务自动恢复
  - `TEST_NEG_SLOT_ACTIVE=1`：并发启动第二个 dt-main 使用同一 slot，断言 slot active 类错误
  - `TEST_NEG_NO_REPL_USER=1`：临时创建无 replication 权限用户跑 precheck/任务，断言权限错误并清理 role

## Constraints

- 敏感信息仅来自环境变量，且不输出到日志。
- failover 通过 sshpass 自动化执行 `cm_ctl`，并 best-effort 切回原主。

## Done-When

- [ ] `TEST_FAILOVER=1 bash scripts/e2e/gaussdb_to_pg_cdc.sh` 在真实环境 PASS
- [ ] 负例开关至少本地跑通 1 个（slot active / no repl user）

