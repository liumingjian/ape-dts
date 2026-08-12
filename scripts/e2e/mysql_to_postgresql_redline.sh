#!/usr/bin/env bash
set -Eeuo pipefail

sanitize_run_id() {
  local raw="$1"
  local sanitized
  sanitized="$(printf '%s' "$raw" | tr '[:upper:]' '[:lower:]' | tr -cs 'a-z0-9-' '-' | sed -E 's/^-+//; s/-+$//; s/-+/-/g')"
  [[ -n "$sanitized" ]] || return 1
  printf '%s' "${sanitized:0:63}"
}

parse_compose_port() {
  local address="$1"
  local port="${address##*:}"
  [[ "$port" =~ ^[0-9]+$ ]] || return 1
  (( port >= 1 && port <= 65535 )) || return 1
  printf '%s' "$port"
}

parse_master_status() {
  local status="$1"
  local field="$2"
  local filename position _rest
  [[ -n "$status" ]] || return 1
  IFS=$'\t' read -r filename position _rest <<<"$status"
  [[ -n "$filename" && "$position" =~ ^[0-9]+$ ]] || return 1
  case "$field" in
    filename) printf '%s' "$filename" ;;
    position) printf '%s' "$position" ;;
    *) return 1 ;;
  esac
}

expected_rows() {
  case "$1" in
    snapshot)
      printf '%s' $'1|ORD-001|Alice|100.50|created|<NULL>\n2|ORD-002|Bob|220.00|created|snapshot row\n3|ORD-003|Carol|19.99|created|will be deleted'
      ;;
    insert)
      printf '%s' $'1|ORD-001|Alice|100.50|created|<NULL>\n2|ORD-002|Bob|220.00|created|snapshot row\n3|ORD-003|Carol|19.99|created|will be deleted\n4|ORD-004|David|88.80|created|cdc insert'
      ;;
    update)
      printf '%s' $'1|ORD-001|Alice|188.80|paid|cdc update\n2|ORD-002|Bob|220.00|created|snapshot row\n3|ORD-003|Carol|19.99|created|will be deleted\n4|ORD-004|David|88.80|created|cdc insert'
      ;;
    delete|final)
      printf '%s' $'1|ORD-001|Alice|188.80|paid|cdc update\n2|ORD-002|Bob|220.00|created|snapshot row\n4|ORD-004|David|88.80|created|cdc insert'
      ;;
    *) return 1 ;;
  esac
}

mysql_dump_query() {
  printf '%s' "SELECT CONCAT_WS('|', id, order_no, customer_name, CAST(amount AS CHAR), status, COALESCE(note, '<NULL>')) FROM ape_dts_e2e.migration_redline_orders ORDER BY id;"
}

postgres_dump_query() {
  printf '%s' "SELECT id || '|' || order_no || '|' || customer_name || '|' || to_char(amount, 'FM9999999990.00') || '|' || status || '|' || COALESCE(note, '<NULL>') FROM public.migration_redline_orders ORDER BY id;"
}

log() {
  printf '[%s] %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$*"
}

die() {
  FAILURE_REASON="$*"
  printf 'ERROR: %s\n' "$*" >&2
  exit 1
}

# Safety net for stages that fail through `set -e` instead of `die` (a bare
# `mysql_sql` heredoc, a compose call, an assignment from a failing command
# substitution). Without this the run aborts with an empty FAILURE_REASON and
# summary.md shows no cause at all. Recorded unconditionally; write_summary
# only falls back to it when the run actually failed and nothing called `die`.
record_err() {
  local status="$1"
  local lineno="$2"
  local cmd="$3"
  LAST_ERR_REASON="stage ${CURRENT_STAGE:-initialization} aborted: \`${cmd}\` exited with status ${status} (${BASH_SOURCE[0]##*/}:${lineno})"
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing dependency: $1"
}

compose() {
  docker compose -p "$COMPOSE_PROJECT_NAME" -f "$COMPOSE_FILE" "$@"
}

mysql_sql() {
  compose exec -T mysql-source mysql --batch --raw --skip-column-names -uroot -pmysql "$@"
}

postgres_sql() {
  compose exec -T postgresql-target psql -X -v ON_ERROR_STOP=1 -U postgres -d postgres "$@"
}

# Probe seams: overridden by the unit tests to drive readiness sequences.
probe_mysql() {
  mysql_sql -e "SELECT 1" >/dev/null 2>&1
}

