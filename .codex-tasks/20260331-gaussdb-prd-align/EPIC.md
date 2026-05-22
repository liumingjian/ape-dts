# GaussDB PRD 对齐迭代（Epic）

## Goal

以 `docs/agent-summary/gaussdb-prd.md` 为需求真相源，完成：

1. 建立“Docs + Taskmaster Epic”双轨的全局进度跟踪体系（每个 spec 完成可立即知道后续安排）。
2. 结构同步（Struct）对象扩展：**普通视图 + 物化视图(无数据) + 函数/存储过程(仅 plpgsql+SQL) + routine grants**，并要求 **双向**（PG→GaussDBPg、GaussDBPg→PG）都有 dt-tests 覆盖与真实联调证据。
3. 之后推进：**PG→GaussDB CDC**。
4. `SHA256 auth` 纳入 epic，但在联调环境可用前标记 `BLOCKED`。

## Non-Goals（本 Epic 内）

- 不在本 Epic 内把专网真实环境接入公共 CI。
- 不在本 Epic 内实现 DDL CDC（仍按当前 CDC MVP 策略：不支持则 fail fast）。

## Constraints / Decisions（锁定）

- PRD 为需求真相源；`docs/agent-summary/plan.md` 会更新为 PRD 驱动的迭代计划。
- Tracker 双轨：`docs/agent-summary/gaussdb-progress-tracker.md`（可读） + 本 epic 的 `SUBTASKS.csv`（执行真表）。
- Struct 扩展：view+matview(WITH NO DATA)+routine(plpgsql/sql)+routine grants；双向同时。
- router：不改写定义体内部引用，只改写对象 header（schema/name）。
- matview 已存在：默认跳过（不自动重建）。
- 失败策略：默认严格（`conflict_policy=interrupt`），允许用户配置 `ignore`。

## Done-When

- `SUBTASKS.csv` 中除 `SHA256 auth` 外均为 `DONE`，且每个子任务都有明确验证记录（命令 + 结果）写入对应子任务 `PROGRESS.md`。

