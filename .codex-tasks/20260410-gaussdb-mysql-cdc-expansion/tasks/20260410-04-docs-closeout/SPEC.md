# Child Spec

## Title

Docs / runbook / tracker closeout for `GaussDBMySQL CDC Expansion`

## Parent Epic

- `.codex-tasks/20260410-gaussdb-mysql-cdc-expansion/EPIC.md`

## Goal

把本 Epic 的交付（`MySQL -> GaussDBMySQL` CDC basic/type-matrix/resume）同步到 docs 与 tracker：

1. `docs/templates/mysql_to_gaussdb_mysql.md`：明确“目标端走 postgres 协议 + MySQL 兼容模式库”的环境契约，并补齐 CDC 用例入口
2. `docs/agent-summary/gaussdb-progress-tracker.md`：Dashboard 与 checklist 对齐到当前真实进度与证据入口
3. `docs/agent-summary/gaussdb-e2e-test-plan.md`：把 MySQL->GaussDBMySQL CDC 用例纳入 Quick/Full 与 Batch A/B

## Constraints

- 不提交凭据；不在 docs 中写明任何口令
- 证据链接指向 `.codex-tasks/.../PROGRESS.md` 或脱敏 `raw/` 片段

## Acceptance

- `rg -n "GaussDBMySQL CDC Expansion|mysql_to_gaussdb_mysql.*cdc" docs/agent-summary/plan.md docs/agent-summary/gaussdb-progress-tracker.md docs/agent-summary/gaussdb-e2e-test-plan.md docs/templates/mysql_to_gaussdb_mysql.md`

