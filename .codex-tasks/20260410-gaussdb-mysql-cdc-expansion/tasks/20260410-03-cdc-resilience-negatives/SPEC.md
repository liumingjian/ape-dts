# Child Spec

## Title

MySQL -> GaussDBMySQL cdc resilience + negatives

## Parent Epic

- `.codex-tasks/20260410-gaussdb-mysql-cdc-expansion/EPIC.md`

## Goal

在 `cdc basic` / `cdc type-matrix` 已闭环的前提下，把 `MySQL -> GaussDBMySQL` CDC 做到：

1. **Resume（进程重启）**：启用 `resumer.from_log`，任务重启后能从 checkpoint position 恢复并继续同步
2. **负例可诊断**：覆盖最关键的失败路径并给出可定位报错（避免静默失败）

## Constraints

- 仍只覆盖 `MySQL -> GaussDBMySQL`（目标端 `sql_compatibility=M` 且通过 pg-wire 连接）
- 仍保持 CDC 语义：仅 DML（不加入 DDL CDC）
- 必须“无污染”：测试前后清理 schema/db/临时用户；证据日志需脱敏

## Environment (Facts)

- Local MySQL 8: `mysql://127.0.0.1:3311`（docker）
- Remote GaussDB MySQL-mode DB: 通过 **postgres 协议**连接（示例：`postgres://<host>:8000/jyp_test_m?...`）
- 凭据只放本地 `.env.local`/`.local/e2e/.env`，不写入 git

## Acceptance

- `cargo test -p dt-tests --test integration_test -- mysql_to_gaussdb_mysql::cdc_tests::test::cdc_resume_test --nocapture`
- 真实环境证据归档到 child `raw/`

