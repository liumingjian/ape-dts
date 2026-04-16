# Progress Log

## Context Recovery Block

- **Task**: `Oracle -> GaussDBOracle CDC basic（trigger-based bootstrap）`
- **Shape**: `single-full` (Epic child)
- **Parent truth file**: `.codex-tasks/20260415-oracle-gaussdboracle-bidir-epic/SUBTASKS.csv`
- **Truth file**: `.codex-tasks/20260415-oracle-gaussdboracle-bidir-epic/tasks/20260416-06-oracle-to-gaussdboracle-cdc-basic/TODO.csv`
- **Current status**: `IN_PROGRESS`
- **Next action**: 先补齐 `dt-common TaskConfig` 对 `oracle + cdc` 的解析与 `ExtractorConfig` 变体，再实现 `OracleCdcExtractor`。

## Notes

- CDC 方案选用 trigger-based，优先保证链路可跑通与可回归；后续如需 LogMiner/OGG 另开 Epic。

