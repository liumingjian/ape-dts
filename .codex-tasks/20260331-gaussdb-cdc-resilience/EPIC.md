# GaussDBPg CDC P1：Resume + Failover + 负例套件（Epic）

## Goal

- 在不扩大功能边界（DDL/未知事件继续 fail fast）的前提下，把 `GaussDBPg -> PG` CDC 做到“可演练、可回归、可定位”：
  - 断点续传（进程重启，从 checkpoint LSN 恢复）
  - 主备切换自动恢复（无需人工重启任务）
  - 负例套件优先覆盖：权限/slot 类问题
- 按行业常见期望：数据库切主不应破坏实时同步任务，可允许短暂超时/异常，但工具需具备自愈能力（自动重连并继续同步）。
- 将 `gaussdb_pg_candidate_hosts` 纳入更严格的 precheck（无 RW 主直接 fail）。
- 交付以 `dt-tests + e2e 脚本` 双保险，并把证据归档到 `PROGRESS.md/raw/`（脱敏）。

## Non-Goals

- 不实现/不承诺 CDC DDL 同步；遇到 DDL/未知事件仍按现有策略 fail fast（只补负例与证据采集路径）。
- 不引入新的公开配置项（仅利用既有配置与环境变量）。
- 不把专网真实环境接入公共 CI。

## Constraints

- 口令/凭据不进入 git：`.env.local`、`.local/`、带口令的 URL、ssh 密码等均禁止提交。
- e2e 脚本通过环境变量传入敏感信息，且不得打印到 stdout/stderr。
- 真实环境 failover 使用 `cm_ctl`，通过 `sshpass -e` 自动化（`SSHPASS` 来自环境变量）。

## Risk Assessment

- 主备切换可能影响共享环境：脚本必须 best-effort 切回原主并在退出时清理测试对象。
- GaussDB HA 环境可能不稳定：测试需要更长超时与明确的日志/证据归档。

## Child Deliverables

- dt-tests：新增 `gaussdb_to_pg` CDC `resume_test` 并回归通过。
- e2e：扩展 `scripts/e2e/gaussdb_to_pg_cdc.sh` 支持 resume/failover/负例（slot active、无 replication 权限）。
- precheck：候选选主严格化 + 权限/slot/HA 端口检查，fail fast 且报错可定位。
- docs：runbook + tracker 同步更新，补演练步骤与证据采集指引。

## Dependency Notes

- `#2 e2e` 依赖 `#1 dt-tests` 的能力落地（至少复用 SQL/断言与日志证据口径）。
- `#3 precheck` 先于 `#2 e2e negative` 更稳（负例优先通过 precheck 暴露）。
- `#4 docs` 收尾依赖前 3 项均有证据产出。

## Done-When

- [ ] `SUBTASKS.csv` 全部为 `DONE`
- [ ] 至少完成一次真实环境 e2e：`TEST_FAILOVER=1` 并归档脱敏证据
- [ ] `cargo test -p dt-tests ... cdc_resume_test` 通过
