# Progress Log

## Context Recovery Block

- **Task**: `precheck: candidate 选主严格化 + 权限/slot 检查`
- **Shape**: `single-full`
- **Truth file**: `.codex-tasks/20260331-gaussdb-cdc-resilience/tasks/20260331-03-precheck/TODO.csv`

## 2026-04-01

Code landed:

- Candidate-first primary binding for `DbType::GaussDBPg` + CDC source (`gaussdb_pg_candidate_hosts`):
  - Binds precheck connection to a RW primary (`pg_is_in_recovery=false`), otherwise fail-fast.
- Fail-fast checks:
  - standby/recovery mode
  - permission (need superuser or replication role)
  - slot active (no side effects)
  - HA replication port reachability (`sql_port+1`)

Validation:

- `cargo test -p dt-precheck` PASS
- Real env e2e negatives PASS:
  - `TEST_NEG_SLOT_ACTIVE=1 bash scripts/e2e/gaussdb_to_pg_cdc.sh`
  - `TEST_NEG_NO_REPL_USER=1 bash scripts/e2e/gaussdb_to_pg_cdc.sh`

Evidence (sanitized):

- slot-active precheck: `.codex-tasks/20260331-gaussdb-cdc-resilience/tasks/20260331-02-e2e-script/raw/20260401_e2e_neg_slot_active_precheck_snippet.txt`
- no-repl-user precheck: `.codex-tasks/20260331-gaussdb-cdc-resilience/tasks/20260331-02-e2e-script/raw/20260401_e2e_neg_no_repl_precheck_snippet.txt`
