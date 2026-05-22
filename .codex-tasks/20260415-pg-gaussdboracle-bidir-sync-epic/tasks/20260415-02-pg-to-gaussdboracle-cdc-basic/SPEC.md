# SPEC — PG -> GaussDBOracle (CDC basic)

## Goal

补齐 `PG -> GaussDBOracle` 的增量同步入口：新增 `dt-tests` 的 `cdc basic` 用例，验证 `PgCdcExtractor(pgoutput)` + `PgSinker(gaussdb_oracle)` 的 DML 主链路可用（insert/update/delete）。

## Scope

- `dt-tests`：
  - `dt-tests/tests/pg_to_gaussdb_oracle` 新增 `cdc_tests.rs`
  - 新增 fixtures：`dt-tests/tests/pg_to_gaussdb_oracle/cdc/basic_test/*`
- 文档由 epic 的收口任务统一更新（本子任务只负责跑通与证据）。

## Constraints / Assumptions

- 源端 PG：本机 docker（默认 `5434`）并已开启逻辑复制所需配置（沿用现有 pg cdc 用例）。
- 目标端 GaussDBOracle：远端 oracle-mode `testdb`（通过 `.env.local` 注入）。

## Acceptance Criteria

- `cargo test -p dt-tests --test integration_test -- pg_to_gaussdb_oracle::cdc_tests::test::cdc_basic_test --nocapture` PASS

