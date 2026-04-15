# Task Specification

## Task Shape

- **Shape**: `single-full`

## Goals

- 更新 `docs/agent-summary/` 下的计划与 tracker 文档，使其准确反映当前 `GaussDB -> MySQL` bootstrap 状态。
- 明确快照/对账/CDC 的验证命令入口与关键约定（router/db_map、目标库命名）。
- 让后续读者在不翻代码的情况下，能快速复现证据并推进下一阶段工作。

## Non-Goals

- 不在本 child 内改运行时或测试代码（只做文档收口）。
- 不重写整套文档结构，只做最小必要补充/修正。

## Constraints

- 文档内容不得写入任何真实凭据（遵守 `.env/.env.local` 契约）。
- 以已验证的 dt-tests 入口为唯一证据来源（避免“未跑过”的描述）。

## Deliverables

- 更新以下文档：
  - `docs/agent-summary/plan.md`
  - `docs/agent-summary/gaussdb-progress-tracker.md`
  - `docs/agent-summary/gaussdb-e2e-test-plan.md`

## Done-When

- [x] 上述三份文档包含 `GaussDB -> MySQL` bootstrap 的最新状态与验证命令
- [x] `rg -n "GaussDB -> MySQL|gaussdb_to_mysql" ...` 能命中新增内容

## Final Validation Command

```bash
rg -n "GaussDB -> MySQL|gaussdb_to_mysql" docs/agent-summary/plan.md docs/agent-summary/gaussdb-progress-tracker.md docs/agent-summary/gaussdb-e2e-test-plan.md
```
