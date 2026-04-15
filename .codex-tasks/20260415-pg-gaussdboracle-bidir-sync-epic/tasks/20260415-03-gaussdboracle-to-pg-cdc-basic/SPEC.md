# SPEC — GaussDBOracle -> PG (CDC basic)

## Goal

补齐 `GaussDBOracle -> PG` 的增量同步入口：在 `dt-common/dt-task` 启用 `DbType::GaussDBOracle + extract_type=cdc`，并新增 `dt-tests` 的 `cdc basic` 用例验证主链路（insert/update/delete）。

## Scope

- `dt-common`：
  - `task_config` 支持 `GaussDBOracle + extract_type=cdc`（复用 `ExtractorConfig::GaussDBCdc`，底层 `mppdb_decoding`）
- `dt-tests`：
  - 新增 `gaussdb_oracle_to_pg/cdc/basic_test/*` fixtures
  - 新增 `gaussdb_oracle_to_pg::cdc_tests::cdc_basic_test`

## Constraints / Assumptions

- 源端 GaussDBOracle：远端 oracle-mode `testdb`（通过 `dt-tests/tests/.env.local` 注入，禁止提交凭据）。
- 目标端 PG：本机 docker（默认 `5434`）。
- `mppdb_decoding` 在 oracle-mode 下的可用性未知：若环境不支持，应保留失败证据并在 Epic 中标记 `BLOCKED/FAILED`，同时给出后续方案（例如仅承诺 PG->GaussDBOracle CDC）。

## Acceptance Criteria

- `cargo test -p dt-tests --test integration_test -- gaussdb_oracle_to_pg::cdc_tests::test::cdc_basic_test --nocapture` PASS
- 或：明确的环境不支持证据（错误日志 + 说明），并在 Epic `SUBTASKS.csv` 中标记为 `FAILED/BLOCKED`