probe_postgres() {
  postgres_sql -tA -c "SELECT 1" >/dev/null 2>&1
}

# Prints the compose service's container health: healthy/unhealthy/starting, or
# "none" when the image declares no healthcheck. Empty output means "unknown"
# (container not created yet, or docker inspect failed) and counts as not ready.
service_health() {
  local service="$1"
  local cid
  cid="$(compose ps -q "$service" 2>/dev/null | head -n1)"
  [[ -n "$cid" ]] || return 1
  docker inspect -f '{{if .State.Health}}{{.State.Health.Status}}{{else}}none{{end}}' "$cid" 2>/dev/null
}

services_healthy() {
  local service health
  for service in mysql-source postgresql-target; do
    health="$(service_health "$service" || true)"
    case "$health" in
      healthy|none) ;;
      *) return 1 ;;
    esac
  done
  return 0
}

# The MySQL official image starts a temporary server during initialization and
# then restarts it. A single successful `SELECT 1` can land on that temporary
# server, after which the very next statement dies with ERROR 2002. Readiness
# therefore requires both the compose healthcheck to report healthy and
# DB_READY_STREAK_REQUIRED consecutive successful probes, so the restart window
# breaks the streak instead of being mistaken for a ready database.
wait_for_databases() {
  local deadline=$((SECONDS + DOCKER_TIMEOUT_SECS))
  local streak=0
  local reason="no readiness probe completed"
  while (( SECONDS <= deadline )); do
    if ! services_healthy; then
      streak=0
      reason="container healthcheck not healthy yet"
    elif ! probe_mysql; then
      streak=0
      reason="mysql not accepting connections yet"
    elif ! probe_postgres; then
      streak=0
      reason="postgresql not accepting connections yet"
    else
      streak=$((streak + 1))
      reason="only ${streak}/${DB_READY_STREAK_REQUIRED} consecutive probes succeeded"
      if (( streak >= DB_READY_STREAK_REQUIRED )); then
        log "databases ready (${streak} consecutive probes)"
        return 0
      fi
    fi
    sleep "$DB_READY_PROBE_INTERVAL_SECS"
  done
  die "database readiness exceeded ${DOCKER_TIMEOUT_SECS}s (${reason})"
}

prepare_snapshot_data() {
  mysql_sql <<'SQL' || return 1
DROP DATABASE IF EXISTS ape_dts_e2e;
CREATE DATABASE ape_dts_e2e;
CREATE TABLE ape_dts_e2e.migration_redline_orders (
  id BIGINT PRIMARY KEY,
  order_no VARCHAR(64) NOT NULL,
  customer_name VARCHAR(128) NOT NULL,
  amount DECIMAL(12, 2) NOT NULL,
  status VARCHAR(32) NOT NULL,
  note VARCHAR(255) NULL
);
INSERT INTO ape_dts_e2e.migration_redline_orders
  (id, order_no, customer_name, amount, status, note)
VALUES
  (1, 'ORD-001', 'Alice', 100.50, 'created', NULL),
  (2, 'ORD-002', 'Bob', 220.00, 'created', 'snapshot row'),
  (3, 'ORD-003', 'Carol', 19.99, 'created', 'will be deleted');
SQL

  postgres_sql <<'SQL' || return 1
DROP TABLE IF EXISTS public.migration_redline_orders;
CREATE TABLE public.migration_redline_orders (
  id BIGINT PRIMARY KEY,
  order_no VARCHAR(64) NOT NULL,
  customer_name VARCHAR(128) NOT NULL,
  amount NUMERIC(12, 2) NOT NULL,
  status VARCHAR(32) NOT NULL,
  note VARCHAR(255) NULL
);
SQL
  return 0
}

write_snapshot_config() {
  cat >"$SNAPSHOT_CONFIG" <<EOF
[extractor]
db_type=mysql
extract_type=snapshot
url=mysql://127.0.0.1:${MYSQL_PORT}/ape_dts_e2e
username=root
password=mysql
batch_size=100
max_connections=4

[sinker]
db_type=pg
sink_type=write
url=postgres://127.0.0.1:${POSTGRES_PORT}/postgres?options[statement_timeout]=10s
username=postgres
password=postgres
batch_size=50

[filter]
do_dbs=
ignore_dbs=
do_tbs=ape_dts_e2e.migration_redline_orders
ignore_tbs=
do_events=insert

[router]
db_map=
tb_map=ape_dts_e2e.migration_redline_orders:public.migration_redline_orders
col_map=

[parallelizer]
parallel_type=snapshot
parallel_size=1

[pipeline]
buffer_size=100
checkpoint_interval_secs=1

[runtime]
log_level=info
log4rs_file=${REPO_ROOT}/log4rs.yaml
log_dir=${RUN_DIR}/engine-logs/snapshot
EOF
}

