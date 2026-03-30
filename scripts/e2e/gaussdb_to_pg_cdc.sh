#!/usr/bin/env bash
set -euo pipefail

#
# GaussDB (PG-compatible) -> Postgres CDC E2E (no-pollution)
#
# This script will:
# - (optional) start a local Postgres15 docker container on 127.0.0.1:5434
# - cleanup source/destination test objects to ensure a clean start
# - create source/destination schema/table
# - start dt-main with a generated task_config.ini (written under .local/, gitignored)
# - run source DML (insert/update/delete)
# - assert destination final state
# - ALWAYS cleanup (trap): stop dt-main, drop slot, drop schema/table, remove container (if started)
#
# Required:
# - psql, cargo, docker (unless SKIP_DOCKER_PG=1)
# - GaussDB credentials (env or .local/manual/gaussdb_to_pg_cdc.ini fallback)
#
# Example:
#   export gaussdb_pg_candidate_hosts="10.250.0.52:8000,10.250.0.30:8000,10.250.0.51:8000"
#   export SRC_GAUSS_PRIMARY_HOSTPORT="10.250.0.51:8000"
#   export SRC_GAUSS_USERNAME="root"
#   export SRC_GAUSS_PASSWORD="***"
#   bash scripts/e2e/gaussdb_to_pg_cdc.sh
#

log() {
  local ts
  ts="$(date '+%Y-%m-%d %H:%M:%S')"
  echo "[$ts] $*"
}

die() {
  echo "ERROR: $*" >&2
  exit 1
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing dependency: $1"
}

sanitize_slot_name() {
  local raw="$1"
  # START_REPLICATION SLOT <name> is sensitive to special chars on some servers.
  # Keep it strict and predictable for e2e: [A-Za-z0-9_], max 63 chars.
  local sanitized
  sanitized="$(echo -n "$raw" | tr -c 'A-Za-z0-9_' '_' | sed -E 's/^_+//; s/_+$//')"
  sanitized="${sanitized:0:63}"
  if [[ -z "$sanitized" ]]; then
    sanitized="ape_e2e_gaussdb_to_pg_$(date +%Y%m%d_%H%M%S)"
    sanitized="${sanitized:0:63}"
  fi
  echo -n "$sanitized"
}

parse_ini_value() {
  local file="$1"
  local key="$2"
  # naive ini key=value parser, ignores comments and sections
  grep -E "^[[:space:]]*${key}[[:space:]]*=" "$file" 2>/dev/null | head -n 1 | sed -E "s/^[^=]*=[[:space:]]*//"
}

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

# Inputs (with defaults)
: "${TEST_SCHEMA:=ape_dts_manual}"
: "${TEST_TABLE:=gaussdb_to_pg_cdc_basic}"

: "${SRC_GAUSS_PRIMARY_HOSTPORT:=10.250.0.51:8000}"
: "${SRC_GAUSS_DB:=postgres}"

: "${DST_PG_URL:=postgres://127.0.0.1:5434/postgres?options[statement_timeout]=10s}"
: "${DST_PG_USERNAME:=postgres}"
: "${DST_PG_PASSWORD:=postgres}"

: "${SKIP_DOCKER_PG:=0}"
: "${PG_CONTAINER_NAME:=ape-dts-pg15}"
: "${PG_IMAGE:=postgres:15}"

: "${SLOT_NAME:=ape_e2e_gaussdb_to_pg_$(date +%Y%m%d_%H%M%S)}"
RAW_SLOT_NAME="$SLOT_NAME"
SLOT_NAME="$(sanitize_slot_name "$SLOT_NAME")"
if [[ "$SLOT_NAME" != "$RAW_SLOT_NAME" ]]; then
  log "SLOT_NAME sanitized: '${RAW_SLOT_NAME}' -> '${SLOT_NAME}'"
fi

