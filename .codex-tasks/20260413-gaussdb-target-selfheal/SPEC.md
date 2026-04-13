# Task Specification

> Implement GaussDB target (PG-wire) self-healing without VIP/LB: pick RW primary from `gaussdb_pg_candidate_hosts`, and recover from HA switchover by reconnecting automatically.

## Task Shape

- **Shape**: `single-full`

## Goals

- For **target-side PG-wire** sinkers (`DbType::GaussDBPg` and `DbType::GaussDBMySQL`):
  - When env `gaussdb_pg_candidate_hosts` is set (SQL ports list), prefer candidates to select a **RW primary** (by `pg_is_in_recovery=false`).
  - On failover-induced errors (read-only / connection reset / server closed), **self-heal** by re-resolving RW primary and swapping the write pool, then retry.
  - Provide clear logs showing candidate selection and pool switch (without leaking credentials).
- Add **dt-tests** coverage: `mysql_to_gaussdb_mysql` CDC failover test that performs `cm_ctl switchover` and validates the task keeps syncing.

## Non-Goals

- No CDC DDL support changes (unsupported events remain fail-fast elsewhere).
- No new public config items in task config; only optional env vars for timeouts (non-public).
- No changes to cluster/VIP/LB topology (assume none; program must self-heal).

## Constraints

- Never commit or print credentials (SSH passwords, DB passwords). Use local env injection only.
- Failover operations must run `cm_ctl switchover` on the **current primary DN host** (per runbook).
- Ensure test cleanup is best-effort and avoids environment pollution.

## Environment

- **Project root**: `/Users/lmj/projects/ai-project/ape-dts`
- **Language/runtime**: Rust (tokio), sqlx
- **Test framework**: `cargo test` (dt-tests integration harness)

## Deliverables

- Code changes to make PG-wire sinker resilient to GaussDB HA failover (candidate RW + reconnect).
- New dt-tests integration case: `mysql_to_gaussdb_mysql::cdc_tests::test::cdc_failover_test`.
- Task evidence logs recorded under `raw/` (local only; do not commit secrets).

## Done-When

- [ ] `cargo test -p dt-tests --test integration_test -- mysql_to_gaussdb_mysql::cdc_tests::test::cdc_failover_test --nocapture` passes when `ENABLE_GAUSSDB_FAILOVER_TEST=1` and CM env vars are provided.
- [ ] Existing GaussDBMySQL CDC tests still pass (basic/type-matrix/resume).
- [ ] Logs clearly indicate candidate probe order and a pool switch after switchover (no credentials).

## Final Validation Command

```bash
cargo test -p dt-tests --test integration_test -- \
  mysql_to_gaussdb_mysql::cdc_tests::test::cdc_basic_test \
  mysql_to_gaussdb_mysql::cdc_tests::test::cdc_type_matrix_test \
  mysql_to_gaussdb_mysql::cdc_tests::test::cdc_resume_test \
  mysql_to_gaussdb_mysql::cdc_tests::test::cdc_failover_test \
  --nocapture
```