write_cdc_config() {
  local binlog_filename="$1"
  local binlog_position="$2"
  cat >"$CDC_CONFIG" <<EOF
[extractor]
db_type=mysql
extract_type=cdc
binlog_filename=${binlog_filename}
binlog_position=${binlog_position}
server_id=7317
url=mysql://127.0.0.1:${MYSQL_PORT}/ape_dts_e2e
username=root
password=mysql
heartbeat_interval_secs=0
heartbeat_tb=

[filter]
do_dbs=
ignore_dbs=
do_tbs=ape_dts_e2e.migration_redline_orders
ignore_tbs=
do_events=insert,update,delete
do_ddls=

[sinker]
db_type=pg
sink_type=write
url=postgres://127.0.0.1:${POSTGRES_PORT}/postgres?options[statement_timeout]=10s
username=postgres
password=postgres
batch_size=2

[router]
db_map=
tb_map=ape_dts_e2e.migration_redline_orders:public.migration_redline_orders
col_map=

[parallelizer]
parallel_type=rdb_merge
parallel_size=1

[pipeline]
buffer_size=4
checkpoint_interval_secs=1

[runtime]
log_level=info
log4rs_file=${REPO_ROOT}/log4rs.yaml
log_dir=${RUN_DIR}/engine-logs/cdc
EOF
}

capture_master_status() {
  local status=""
  status="$(mysql_sql -e "SHOW MASTER STATUS" 2>/dev/null || true)"
  if [[ -z "$status" ]]; then
    status="$(mysql_sql -e "SHOW BINARY LOG STATUS" 2>/dev/null || true)"
  fi
  [[ -n "$status" ]] || die "MySQL returned no binary log status"
  printf '%s\n' "$status" >"$RUN_DIR/mysql-master-status.tsv"
  BINLOG_FILENAME="$(parse_master_status "$status" filename)" || die "invalid binary log filename"
  BINLOG_POSITION="$(parse_master_status "$status" position)" || die "invalid binary log position"
}

start_cdc() {
  "$DT_MAIN_BIN" "$CDC_CONFIG" >"$RUN_DIR/cdc.stdout.log" 2>"$RUN_DIR/cdc.stderr.log" &
  CDC_PID=$!
  log "cdc dt-main started (pid=$CDC_PID, binlog=$BINLOG_FILENAME:$BINLOG_POSITION)"
}

cdc_is_alive() {
  [[ -n "${CDC_PID:-}" ]] && kill -0 "$CDC_PID" >/dev/null 2>&1
}

require_cdc_alive() {
  if ! cdc_is_alive; then
    set +e
    wait "$CDC_PID"
    CDC_EXIT_CODE=$?
    set -e
    die "cdc dt-main exited before convergence with exit code $CDC_EXIT_CODE"
  fi
}

stop_cdc() {
  cdc_is_alive || return 0
  log "stopping cdc dt-main (pid=$CDC_PID)"
  kill -INT "$CDC_PID" >/dev/null 2>&1 || true
  local deadline=$((SECONDS + STOP_TIMEOUT_SECS))
  while cdc_is_alive && (( SECONDS <= deadline )); do
    sleep 1
  done
  if cdc_is_alive; then
    kill -TERM "$CDC_PID" >/dev/null 2>&1 || true
    sleep 2
  fi
  if cdc_is_alive; then
    kill -KILL "$CDC_PID" >/dev/null 2>&1 || true
  fi
  set +e
  wait "$CDC_PID" >/dev/null 2>&1
  CDC_EXIT_CODE=$?
  set -e
  CDC_PID=""
}

ensure_dt_main() {
  if [[ -n "${DT_MAIN_BIN:-}" ]]; then
    [[ -x "$DT_MAIN_BIN" ]] || die "DT_MAIN_BIN is not executable: $DT_MAIN_BIN"
    return
  fi
  DT_MAIN_BIN="$REPO_ROOT/target/debug/dt-main"
  if [[ ! -x "$DT_MAIN_BIN" ]]; then
    log "building dt-main"
    cargo build -p dt-main
  fi
}

