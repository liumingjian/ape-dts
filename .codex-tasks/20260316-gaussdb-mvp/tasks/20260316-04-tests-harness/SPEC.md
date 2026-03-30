# 子任务 4：测试与联调 Harness（single-full）

## Goal

- 将 GaussDB PG 兼容模式纳入现有 `dt-tests` 测试骨架：
  - `PG -> GaussDB`：snapshot / struct / check
  - `GaussDB -> PG`：snapshot / cdc / check
- 让 `dt-tests` runner 在 `DbType::GaussDBPg` 下复用现有 Pg 连接池与转义规则（不复制实现）
- 在 `dt-tests/tests/.env` 中增加 GaussDB URL 占位变量，允许通过 `.env.local` 覆盖

## Non-Goals

- 不把真实 GaussDB 环境接入公共 CI（真实联调作为人工门禁）
- 不在本子任务中产出完整的 4 组真实联调证据（仅预留归档点/目录）

## Done-When

- `dt-tests/tests/` 下新增 `pg_to_gaussdb/`、`gaussdb_to_pg/` 用例目录与最小配置占位
- `dt-tests` runner 支持 `DbType::GaussDBPg` 的连接池初始化与必要的 extractor 配置分支
- `cargo test -p dt-tests --test integration_test --no-run` 通过

