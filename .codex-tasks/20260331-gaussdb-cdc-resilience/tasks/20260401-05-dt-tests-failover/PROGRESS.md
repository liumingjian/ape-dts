# Progress Log

## Context Recovery Block

- **Task**: `dt-tests: gaussdb_to_pg cdc failover_test (cm_ctl switchover)`
- **Shape**: `single-full`
- **Truth file**: `.codex-tasks/20260331-gaussdb-cdc-resilience/tasks/20260401-05-dt-tests-failover/TODO.csv`

## 2026-04-01

Implementation notes:

- Added `gaussdb_to_pg/cdc/failover_test` (2-phase DML around switchover).
- Runner now includes `run_cdc_failover_test()`:
  - Detect current RW endpoint from `gaussdb_pg_candidate_hosts`
  - Execute `cm_ctl switchover` on the actual primary DN host (sshpass + `su - Ruby` + `source ~/gauss_env_file`)
  - Verify via `cm_ctl query -Cv | grep -A5 \"Datanode State\"`
  - Assert dt-main reconnect evidence: `gaussdb cdc replication streaming started: <new_host>:<sql_port+1>`
  - Best-effort restore original primary and verify no new unhealthy nodes introduced

Validation:

- Compile-only: `cargo test -p dt-tests --test integration_test --no-run` PASS

Real env status:

- CM reports `cluster_state: Degraded` (node3 Down Unknown/Starting), and `cm_ctl switchover` to node2 failed:
  - `cm_ctl: failed to do switch-over. Wait the candidate to be promoted timeout.`
  - Occasionally also: `cm_ctl: can not do switchover, another command(11) is running.`
- This blocks reliable automated failover validation.

Code adjustments based on the above:

- Default switchover mode changed to non-fast (align with runbook); opt-in fast via `GAUSSDB_CM_SWITCHOVER_FAST=1`.
- Failover test now fails fast when CM is busy (`another command ... is running`) and uses a shorter converge wait on explicit switchover/promotion timeout errors (avoid multi-minute hangs).

## 2026-04-02

Real-environment validation closed after hardening the test against shared-HA timing windows.

Key test-runner adjustments:

- Before `cm_ctl switchover`, close the long-lived source comparison pool so the test itself does not keep an idle SQL session pinned to the old primary.
- For GaussDBPg source reads/writes with `gaussdb_pg_candidate_hosts` configured, always resolve a fresh RW pool on demand.
- In `cm_switchover_until_primary_node()`, resolve the current RW endpoint with retry/wait before issuing the next CM action so transient post-switchover windows do not cause false failures.

Validation timeline:

- `cargo test -p dt-tests --test integration_test --no-run` PASS after each patch round.
- Run8/Run10 showed failover itself could complete, but restore / RW re-resolution still had timing flakes.
- Run11 PASS:
  - Command:
    - `set -a && source .local/e2e/.env && set +a`
    - `ENABLE_GAUSSDB_FAILOVER_TEST=1 GAUSSDB_CM_SWITCHOVER_TIMEOUT_SECS=240 GAUSSDB_CM_SWITCHOVER_CONVERGE_TIMEOUT_SECS=240 GAUSSDB_CM_BUSY_RETRY_MAX=30 cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::cdc_tests::test::cdc_failover_test --nocapture`
  - Evidence:
    - `.codex-tasks/20260331-gaussdb-cdc-resilience/tasks/20260401-05-dt-tests-failover/raw/20260402_dt_tests_failover_run11.stdout.log`
    - `.codex-tasks/20260331-gaussdb-cdc-resilience/tasks/20260401-05-dt-tests-failover/raw/20260402_dt_tests_failover_run11_snippet.txt`
  - Key proof points:
    - replication starts on old primary HA port: `10.250.0.51:8001`
    - after CM switchover, replication reconnects to new primary HA port: `10.250.0.30:8001`
    - phase2 DML (`UPDATE id=2 -> c`, `DELETE id=1`) is captured
    - final compare passes with source/destination row count `1`
    - test exits `... ok`