run_with_deadline() {
  local timeout_secs="$1"
  local stdout_log="$2"
  local stderr_log="$3"
  shift 3

  "$@" >"$stdout_log" 2>"$stderr_log" &
  local pid=$!
  local deadline=$((SECONDS + timeout_secs))
  while kill -0 "$pid" >/dev/null 2>&1; do
    if (( SECONDS > deadline )); then
      kill -INT "$pid" >/dev/null 2>&1 || true
      sleep 2
      kill -TERM "$pid" >/dev/null 2>&1 || true
      wait "$pid" >/dev/null 2>&1 || true
      return 124
    fi
    sleep 1
  done
  wait "$pid"
}

dump_mysql() {
  mysql_sql -e "$(mysql_dump_query)" | tr -d '\r'
}

dump_postgres() {
  postgres_sql -tA -c "$(postgres_dump_query)" | tr -d '\r'
}

phase_mismatch_reason() {
  local phase="$1"
  local mysql_dump="$2"
  local postgres_dump="$3"
  local diff_file="$4"
  local expected
  expected="$(expected_rows "$phase")" || { printf '%s' "unknown phase: $phase"; return 1; }
  : >"$diff_file"

  if ! diff -u <(printf '%s\n' "$expected") "$mysql_dump" >>"$diff_file"; then
    printf '%s' "$phase source rows differ from fixed expectation"
    return 1
  fi
  if ! diff -u <(printf '%s\n' "$expected") "$postgres_dump" >>"$diff_file"; then
    printf '%s' "$phase target rows differ from fixed expectation"
    return 1
  fi
  if ! diff -u "$mysql_dump" "$postgres_dump" >>"$diff_file"; then
    printf '%s' "$phase source and target rows differ"
    return 1
  fi
  return 0
}

# Dumps both sides into artifacts and records the mismatch reason; never exits.
# Callers decide what a mismatch means: assert_phase treats it as a failure,
# wait_for_phase's timeout branch only wants the artifacts and the last reason.
dump_phase() {
  local phase="$1"
  local mysql_dump="$RUN_DIR/dumps/${phase}-mysql.tsv"
  local postgres_dump="$RUN_DIR/dumps/${phase}-postgresql.tsv"
  PHASE_DIFF_FILE="$RUN_DIR/diffs/${phase}.diff"
  PHASE_MISMATCH_REASON=""

  if ! dump_mysql >"$mysql_dump"; then
    PHASE_MISMATCH_REASON="$phase source dump failed (mysql unreachable)"
    return 1
  fi
  if ! dump_postgres >"$postgres_dump"; then
    PHASE_MISMATCH_REASON="$phase target dump failed (postgresql unreachable)"
    return 1
  fi
  PHASE_MISMATCH_REASON="$(phase_mismatch_reason "$phase" "$mysql_dump" "$postgres_dump" "$PHASE_DIFF_FILE")" || return 1
  return 0
}

assert_phase() {
  local phase="$1"
  dump_phase "$phase" || die "${PHASE_MISMATCH_REASON:-$phase verification failed} (diff: ${PHASE_DIFF_FILE:-none})"
  log "$phase verified"
}

phase_matches() {
  local phase="$1"
  local expected mysql_rows postgres_rows
  expected="$(expected_rows "$phase")"
  mysql_rows="$(dump_mysql 2>/dev/null || true)"
  postgres_rows="$(dump_postgres 2>/dev/null || true)"
  [[ "$mysql_rows" == "$expected" && "$postgres_rows" == "$expected" && "$mysql_rows" == "$postgres_rows" ]]
}

wait_for_phase() {
  local phase="$1"
  local timeout_secs="$2"
  local deadline=$((SECONDS + timeout_secs))
  while (( SECONDS <= deadline )); do
    require_cdc_alive
    if phase_matches "$phase"; then
      assert_phase "$phase"
      return 0
    fi
    sleep 1
  done
  dump_phase "$phase" || true
  die "$phase did not converge within ${timeout_secs}s (last observed: ${PHASE_MISMATCH_REASON:-no mismatch at timeout}; diff: ${PHASE_DIFF_FILE:-none})"
}

