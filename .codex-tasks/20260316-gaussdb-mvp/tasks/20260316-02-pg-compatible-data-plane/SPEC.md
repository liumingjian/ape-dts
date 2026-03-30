# 子任务 2：PG 兼容数据面打通（single-full）

## Goal

- 在不复制 PostgreSQL 实现的前提下，让 `DbType::GaussDBPg` 在非 CDC 场景复用现有 PG 数据面能力：
  - `PG -> GaussDB`：snapshot / struct / check
  - `GaussDB -> PG`：snapshot / check（CDC 见子任务 3）
- 结构同步能力边界明确收敛到仓库现有能力（schema/table/sequence/comment/index/constraint/rbac）

## Non-Goals

- 本子任务不实现 GaussDB CDC（见子任务 3）
- 本子任务不新增/交付 SHA256 认证支持（见子任务 5 路线说明）
- 本子任务不扩展超出现有 PG 结构链路能力的对象同步

## Done-When

- `DbType::GaussDBPg` 在 snapshot/struct/check 路径不因缺少分支而启动失败
- `cargo test -p dt-common -p dt-task` 通过

