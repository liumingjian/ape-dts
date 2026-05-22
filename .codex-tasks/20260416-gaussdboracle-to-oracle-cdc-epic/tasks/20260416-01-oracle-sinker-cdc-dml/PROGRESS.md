# Progress Log

## Context Recovery Block

- **Task**: `OracleSinker 支持 CDC DML（UPDATE/DELETE）`
- **Shape**: `single-full` (Epic child)
- **Parent truth file**: `.codex-tasks/20260416-gaussdboracle-to-oracle-cdc-epic/SUBTASKS.csv`
- **Truth file**: `.codex-tasks/20260416-gaussdboracle-to-oracle-cdc-epic/tasks/20260416-01-oracle-sinker-cdc-dml/TODO.csv`
- **Current status**: `DONE`
- **Next action**: 无（已完成并验证）。

## 2026-04-16

- 已扩展 `OracleSinker` 支持 `RowType::Update` / `RowType::Delete`，并补充最小 SQL 生成单测。
- 验证：`cargo test -p dt-connector` PASS。
