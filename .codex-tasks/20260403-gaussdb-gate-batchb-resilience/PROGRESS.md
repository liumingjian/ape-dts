# Progress Log

## Context Recovery Block

- **Task**: `20260403-gaussdb-gate-batchb-resilience`
- **Shape**: `single-full`
- **Truth file**: `.codex-tasks/20260403-gaussdb-gate-batchb-resilience/TODO.csv`
- **Evidence dirs**:
  - `raw/batch-b/` (dt-tests enhanced regression)
  - `raw/resilience/` (dt-tests failover + e2e script)

## 2026-04-03

### Env Check

- Local docker PG15 started on `127.0.0.1:5434` (container: `ape-dts-pg15`).
- Env files present:
  - `.local/e2e/.env` (local-only, not committed)
  - `dt-tests/tests/.env.local` (local-only, not committed)
- CM cluster health snippet captured (for failover gating).
- Evidence: `raw/env_check.log`

### Batch B

- Ran 6 enhanced dt-tests and archived logs under `raw/batch-b/`.
- Result: `6/6 PASS` (`summary.tsv` contains no `FAIL` rows).
- Notable long-tail behavior:
  - `gaussdb_to_pg::cdc_tests::test::cdc_resume_test` took `780.27s`.
  - The test hit repeated transient issues before recovering: candidate primary drift (`10.250.0.30`/`10.250.0.51`), replication `Connection reset by peer (os error 54)`, intermittent `Connection refused (os error 61)`, and repeated `resume_phase1` compare timeouts.
  - Final attempt succeeded: phase1 inserts were eventually sinked, restart resumed from checkpoint LSN, and phase2 update/delete converged on target.
- Evidence:
  - `raw/batch-b/summary.tsv`
  - `raw/batch-b/gaussdb_to_pg_cdc_resume.log`

### Resilience Blocker

- `dt-tests` failover could not start from the current shell:
  - command: `GAUSSDB_CM_REQUIRE_HEALTHY=1 ENABLE_GAUSSDB_FAILOVER_TEST=1 cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::cdc_tests::test::cdc_failover_test --nocapture`
  - result: all candidate connections to `10.250.0.52/30/51:8000` failed immediately with `Operation not permitted (os error 1)`.
- A minimal e2e repro hit the same blocker:
  - command: `GAUSSDB_CM_REQUIRE_HEALTHY=1 SKIP_DOCKER_PG=1 bash scripts/e2e/gaussdb_to_pg_cdc.sh`
  - result: script failed to detect RW primary within `60s`; every `psql` probe to the candidate list returned `Operation not permitted`.
- Conclusion:
  - Batch B used a shell started before the sandbox/network policy changed and was able to finish.
  - New shells in the current session cannot reach the remote GaussDB environment, so failover/e2e resilience cannot be meaningfully executed right now.
  - Stopped after the minimal e2e repro to avoid producing four more redundant failures.
- Evidence:
  - `raw/resilience/summary.tsv`
  - `raw/resilience/dt_tests_failover.log`
  - `raw/resilience/e2e_basic.log`

### Resilience Retry After Access Restore

- User restored remote access and the gate resumed from `Step 3`.
- Re-ran:
  - `GAUSSDB_CM_REQUIRE_HEALTHY=1 ENABLE_GAUSSDB_FAILOVER_TEST=1 cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::cdc_tests::test::cdc_failover_test --nocapture`
- Observed behavior:
  - The test started from `node 2 / 10.250.0.30` as RW primary.
  - CDC replication started on `10.250.0.30:8001`, then after switchover reconnected on `10.250.0.51:8001`, proving the real failover path was exercised.
  - During post-failover verification the test repeatedly saw source/target mismatch on `public.gaussdb_cdc_failover`.
  - Cleanup/restore was not stable: repeated `cm_ctl` busy warnings (`another command(11) is running`) and restore convergence timeouts prevented returning to the original primary within the test budget.
  - Final test failure:
    - `cm primary node is not restored after failover test (orig_primary_node=2, final_primary_node=1)`