# Convenience: allow uppercase alias.
if [[ -n "${GAUSSDB_PG_CANDIDATE_HOSTS:-}" && -z "${gaussdb_pg_candidate_hosts:-}" ]]; then
  export gaussdb_pg_candidate_hosts="$GAUSSDB_PG_CANDIDATE_HOSTS"
fi

RUN_ID="$(date +%Y%m%d_%H%M%S)"
RUN_DIR="${RUN_DIR:-$REPO_ROOT/.local/e2e/gaussdb_to_pg_cdc_${RUN_ID}}"
mkdir -p "$RUN_DIR"

LOG_DIR="$RUN_DIR/logs"
mkdir -p "$LOG_DIR"

DTMAIN_STDOUT_LOG="$RUN_DIR/dt-main.stdout.log"
DTMAIN_STDERR_LOG="$RUN_DIR/dt-main.stderr.log"
DTMAIN_PID=""
STARTED_DOCKER_PG=0

# For psql, strip query string by default (libpq may not understand options[...]=...).
DST_PG_PSQL_URL="${DST_PG_PSQL_URL:-${DST_PG_URL%%\?*}}"
SRC_GAUSS_PSQL_URL="${SRC_GAUSS_PSQL_URL:-postgres://${SRC_GAUSS_PRIMARY_HOSTPORT}/${SRC_GAUSS_DB}}"

# Credentials: prefer explicit env, then fall back to .local/manual/gaussdb_to_pg_cdc.ini (local-only).
if [[ -z "${SRC_GAUSS_USERNAME:-}" || -z "${SRC_GAUSS_PASSWORD:-}" ]]; then
  local_ini="$REPO_ROOT/.local/manual/gaussdb_to_pg_cdc.ini"
  if [[ -f "$local_ini" ]]; then
    SRC_GAUSS_USERNAME="${SRC_GAUSS_USERNAME:-$(parse_ini_value "$local_ini" "username")}"
    SRC_GAUSS_PASSWORD="${SRC_GAUSS_PASSWORD:-$(parse_ini_value "$local_ini" "password")}"
  fi
fi
if [[ -z "${SRC_GAUSS_USERNAME:-}" || -z "${SRC_GAUSS_PASSWORD:-}" ]]; then
  die "missing SRC_GAUSS_USERNAME/SRC_GAUSS_PASSWORD (or provide .local/manual/gaussdb_to_pg_cdc.ini)"
fi

psql_src() {
  PGPASSWORD="$SRC_GAUSS_PASSWORD" psql "$SRC_GAUSS_PSQL_URL" -X -v ON_ERROR_STOP=1 -U "$SRC_GAUSS_USERNAME" "$@"
}

psql_dst() {
  PGPASSWORD="$DST_PG_PASSWORD" psql "$DST_PG_PSQL_URL" -X -v ON_ERROR_STOP=1 -U "$DST_PG_USERNAME" "$@"
}

stop_dt_main() {
  if [[ -n "$DTMAIN_PID" ]] && kill -0 "$DTMAIN_PID" >/dev/null 2>&1; then
    log "stopping dt-main (pid=$DTMAIN_PID)"
    kill "$DTMAIN_PID" >/dev/null 2>&1 || true

    local wait_secs=8
    for _ in $(seq 1 "$wait_secs"); do
      if ! kill -0 "$DTMAIN_PID" >/dev/null 2>&1; then
        return 0
      fi
      sleep 1
    done

    log "dt-main still running, force kill (pid=$DTMAIN_PID)"
    kill -9 "$DTMAIN_PID" >/dev/null 2>&1 || true
  fi
}

cleanup_src() {
  log "cleanup source: drop table/schema + drop slot (best-effort)"
  psql_src -c "DROP TABLE IF EXISTS ${TEST_SCHEMA}.${TEST_TABLE};" >/dev/null 2>&1 || true
  psql_src -c "DROP SCHEMA IF EXISTS ${TEST_SCHEMA} CASCADE;" >/dev/null 2>&1 || true
  psql_src -c "SELECT pg_drop_replication_slot(slot_name) FROM pg_replication_slots WHERE slot_name = '${SLOT_NAME}';" >/dev/null 2>&1 || true
}

