# Progress Log

## Context Recovery Block

- **Task**: `20260403-gaussdb-dt-failover-restore`
- **Shape**: `single-full`
- **Truth file**: `.codex-tasks/20260403-gaussdb-dt-failover-restore/TODO.csv`
- **Primary evidence**:
  - current failure: `.codex-tasks/20260403-gaussdb-gate-batchb-resilience/raw/resilience/dt_tests_failover.log`
  - current e2e success: `.codex-tasks/20260403-gaussdb-gate-batchb-resilience/raw/resilience/e2e_failover.log`
  - historical dt-tests pass: `.codex-tasks/20260331-gaussdb-cdc-resilience/tasks/20260401-05-dt-tests-failover/raw/20260402_dt_tests_failover_run11.stdout.log`

## 2026-04-03

### Initial Findings

- Current gate failure is not a network issue anymore.
- Real failover did happen and CDC reconnected to the new primary HA port.
- Script-based failover still passes in the same environment.
- Historical `dt-tests` pass required:
  - `GAUSSDB_CM_SWITCHOVER_TIMEOUT_SECS=240`
  - `GAUSSDB_CM_SWITCHOVER_CONVERGE_TIMEOUT_SECS=240`
  - `GAUSSDB_CM_BUSY_RETRY_MAX=30`
- Current default path used `GAUSSDB_CM_BUSY_RETRY_MAX=8` and restore failed after:
  - one promotion-timeout attempt
  - two more `another command(11)` busy retries
- Working hypothesis:
  - the current default restore path is too aggressive for shared HA timing windows
  - especially the busy retry budget and the shortened post-error convergence wait

### Code Changes

- Updated `dt-tests/tests/test_runner/rdb_test_runner.rs`:
  - raised the default `GAUSSDB_CM_BUSY_RETRY_MAX` from `8` to `30`
  - when `action == "restore"` and CM returns transitional errors (`another command is running`, `failed to do switch-over`, `candidate to be promoted timeout`), keep the full convergence wait instead of shrinking it to `30s/60s`
- Validation:
  - `cargo test -p dt-tests --test integration_test --no-run` PASS

### Follow-up Changes

- Extended `run_cdc_failover_test` compare patience in `dt-tests/tests/test_runner/rdb_test_runner.rs`:
  - `failover_phase1` compare wait now uses `max(parse_millis * 2, 60s)`
  - `failover_phase2` compare wait now uses `max(parse_millis * 4, 120s)`
- Rationale:
  - the previous rerun no longer failed on restore ownership drift
  - instead it failed during `failover_phase1` data comparison after a fixed `30s` window
  - in this HA environment, short post-switchover sink stalls should not fail the test before CDC has had enough time to settle
- Validation:
  - `cargo test -p dt-tests --test integration_test --no-run` PASS

### Latest Runtime Attempt

- Command:
  - `set -a; source .local/e2e/.env; set +a; ENABLE_GAUSSDB_FAILOVER_TEST=1 GAUSSDB_CM_REQUIRE_HEALTHY=1 cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::cdc_tests::test::cdc_failover_test --nocapture`
- Result:
  - the test did not reach failover logic in this shell session
  - runner initialization failed immediately while probing `10.250.0.52/30/51:8000`
  - every candidate returned `Operation not permitted (os error 1)`
- Evidence:
  - `.codex-tasks/20260403-gaussdb-dt-failover-restore/raw/dt_tests_failover_after_compare_wait_fix.log`
- Current assessment:
  - code is locally buildable
  - the latest validation is blocked by current Codex-shell remote network restrictions rather than by the failover logic itself

### Follow-up Diagnosis And Fixes

- Once network access recovered, the root cause shifted from pure restore timing to a combination of:
  - reused fixed GaussDB CDC slots across repeated dt-tests runs
  - restore-attempt startup depending too strongly on immediate RW re-resolution
  - final cleanup / final health check being too eager during HA convergence windows
- Additional code changes in `dt-tests/tests/test_runner/rdb_test_runner.rs`:
  - rewrite GaussDB CDC `slot_name` to a unique per-run slot for dt-tests isolation
  - best-effort drop that unique slot during cleanup
  - add fallback host reuse for restore attempts when current RW host cannot be resolved immediately
  - wait for final CM datanode health convergence instead of checking once and failing on transient `Building(0%)`
- Intermediate validation evidence:
  - `raw/dt_tests_basic_after_unique_slot.log`
  - `raw/dt_tests_failover_after_unique_slot.log`
  - `raw/dt_tests_failover_after_resolve_fallback.log`

### Final Validation

- Compile validation:
  - `cargo test -p dt-tests --test integration_test --no-run` PASS
- Final real-env command:
  - `set -a; source .local/e2e/.env; set +a; ENABLE_GAUSSDB_FAILOVER_TEST=1 GAUSSDB_CM_REQUIRE_HEALTHY=1 cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::cdc_tests::test::cdc_failover_test --nocapture`
- Final result:
  - PASS
  - Evidence:
    - `.codex-tasks/20260403-gaussdb-dt-failover-restore/raw/dt_tests_failover_after_health_wait.snippet.txt`
    - local full log: `.codex-tasks/20260403-gaussdb-dt-failover-restore/raw/dt_tests_failover_after_health_wait.log`
- Key proof points from the final passing run:
  - source CDC uses an isolated per-run slot
  - failover path reconnects from `10.250.0.51:8001` to `10.250.0.30:8001`
  - phase2 DML still reaches the sink and final compare passes
  - restore may still report a transient promotion timeout on attempt 1, but later convergence succeeds
  - cleanup finishes with `cleanup gaussdb cdc slot ok`
  - final test exits `... ok`

### Final Assessment

- The remaining `dt-tests cdc_failover_test` red point is closed in the current worktree.
- At this point, `GaussDBPg -> PG CDC P1` should be considered complete again from an implementation/evidence standpoint.
