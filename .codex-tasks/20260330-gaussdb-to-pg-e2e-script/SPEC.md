# GaussDB (PG-compatible) -> Postgres CDC：手动步骤脚本化（无污染 E2E）

## 背景

用户在按手动文档执行 GaussDB -> PG CDC 验证时出现失败，希望将当前步骤固化为可重复执行的 E2E 脚本，并由脚本驱动执行测试与清理，确保测试前后环境无污染。

当前手动文档：

- `docs/zh/cdc/gaussdb_to_pg_manual_test.md`

用户环境（2026-03-30）：

- `gaussdb_pg_candidate_hosts="10.250.0.52:8000,10.250.0.30:8000,10.250.0.51:8000"`（其中 `10.250.0.51:8000` 为主）
- 目标 PG：`postgres://127.0.0.1:5434/postgres?options[statement_timeout]=10s`（本机 Docker Postgres15）

## 目标

1. 将手动步骤实现为一键脚本：启动/校验目标 PG、准备源端/目标端表、启动 `dt-main` CDC 任务、执行源端 DML、验证目标端结果、最后强制清理。
2. 环境无污染：
   - 源端/目标端测试 schema/table 不残留
   - 源端 replication slot 不残留
   - 若脚本启动了 Docker Postgres 容器，脚本结束后容器不残留
3. 脚本可直接用于用户给定的候选节点与本地 PG URL，且不要求将密码写入仓库（使用 `.local/` 等 gitignored 路径）。

## 成功标准

- `bash scripts/e2e/gaussdb_to_pg_cdc.sh` 在用户环境下通过：
  - 目标端最终仅剩 `(2, 'c')`
  - 退出后源端/目标端无测试 schema/table，源端 slot 已 drop
  - `ape-dts-pg15` 容器（若由脚本启动）被删除
- 失败时也必须执行清理（trap）。

## 约束与注意事项

- 不在 Git 中提交任何真实凭据。脚本生成的 `dt-main` 配置与运行日志写入 `.local/`（已在 repo `.gitignore` 中忽略）。
- 清理逻辑仅作用于脚本指定的 `TEST_SCHEMA/TEST_TABLE/SLOT_NAME`，避免误删其他对象。