wait_for_probe() {
  local should_exist="$1"
  local deadline=$((SECONDS + CDC_PROBE_TIMEOUT_SECS))
  local expected="9001|PROBE-9001|Readiness Probe|0.01|probe|<NULL>"
  while (( SECONDS <= deadline )); do
    require_cdc_alive
    local actual
    actual="$(postgres_sql -tA -c "$(postgres_dump_query)" 2>/dev/null | grep '^9001|' || true)"
    if [[ "$should_exist" == "1" && "$actual" == "$expected" ]]; then
      return 0
    fi
    if [[ "$should_exist" == "0" && -z "$actual" ]]; then
      return 0
    fi
    sleep 1
  done
  die "CDC probe did not reach expected state within ${CDC_PROBE_TIMEOUT_SECS}s"
}

run_cdc_scenario() {
  CURRENT_STAGE="cdc-position"
  capture_master_status
  write_cdc_config "$BINLOG_FILENAME" "$BINLOG_POSITION"
  start_cdc

  CURRENT_STAGE="cdc-probe-insert"
  mysql_sql -e "INSERT INTO ape_dts_e2e.migration_redline_orders VALUES (9001, 'PROBE-9001', 'Readiness Probe', 0.01, 'probe', NULL);"
  wait_for_probe 1
  CURRENT_STAGE="cdc-probe-delete"
  mysql_sql -e "DELETE FROM ape_dts_e2e.migration_redline_orders WHERE id = 9001;"
  wait_for_probe 0
  log "cdc probe verified"

  CURRENT_STAGE="cdc-insert"
  mysql_sql -e "INSERT INTO ape_dts_e2e.migration_redline_orders VALUES (4, 'ORD-004', 'David', 88.80, 'created', 'cdc insert');"
  wait_for_phase insert "$CRUD_TIMEOUT_SECS"

  CURRENT_STAGE="cdc-update"
  mysql_sql -e "UPDATE ape_dts_e2e.migration_redline_orders SET amount = 188.80, status = 'paid', note = 'cdc update' WHERE id = 1;"
  wait_for_phase update "$CRUD_TIMEOUT_SECS"

  CURRENT_STAGE="cdc-delete"
  mysql_sql -e "DELETE FROM ape_dts_e2e.migration_redline_orders WHERE id = 3;"
  wait_for_phase delete "$CRUD_TIMEOUT_SECS"

  CURRENT_STAGE="final-verify"
  wait_for_phase final "$FINAL_TIMEOUT_SECS"
}

collect_diagnostics() {
  [[ "${COMPOSE_STARTED:-0}" == "1" ]] || return 0
  mkdir -p "$RUN_DIR/docker"
  compose ps -a >"$RUN_DIR/docker/compose-ps.log" 2>&1 || true
  compose logs --no-color mysql-source >"$RUN_DIR/docker/mysql.log" 2>&1 || true
  compose logs --no-color postgresql-target >"$RUN_DIR/docker/postgresql.log" 2>&1 || true
}

# A failed run must never report "none": fall back to whatever the ERR trap
# caught when the failing stage bypassed `die`.
summary_reason() {
  local exit_code="$1"
  if [[ "$exit_code" == "0" ]]; then
    printf '%s' "${FAILURE_REASON:-none}"
    return 0
  fi
  printf '%s' "${FAILURE_REASON:-${LAST_ERR_REASON:-unknown failure (no reason recorded)}}"
}

write_summary() {
  local exit_code="$1"
  {
    printf '# MySQL to PostgreSQL red-line run\n\n'
    printf -- '- Result: %s\n' "$([[ "$exit_code" == "0" ]] && printf PASS || printf FAIL)"
    printf -- '- Stage: %s\n' "${CURRENT_STAGE:-initialization}"
    printf -- '- Reason: %s\n' "$(summary_reason "$exit_code")"
    printf -- '- Snapshot exit code: %s\n' "${SNAPSHOT_EXIT_CODE:-not-run}"
    printf -- '- CDC exit code: %s\n' "${CDC_EXIT_CODE:-not-stopped}"
    printf -- '- Artifacts: `%s`\n' "$RUN_DIR"
  } >"$RUN_DIR/summary.md"
}

