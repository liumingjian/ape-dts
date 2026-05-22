# Progress Log

> Keep verbose execution evidence here (commands, outputs, key logs). Do not include secrets.

## 2026-04-13

- Init: created task dir + SPEC/TODO/PROGRESS.

### Evidence (Local, Not Committed)

- Failover test runs:
  - `.codex-tasks/20260413-gaussdb-target-selfheal/raw/20260413_mysql_to_gaussdb_mysql_cdc_failover_test.log` (initial repro, FAIL)
  - `.codex-tasks/20260413-gaussdb-target-selfheal/raw/20260413_mysql_to_gaussdb_mysql_cdc_failover_test.run2.log` (compare ok, cleanup FAIL)
  - `.codex-tasks/20260413-gaussdb-target-selfheal/raw/20260413_mysql_to_gaussdb_mysql_cdc_failover_test.run3.log` (PASS)
- Regression:
  - `.codex-tasks/20260413-gaussdb-target-selfheal/raw/20260413_mysql_to_gaussdb_mysql_cdc_regression_basic_type_matrix_resume.log` (PASS)

### Environment Notes

- Local MySQL8 source container: `ape-dts-mysql8` (port `3311`).
- Target GaussDB is pg-wire, no VIP/LB; failover is performed via CM SSH (password injected via env only).
- Commands were run with:
  - `set -a; source .local/e2e/.env; set +a`
  - `ENABLE_GAUSSDB_FAILOVER_TEST=1`
  - `GAUSSDB_CM_REQUIRE_HEALTHY=1`

### Debug / Fix Log

1) Repro: `mysql_to_gaussdb_mysql::cdc_failover_test` FAIL
- Symptom: pipeline shutdown during Phase2 compare after `cm_ctl switchover`.
- Key error (from `TaskRunner` logs):
  - `pipeline.start returned error: ... cannot execute DELETE in a read-only transaction`
- Root cause: `PgSinker::sink_dml` retry/self-heal loop was bypassed in batch paths because `call_batch_fn!` uses `?` internally, causing an early-return from `sink_dml` and preventing retries.

2) Fix: make batch errors observable to retry loop
- Change: wrap `call_batch_fn!` invocations in an `async { ... }.await` block so the `?` returns an `Err` value to the loop, instead of returning from `sink_dml` directly.

3) Re-run: compare ok, but cleanup FAIL
- Symptom: cleanup `DROP DATABASE` failed with `attempted to acquire a connection on a closed pool`.
- Root cause: failover test intentionally closes the PG comparison pool before switchover; cleanup reused the closed pool for `GaussDBMySQL` targets.

4) Fix: cleanup should recreate pool when needed
- Change: `execute_dst_sqls` (and symmetric `execute_src_sqls`) treat `DbType::GaussDBMySQL + postgres://` as pg-wire gaussdb; when `gaussdb_pg_candidate_hosts` is set, always resolve a fresh RW pool. If the existing pool is closed, re-create a temporary pool for cleanup SQLs.

### Final Validation

- PASS (failover + self-heal):
  - `cargo test -p dt-tests --test integration_test -- mysql_to_gaussdb_mysql::cdc_tests::test::cdc_failover_test --nocapture`
- PASS (regression):
  - `cargo test -p dt-tests --test integration_test -- mysql_to_gaussdb_mysql::cdc_tests::test::cdc_basic_test mysql_to_gaussdb_mysql::cdc_tests::test::cdc_type_matrix_test mysql_to_gaussdb_mysql::cdc_tests::test::cdc_resume_test --nocapture`
