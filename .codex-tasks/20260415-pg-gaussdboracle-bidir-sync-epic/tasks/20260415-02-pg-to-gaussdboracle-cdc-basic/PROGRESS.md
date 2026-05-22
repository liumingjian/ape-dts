# Progress Log

## Context Recovery Block

- **Task**: `PG -> GaussDBOracle (CDC basic)`
- **Shape**: `single-full`
- **Truth file**: `.codex-tasks/20260415-pg-gaussdboracle-bidir-sync-epic/tasks/20260415-02-pg-to-gaussdboracle-cdc-basic/TODO.csv`
- **Current status**: `DONE`
- **Current milestone**: #3 — 运行并通过 `cdc_basic_test`
- **Latest validation**: `cargo test -p dt-tests --test integration_test -- pg_to_gaussdb_oracle::cdc_tests::test::cdc_basic_test --nocapture` (PASS @ 2026-04-15 18:14:47 +0800)
- **Key fix**: `run_cdc_test` 启动前清理 `runtime.log_dir`，避免 compare retry 误判历史 `shutdown triggered by ...`
- **Key fix**: compare/fetch 对 `DbType::GaussDBOracle` 也启用 `gaussdb_pg_candidate_hosts` 的 RW 选主（减少 connection reset/standby 读写问题）
