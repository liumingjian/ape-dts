# SPEC — GaussDBOracle -> PG (non-CDC basic)

## Goal

补齐能力矩阵缺口：新增 `GaussDBOracle -> PG` 的 non-CDC 基础链路，并提供可回归的 `dt-tests` 用例：

- snapshot smoke
- struct basic
- check basic

## Scope

- `dt-tests`：
  - 新增模块 `gaussdb_oracle_to_pg`（`mod.rs + snapshot_tests/struct_tests/check_tests`）
  - 新增 fixtures：
    - `snapshot/smoke_test/*`
    - `struct/basic_test/*`
    - `check/basic_test/*`
- 更新 `docs/agent-summary`：
  - `gaussdb-progress-tracker.md` 增加 `GaussDBOracle -> PG` 行（矩阵补齐）
  - `gaussdb-e2e-test-plan.md` 增加 Quick Gate 命令
  - `gaussdb-oracle-roadmap.md` 增加双向 non-CDC 入口与证据链接

## Constraints / Assumptions

- 复用远端 `GaussDBOracle`（oracle-mode `testdb`），通过 `dt-tests/tests/.env.local` 覆盖 `gaussdb_oracle_sinker_*`。
- 复用本机 PG docker（默认 `5434`）作为目标端。

## Acceptance Criteria

- `cargo test -p dt-tests --test integration_test -- gaussdb_oracle_to_pg::snapshot_tests::test::smoke_test --nocapture` PASS
- `cargo test -p dt-tests --test integration_test -- gaussdb_oracle_to_pg::struct_tests::test::struct_basic_test --nocapture` PASS
- `cargo test -p dt-tests --test integration_test -- gaussdb_oracle_to_pg::check_tests::test::check_basic_test --nocapture` PASS
- `docs/agent-summary` 中可检索到入口（`rg -n "gaussdb_oracle_to_pg" docs/agent-summary/*.md` 命中）