cleanup() {
  local exit_code=$?
  trap - EXIT INT TERM ERR
  set +e
  collect_diagnostics
  stop_cdc
  write_summary "$exit_code"
  if [[ "${KEEP_ENV:-0}" == "1" && "${COMPOSE_STARTED:-0}" == "1" ]]; then
    log "KEEP_ENV=1: database containers retained"
    log "compose project: $COMPOSE_PROJECT_NAME"
    log "mysql: 127.0.0.1:${MYSQL_PORT:-unknown}"
    log "postgresql: 127.0.0.1:${POSTGRES_PORT:-unknown}"
    log "cleanup: docker compose -p '$COMPOSE_PROJECT_NAME' -f '$COMPOSE_FILE' down -v --remove-orphans"
  elif [[ "${COMPOSE_STARTED:-0}" == "1" ]]; then
    compose down -v --remove-orphans >/dev/null 2>&1 || true
  fi
  log "artifacts: $RUN_DIR"
  exit "$exit_code"
}

main() {
  REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
  COMPOSE_FILE="$REPO_ROOT/scripts/e2e/docker-compose.mysql-to-postgresql-redline.yml"
  DOCKER_TIMEOUT_SECS="${DOCKER_TIMEOUT_SECS:-90}"
  DB_READY_STREAK_REQUIRED="${DB_READY_STREAK_REQUIRED:-3}"
  DB_READY_PROBE_INTERVAL_SECS="${DB_READY_PROBE_INTERVAL_SECS:-1}"
  SNAPSHOT_TIMEOUT_SECS="${SNAPSHOT_TIMEOUT_SECS:-120}"
  CDC_PROBE_TIMEOUT_SECS="${CDC_PROBE_TIMEOUT_SECS:-60}"
  CRUD_TIMEOUT_SECS="${CRUD_TIMEOUT_SECS:-30}"
  FINAL_TIMEOUT_SECS="${FINAL_TIMEOUT_SECS:-60}"
  STOP_TIMEOUT_SECS="${STOP_TIMEOUT_SECS:-15}"
  KEEP_ENV="${KEEP_ENV:-0}"

  require_cmd docker
  docker compose version >/dev/null 2>&1 || die "docker compose is unavailable"
  require_cmd cargo
  require_cmd diff

  local generated_id
  generated_id="ape-dts-mysql-pg-$(date '+%Y%m%d-%H%M%S')-$$"
  COMPOSE_PROJECT_NAME="$(sanitize_run_id "${COMPOSE_PROJECT_NAME:-$generated_id}")"
  RUN_DIR="${RUN_DIR:-$REPO_ROOT/.local/e2e/mysql-to-postgresql/$COMPOSE_PROJECT_NAME}"
  SNAPSHOT_CONFIG="$RUN_DIR/snapshot-task.ini"
  CDC_CONFIG="$RUN_DIR/cdc-task.ini"
  CDC_PID=""
  FAILURE_REASON=""
  LAST_ERR_REASON=""
  mkdir -p "$RUN_DIR"/{docker,dumps,diffs,engine-logs/snapshot,engine-logs/cdc}

  trap 'record_err "$?" "$LINENO" "$BASH_COMMAND"' ERR
  trap cleanup EXIT
  trap 'exit 130' INT
  trap 'exit 143' TERM

  CURRENT_STAGE="docker-start"
  log "artifacts: $RUN_DIR"
  compose up -d || die "docker compose up failed"
  COMPOSE_STARTED=1
  wait_for_databases
  MYSQL_PORT="$(parse_compose_port "$(compose port mysql-source 3306)")" || die "could not resolve the published mysql port"
  POSTGRES_PORT="$(parse_compose_port "$(compose port postgresql-target 5432)")" || die "could not resolve the published postgresql port"

  CURRENT_STAGE="snapshot-prepare"
  prepare_snapshot_data || die "schema and fixture preparation failed (databases went away after readiness)"
  write_snapshot_config
  ensure_dt_main

  CURRENT_STAGE="snapshot-run"
  set +e
  run_with_deadline "$SNAPSHOT_TIMEOUT_SECS" "$RUN_DIR/snapshot.stdout.log" "$RUN_DIR/snapshot.stderr.log" "$DT_MAIN_BIN" "$SNAPSHOT_CONFIG"
  SNAPSHOT_EXIT_CODE=$?
  set -e
  [[ "$SNAPSHOT_EXIT_CODE" == "0" ]] || die "snapshot dt-main failed with exit code $SNAPSHOT_EXIT_CODE"

  CURRENT_STAGE="snapshot-verify"
  assert_phase snapshot

  run_cdc_scenario
  CURRENT_STAGE="complete"
  log "MySQL to PostgreSQL red-line succeeded"
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  main "$@"
fi
