# Progress Log

## 2026-03-31

### Init

- Inputs detected: `.local/e2e/.env` exists, with prior run dirs under `.local/e2e/gaussdb_to_pg_cdc_*`.
- E2E runner script: `scripts/e2e/gaussdb_to_pg_cdc.sh` (no-pollution).

### Changes

- Updated `scripts/e2e/gaussdb_to_pg_cdc.sh`:
  - Accept `.local/e2e/.env` style `SRC_GAUSS_URL` (URL-with-auth), parse into `SRC_GAUSS_USERNAME/PASSWORD/PRIMARY_HOSTPORT/DB`.
  - Add `TEST_STICKY_RECONNECT=1` option to terminate the replication backend and assert reconnect uses sticky endpoint selection.
  - Add log assertions for candidate-first selection + HA port + NoTLS.

Note:

- Initial attempt failed because macOS bash does not support `mapfile`; replaced with bash-3.2-friendly parsing.

### Validation

Command:

```bash
set -a; source .local/e2e/.env; set +a
export TEST_STICKY_RECONNECT=1
bash scripts/e2e/gaussdb_to_pg_cdc.sh
```

Result: **PASS**

- Run dir: `.local/e2e/gaussdb_to_pg_cdc_20260331_160001`
- Candidate-first evidence + HA port + NoTLS + sticky reconnect evidence archived:
  - `raw/20260331_default_log_excerpt.log`
  - `raw/20260331_task_config_redacted.ini`
