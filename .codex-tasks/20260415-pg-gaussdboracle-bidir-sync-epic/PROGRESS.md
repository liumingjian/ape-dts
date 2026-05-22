# Progress Log

## Context Recovery Block

- **Task**: `PG ↔ GaussDBOracle 双向同步（non-CDC + CDC）Epic`
- **Shape**: `epic`
- **Truth file**: `.codex-tasks/20260415-pg-gaussdboracle-bidir-sync-epic/SUBTASKS.csv`
- **Current status**: `DONE`
- **Current**: `ALL SUBTASKS DONE`
- **Key context**:
  - 子任务 `#2` 已跑通 `pg_to_gaussdb_oracle` CDC basic，并补强了 dt-tests 的 gaussdb 环境抗抖（清理 log_dir + compare/fetch RW 选主）。
  - 子任务 `#3` 已跑通 `gaussdb_oracle_to_pg` CDC basic（oracle-mode testdb 支持 `mppdb_decoding`）。
  - 远端 `GaussDB Oracle compatibility mode`（oracle-mode `testdb`）已验证 `PG -> GaussDBOracle` 的 non-CDC basic（见 `.codex-tasks/20260415-gaussdb-oracle-next/PROGRESS.md`）。
  - Oracle XE 本机 docker 环境与 Oracle connector bootstrap 已在独立 Epic 交付（见 `.codex-tasks/20260415-oracle-gaussdboracle-bidir-epic/`）。
- **Next action**: 提交代码变更，确保 `dt-tests` 的 `pg_to_gaussdb_oracle::cdc_basic_test` / `gaussdb_oracle_to_pg::cdc_basic_test` 可作为持续回归入口。
