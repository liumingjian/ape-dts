# Progress Log

> Epic coordination log. Child task details live under `tasks/<child>/`.

## Context Recovery Block

- **Epic**: `20260331-gaussdb-cdc-resilience`
- **Truth file**: `.codex-tasks/20260331-gaussdb-cdc-resilience/SUBTASKS.csv`

## 2026-03-31

- Epic scaffolding created.

## 2026-04-01

- Verified real-environment E2E (no-pollution) for GaussDBPg -> PG CDC:
  - `bash scripts/e2e/gaussdb_to_pg_cdc.sh` (basic) PASS
  - `TEST_RESUME=1 bash scripts/e2e/gaussdb_to_pg_cdc.sh` PASS
  - `TEST_NEG_SLOT_ACTIVE=1 bash scripts/e2e/gaussdb_to_pg_cdc.sh` PASS (includes precheck fail-fast)
  - `TEST_NEG_NO_REPL_USER=1 bash scripts/e2e/gaussdb_to_pg_cdc.sh` PASS (precheck fail-fast)
- Verified dt-tests resume:
  - `cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::cdc_tests::test::cdc_resume_test --nocapture` PASS
- Verified dt-tests basic:
  - `cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::cdc_tests::test::cdc_basic_test --nocapture` PASS
- Verified dt-precheck unit tests:
  - `cargo test -p dt-precheck` PASS

Pending:

- Failover validation currently blocked by CM cluster state:
  - `cm_ctl query -Cv` shows `cluster_state: Degraded` (node3 Down), and switchover to node2 fails (promotion timeout / CM busy).
- Once cluster is restored to `Normal`, run:
  - `TEST_FAILOVER=1 bash scripts/e2e/gaussdb_to_pg_cdc.sh`
  - `ENABLE_GAUSSDB_FAILOVER_TEST=1 cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::cdc_tests::test::cdc_failover_test --nocapture`

Notes:

- Failover tests/scripts now allow degraded CM cluster (some nodes Down) by default, as long as a healthy standby exists, and enforce "best-effort restore + verify no new unhealthy nodes introduced". To require a fully healthy cluster, set `GAUSSDB_CM_REQUIRE_HEALTHY=1`.
