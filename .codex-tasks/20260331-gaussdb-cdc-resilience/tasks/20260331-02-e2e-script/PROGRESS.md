# Progress Log

## Context Recovery Block

- **Task**: `e2e: gaussdb_to_pg cdc failover+resume+negatives`
- **Shape**: `single-full`
- **Truth file**: `.codex-tasks/20260331-gaussdb-cdc-resilience/tasks/20260331-02-e2e-script/TODO.csv`

## 2026-04-01

Real-environment E2E runs (all no-pollution, with cleanup trap):

- Basic:
  - Command: `bash scripts/e2e/gaussdb_to_pg_cdc.sh`
  - Result: PASS
  - Evidence (sanitized): `.codex-tasks/20260331-gaussdb-cdc-resilience/tasks/20260331-02-e2e-script/raw/20260401_e2e_basic_default_log_snippet.txt`
- Resume:
  - Command: `TEST_RESUME=1 bash scripts/e2e/gaussdb_to_pg_cdc.sh`
  - Result: PASS
  - Evidence (sanitized): `.codex-tasks/20260331-gaussdb-cdc-resilience/tasks/20260331-02-e2e-script/raw/20260401_e2e_resume.stdout.log`
- Negative (slot active):
  - Command: `TEST_NEG_SLOT_ACTIVE=1 bash scripts/e2e/gaussdb_to_pg_cdc.sh`
  - Result: PASS
  - Evidence (sanitized): `.codex-tasks/20260331-gaussdb-cdc-resilience/tasks/20260331-02-e2e-script/raw/20260401_e2e_neg_slot_active.stdout.log`
- Negative (no replication privilege user):
  - Command: `TEST_NEG_NO_REPL_USER=1 bash scripts/e2e/gaussdb_to_pg_cdc.sh`
  - Result: PASS
  - Evidence (sanitized): `.codex-tasks/20260331-gaussdb-cdc-resilience/tasks/20260331-02-e2e-script/raw/20260401_e2e_neg_no_repl.stdout.log`

Pending:

- Failover E2E:
  - Command: `TEST_FAILOVER=1 bash scripts/e2e/gaussdb_to_pg_cdc.sh`
  - Current blocker: CM cluster is `cluster_state: Degraded` (node3 Down), and `cm_ctl switchover` fails (candidate promotion timeout / CM busy).
  - Next action: restore cluster to `Normal` then rerun failover E2E for evidence.