- Environment recovery performed immediately after the failed test:
  - Confirmed cluster ended at `node 1 / 10.250.0.51` as `Primary Normal`.
  - Ran manual restore from current primary host `10.250.0.51`:
    - `cm_ctl switchover -n 2 -D/data/cluster/var/lib/engine/data1/data/dn_6002 -t 600`
  - Manual restore succeeded and cluster returned to:
    - `node 2 / 10.250.0.30` => `Primary Normal`
- Conclusion:
  - The previous resilience blocker was environmental only, but after access restoration the real product/test issue is now visible: failover succeeds, automatic restore is still flaky and currently fails the `dt-tests` resilience gate.
  - Proceed to e2e script coverage from the manually restored healthy state to finish collecting full gate evidence.
- Evidence:
  - `raw/resilience/dt_tests_failover.log`
  - `raw/resilience/manual_restore_to_n2.log`

### E2E Script Coverage

- Re-ran the full script matrix from a manually restored healthy baseline (`node 2 / 10.250.0.30` as `Primary Normal`):
  - `bash scripts/e2e/gaussdb_to_pg_cdc.sh`
  - `TEST_RESUME=1 bash scripts/e2e/gaussdb_to_pg_cdc.sh`
  - `TEST_NEG_SLOT_ACTIVE=1 bash scripts/e2e/gaussdb_to_pg_cdc.sh`
  - `TEST_NEG_NO_REPL_USER=1 bash scripts/e2e/gaussdb_to_pg_cdc.sh`
  - `TEST_FAILOVER=1 bash scripts/e2e/gaussdb_to_pg_cdc.sh`
- Result: `5/5 PASS`.
- Highlights:
  - `basic`: source DML converged on target and log assertions for candidate-first + HA port + NoTLS passed.
  - `resume`: checkpoint LSN was captured before stop, restart logged `cdc recovery from lsn:[...]`, and phase2 DML converged on target.
  - `slot active`: second task / precheck path produced the expected slot-active evidence and failed fast.
  - `no replication user`: precheck failed fast with insufficient-permission evidence and the temporary role was cleaned up.
  - `failover`: script switched primary from `node 2 / 10.250.0.30` to `node 1 / 10.250.0.51`, CDC reconnected on `10.250.0.51:8001`, phase2 DML converged, and cleanup restored the cluster back to `node 2`.
- Important contrast vs `dt-tests`:
  - The script path completed the same operational failover flow successfully, while `dt-tests` still fails on restore verification.
  - This narrows the current resilience gap to the `dt-tests`/test-runner side or to timing differences in restore verification rather than the basic e2e operational path.
- Evidence:
  - `raw/resilience/summary.tsv`
  - `raw/resilience/e2e_basic.log`
  - `raw/resilience/e2e_resume.log`
  - `raw/resilience/e2e_neg_slot_active.log`
  - `raw/resilience/e2e_neg_no_repl_user.log`
  - `raw/resilience/e2e_failover.log`

### No-pollution verification

- Final CM state after all resilience runs:
  - `node 1 / 192.168.1.51 / 6001 => Standby Normal`
  - `node 2 / 192.168.1.30 / 6002 => Primary Normal`
  - `node 3 / 192.168.1.52 / 6003 => Standby Normal`
- Final source-side cleanup checks on restored primary `10.250.0.30:8000`:
  - `pg_replication_slots where slot_name like 'ape_manual_gaussdb_to_pg_%' => 0`
  - `pg_namespace where nspname='ape_dts_manual' => 0`
  - `pg_roles where rolname like 'ape_dts_no_repl_%' => 0`
- Final destination-side cleanup checks on local PG:
  - `information_schema.schemata where schema_name='ape_dts_manual' => 0`
- Evidence:
  - `raw/resilience/manual_restore_to_n2.log`
  - final `cm_ctl query -Cv | grep -A5 "Datanode State"` output recorded during verification