cleanup_dst() {
  log "cleanup destination: drop table/schema (best-effort)"
  psql_dst -c "DROP TABLE IF EXISTS ${TEST_SCHEMA}.${TEST_TABLE};" >/dev/null 2>&1 || true
  psql_dst -c "DROP SCHEMA IF EXISTS ${TEST_SCHEMA} CASCADE;" >/dev/null 2>&1 || true
}

cleanup_pg_container() {
  if [[ "$STARTED_DOCKER_PG" == "1" ]]; then
    log "cleanup docker postgres container: ${PG_CONTAINER_NAME}"
    docker rm -f "$PG_CONTAINER_NAME" >/dev/null 2>&1 || true
  fi
}

cleanup() {
  set +e
  stop_dt_main
  cleanup_src
  cleanup_dst
  cleanup_pg_container
}

trap cleanup EXIT INT TERM

start_pg_container() {
  if [[ "$SKIP_DOCKER_PG" == "1" ]]; then
    log "SKIP_DOCKER_PG=1, skip starting docker postgres"
    return 0
  fi

  require_cmd docker
  log "starting docker postgres (${PG_IMAGE}) on 127.0.0.1:5434 (container=${PG_CONTAINER_NAME})"
  docker rm -f "$PG_CONTAINER_NAME" >/dev/null 2>&1 || true
  docker run -d --name "$PG_CONTAINER_NAME" \
    -e POSTGRES_PASSWORD="$DST_PG_PASSWORD" \
    -p 5434:5432 \
    "$PG_IMAGE" >/dev/null

  STARTED_DOCKER_PG=1

  log "waiting for postgres to be ready..."
  for _ in $(seq 1 60); do
    if psql_dst -c "SELECT 1;" >/dev/null 2>&1; then
      log "postgres is ready"
      return 0
    fi
    sleep 1
  done
  die "postgres not ready on 127.0.0.1:5434 after 60s"
}

prepare_tables() {
  log "prepare source table: ${TEST_SCHEMA}.${TEST_TABLE}"
  psql_src \
    -c "DROP SCHEMA IF EXISTS ${TEST_SCHEMA} CASCADE;" \
    -c "CREATE SCHEMA ${TEST_SCHEMA};" \
    -c "CREATE TABLE ${TEST_SCHEMA}.${TEST_TABLE} (id INTEGER PRIMARY KEY, val TEXT);"

  log "prepare destination table: ${TEST_SCHEMA}.${TEST_TABLE}"
  psql_dst \
    -c "DROP SCHEMA IF EXISTS ${TEST_SCHEMA} CASCADE;" \
    -c "CREATE SCHEMA ${TEST_SCHEMA};" \
    -c "CREATE TABLE ${TEST_SCHEMA}.${TEST_TABLE} (id INTEGER PRIMARY KEY, val TEXT);"
}

write_dt_main_config() {
  local config_path="$1"

  # dt-main expects URL without auth, and username/password as separate keys.
  local extractor_url="postgres://${SRC_GAUSS_PRIMARY_HOSTPORT}/${SRC_GAUSS_DB}?options[statement_timeout]=10s"

  cat >"$config_path" <<EOF
[extractor]
db_type=gaussdb_pg
extract_type=cdc
url=${extractor_url}
username=${SRC_GAUSS_USERNAME}
password=${SRC_GAUSS_PASSWORD}
slot_name=${SLOT_NAME}
start_lsn=
recreate_slot_if_exists=false
keepalive_interval_secs=10
heartbeat_interval_secs=0
heartbeat_tb=

[filter]
do_dbs=
ignore_dbs=
do_tbs=${TEST_SCHEMA}.${TEST_TABLE}
ignore_tbs=
do_events=insert,update,delete

[sinker]
db_type=pg
sink_type=write
url=${DST_PG_URL}
username=${DST_PG_USERNAME}
password=${DST_PG_PASSWORD}
batch_size=2

[router]
db_map=
tb_map=
col_map=

[parallelizer]
parallel_type=rdb_merge
parallel_size=1

[pipeline]
buffer_size=4
checkpoint_interval_secs=1

[runtime]
log_level=info
log4rs_file=./log4rs.yaml
log_dir=${LOG_DIR}
EOF
}

