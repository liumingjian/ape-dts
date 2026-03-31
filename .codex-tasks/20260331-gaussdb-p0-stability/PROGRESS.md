# Progress Log

## 2026-03-31

### Init

- Created task directory `.codex-tasks/20260331-gaussdb-p0-stability/` with `SPEC.md`, `TODO.csv`, `PROGRESS.md`, `raw/`.

### Implementation

- Candidate-first endpoint selection when `gaussdb_pg_candidate_hosts` is set:
  - Prioritize candidates list; base URL is only final fallback.
  - Add sticky endpoint (`last_success_endpoint`) and try it first on reconnect.
- CDC failure diagnostics:
  - Decode errors now log `LSN + category + raw_snippet(<=200)` and fail fast.
  - Unsupported `op_type` surfaces a DDL-like hint message in decoder.
- Runbook updated to document candidate-first + sticky behavior and fail-fast decode policy.

### Validation

Commands:

```bash
cargo test -p dt-connector gaussdb -- --nocapture
cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::cdc_tests::test::cdc_basic_test --nocapture
```

Results:

- `dt-connector` gaussdb unit tests: **PASS** (19 tests)
- `dt-tests` `gaussdb_to_pg::cdc_basic_test`: **PASS** (took ~102s)

Key runtime evidence (excerpt):

- Endpoint selection logs show candidate-first probe order and selected HA `port+1`.
- Replication streaming starts on `host:8001` and advances `Position::PgCdc` until row compare passes.
