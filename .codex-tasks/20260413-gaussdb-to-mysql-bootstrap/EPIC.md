# GaussDB -> MySQL Bootstrap（Epic）

## Goal

在现有 `DbType::GaussDBPg` 主线能力已经闭环的基础上，启动
`GaussDB -> MySQL` 反向链路的第一轮 bootstrap，优先形成可验证、可恢复的
最小数据面闭环：

1. 打通 `GaussDB -> MySQL` snapshot basic。
2. 在 snapshot 稳定后补齐 `check basic`。
3. 复用现有 `GaussDBCdcExtractor` + `Mysql` sink 路径推进 `cdc basic`。
4. 最终把证据、tracker、e2e 入口收口到统一文档中。

## Non-Goals

- 不做 `GaussDBOracle`。
- 不与 `SHA256` 解锁混做。
- 不在本 Epic 内承诺完整的 struct / DDL / cycle / cascade 能力。
- 不回头继续扩 `MySQL -> GaussDBMySQL` 已闭环分支，除非出现阻塞当前 epic 的共享缺陷。

## Constraints / Decisions

- 源端继续使用当前已验证可用的 `GaussDBPg` 环境。
- 目标端继续使用本机或测试环境的 MySQL 8。
- 优先复用现有运行时：
  - snapshot/check：`Pg*` extractor + `Mysql` sink/check
  - cdc：`GaussDBCdcExtractor` + `Mysql` sink
- 第一个 child 只做 `snapshot basic`，先验证“GaussDB 作为源端 + MySQL 作为目标端”的最小主路径。
- 真环境验证以现有 `dt-tests` 约定的 `.env.local` / `.env` 契约为准，不向仓库写入凭据。

## Risk Assessment

- `GaussDB -> MySQL` 目前没有既有 `dt-tests` 套件，首个 blocker 可能出现在测试路由而不是运行时。
- source 为 PG 语义、target 为 MySQL 语义，schema/db 映射可能需要最小约定或路由修正。
- 真实环境下可能出现 GaussDB 连接抖动、共享环境污染或 MySQL 目标残留数据问题。

## Child Deliverables

- child 1: `GaussDB→MySQL snapshot basic`
- child 2: `GaussDB→MySQL check basic`
- child 3: `GaussDB→MySQL cdc basic`
- child 4: `docs/tracker/e2e 收口`

## Dependency Notes

- child 2/3 依赖 child 1，先确认反向主路径的基础建模成立。
- child 4 依赖 child 1/2/3，用来统一收口证据和执行入口。

## Done-When

- `SUBTASKS.csv` 中所有 active child 为 `DONE`
- 每个 child 都有自动化验证结果，真实环境证据尽量归档
- `docs/agent-summary/gaussdb-progress-tracker.md` 和相关计划文档能反映新的 active epic