start_dt_main() {
  local config_path="$1"
  require_cmd cargo

  log "starting dt-main (config=${config_path})"
  log "dt-main logs: ${LOG_DIR}"
  (
    cd "$REPO_ROOT"
    cargo run -p dt-main -- "$config_path" >"$DTMAIN_STDOUT_LOG" 2>"$DTMAIN_STDERR_LOG"
  ) &
  DTMAIN_PID="$!"
}

wait_for_slot() {
  log "waiting for replication slot to appear on source (slot=${SLOT_NAME})"
  for _ in $(seq 1 60); do
    if psql_src -tA -c "SELECT 1 FROM pg_replication_slots WHERE slot_name='${SLOT_NAME}' LIMIT 1;" 2>/dev/null | grep -qx "1"; then
      log "slot is visible"
      return 0
    fi
    sleep 1
  done
  log "dt-main stdout (tail):"
  tail -n 80 "$DTMAIN_STDOUT_LOG" || true
  log "dt-main stderr (tail):"
  tail -n 80 "$DTMAIN_STDERR_LOG" || true
  die "slot did not appear within 60s (slot=${SLOT_NAME})"
}

run_source_dml() {
  log "run source DML (insert/update/delete)"
  psql_src -c "INSERT INTO ${TEST_SCHEMA}.${TEST_TABLE} (id, val) VALUES (1, 'a'), (2, 'b');"
  psql_src -c "UPDATE ${TEST_SCHEMA}.${TEST_TABLE} SET val = 'c' WHERE id = 2;"
  psql_src -c "DELETE FROM ${TEST_SCHEMA}.${TEST_TABLE} WHERE id = 1;"
}

assert_destination() {
  log "assert destination final state (expect: only row (2,'c'))"
  for _ in $(seq 1 60); do
    local out
    out="$(psql_dst -tA -F '|' -c "SELECT id, val FROM ${TEST_SCHEMA}.${TEST_TABLE} ORDER BY id;" 2>/dev/null || true)"
    if [[ "$out" == "2|c" ]]; then
      log "assert ok: ${out}"
      return 0
    fi
    sleep 2
  done

  log "assert failed, destination rows:"
  psql_dst -c "SELECT id, val FROM ${TEST_SCHEMA}.${TEST_TABLE} ORDER BY id;" || true
  log "dt-main stdout (tail):"
  tail -n 120 "$DTMAIN_STDOUT_LOG" || true
  log "dt-main stderr (tail):"
  tail -n 120 "$DTMAIN_STDERR_LOG" || true
  die "destination did not reach expected state within timeout"
}

main() {
  require_cmd psql

  log "run_dir: ${RUN_DIR}"
  log "source: ${SRC_GAUSS_PRIMARY_HOSTPORT}/${SRC_GAUSS_DB} (user=${SRC_GAUSS_USERNAME})"
  log "destination: ${DST_PG_PSQL_URL} (user=${DST_PG_USERNAME})"
  log "test objects: ${TEST_SCHEMA}.${TEST_TABLE} (slot=${SLOT_NAME})"
  if [[ -n "${gaussdb_pg_candidate_hosts:-}" ]]; then
    log "gaussdb_pg_candidate_hosts: ${gaussdb_pg_candidate_hosts}"
  fi

  start_pg_container

  # Ensure clean start even when reusing an existing destination or previous failed runs.
  cleanup_src
  cleanup_dst

  prepare_tables

  local config_path="$RUN_DIR/task_config.ini"
  write_dt_main_config "$config_path"

  start_dt_main "$config_path"
  wait_for_slot

  run_source_dml
  assert_destination

  log "e2e succeeded"
}

main "$@"
