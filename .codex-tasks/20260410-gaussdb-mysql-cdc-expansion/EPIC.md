# GaussDBMySQL CDC Expansion（Epic）

## Goal

在 `GaussDBMySQL Bootstrap` 已完成的基础上，继续把 `MySQL -> GaussDBMySQL`
从“非 CDC 首波能力”推进到“具备基础增量同步能力”的第二阶段：

1. 打通 `MySQL -> GaussDBMySQL` CDC basic 主路径。
2. 补齐类型矩阵 / 特殊值在 CDC 路径下的真实行为验证。
3. 补齐基础 resilience / negative / docs 证据，使其具备可回归、可排障的交付形态。

## Non-Goals

- 不做 `GaussDBMySQL -> MySQL` 源端抽取。
- 不做 `GaussDBOracle`。
- 不在本 Epic 内承诺完整的 cycle/cascade/复杂 DDL CDC 能力。
- 不与 `SHA256` 解锁混做。

## Constraints / Decisions

- 源端继续使用本机 Docker MySQL 8。
- 目标端继续使用 `sql_compatibility=M` 的 GaussDB 数据库，并通过
  `postgres://.../<mysql-compatible-db>` 接入。
- 沿用已经验证过的运行时模型：
  - `DbType::GaussDBMySQL` 描述目标数据库兼容模式
  - 连接协议由 URL 决定，当前真实环境为 pg wire
- 第一个 child 只做 `cdc basic`：
  - DML 为主
  - 先不扩 DDL
  - 先不扩 resume/failover

## Done-When

- `SUBTASKS.csv` 中所有 active child 均为 `DONE`
- 每个 child 都有自动化验证和真实环境证据
- `docs/agent-summary/gaussdb-progress-tracker.md` 能明确显示
  `MySQL -> GaussDBMySQL` 的 CDC 进度与证据入口
