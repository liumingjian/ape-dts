#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
STATE_DIR="$ROOT/.local/self-check/gaussdb-oracle-console"
PID_DIR="$STATE_DIR/pids"
LOG_DIR="$STATE_DIR/logs"
CONSOLE_PORT="${CONSOLE_PORT:-18082}"
WEB_PORT="${WEB_PORT:-5176}"
CONSOLE_BIND_URL="http://127.0.0.1:${CONSOLE_PORT}"
WEB_BIND_URL="http://127.0.0.1:${WEB_PORT}"
CONSOLE_URL="http://localhost:${CONSOLE_PORT}"
WEB_URL="http://localhost:${WEB_PORT}"
CONSOLE_DB_PATH="$STATE_DIR/console.db"
RUN_DATA_DIR="$STATE_DIR/runs"
ENGINE_PATH="${APE_DTS_BINARY_PATH:-$ROOT/target/debug/dt-main}"
ORACLE_CONTAINER="${ORACLE_SQLPLUS_DOCKER_CONTAINER:-oracle-xe-local}"
ENV_LOCAL="$ROOT/dt-tests/tests/.env.local"
TEST_NAME="gaussdb_snapshot_cdc_e2e::test::gaussdb_oracle_snapshot_cdc_via_console_and_playwright"
PREPARE_TEST_NAME="gaussdb_snapshot_cdc_e2e::test::gaussdb_oracle_self_check_prepare_normal"
MUTATE_TEST_NAME="gaussdb_snapshot_cdc_e2e::test::gaussdb_oracle_self_check_mutate_normal"
VERIFY_TEST_NAME="gaussdb_snapshot_cdc_e2e::test::gaussdb_oracle_self_check_verify_normal"

log() {
  printf '[gaussdb-oracle-self-check] %s\n' "$*"
}

die() {
  printf 'ERROR: %s\n' "$*" >&2
  exit 1
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing dependency: $1"
}

env_file_value() {
  local key="$1"
  [[ -f "$ENV_LOCAL" ]] || return 0
  awk -F= -v key="$key" '$1 == key {print substr($0, length(key) + 2); exit}' "$ENV_LOCAL"
}

gaussdb_candidate_hosts() {
  if [[ -n "${gaussdb_pg_candidate_hosts:-}" ]]; then
    printf '%s' "$gaussdb_pg_candidate_hosts"
    return
  fi
  if [[ -n "${GAUSSDB_PG_CANDIDATE_HOSTS:-}" ]]; then
    printf '%s' "$GAUSSDB_PG_CANDIDATE_HOSTS"
    return
  fi
  env_file_value gaussdb_pg_candidate_hosts
}

start_detached() {
  local name="$1"
  local log_file="$2"
  local pid
  shift 2

  pid="$(python3 - "$log_file" "$@" <<'PY'
import os
import subprocess
import sys

log_path = sys.argv[1]
command = sys.argv[2:]
with open(log_path, "ab", buffering=0) as log:
    process = subprocess.Popen(
        command,
        stdin=subprocess.DEVNULL,
        stdout=log,
        stderr=subprocess.STDOUT,
        start_new_session=True,
    )
print(process.pid)
PY
)"
  printf '%s' "$pid" >"$(pid_file "$name")"
  log "started $name pid=$(read_pid "$(pid_file "$name")")"
}

pid_file() {
  printf '%s/%s.pid' "$PID_DIR" "$1"
}

is_pid_alive() {
  local pid="$1"
  [[ -n "$pid" ]] && kill -0 "$pid" >/dev/null 2>&1
}

read_pid() {
  local file="$1"
  [[ -f "$file" ]] && cat "$file" || true
}

stop_pid_name() {
  local name="$1"
  local file pid
  file="$(pid_file "$name")"
  pid="$(read_pid "$file")"
  if is_pid_alive "$pid"; then
    log "stopping $name pid=$pid"
    kill "$pid" >/dev/null 2>&1 || true
    for _ in $(seq 1 20); do
      is_pid_alive "$pid" || break
      sleep 0.5
    done
    is_pid_alive "$pid" && kill -9 "$pid" >/dev/null 2>&1 || true
  else
    log "$name is not running"
  fi
  rm -f "$file"
}

stop_port_listener() {
  local port="$1"
  local pids
  pids="$(lsof -nP -iTCP:"$port" -sTCP:LISTEN 2>/dev/null | awk 'NR > 1 {print $2}' | sort -u || true)"
  if [[ -z "$pids" ]]; then
    log "port $port has no listener"
    return
  fi
  for pid in $pids; do
    log "stopping leftover listener on port $port pid=$pid"
    kill "$pid" >/dev/null 2>&1 || true
  done
  for _ in $(seq 1 20); do
    if ! port_is_listening "$port"; then
      return 0
    fi
    sleep 0.5
  done
  for pid in $pids; do
    is_pid_alive "$pid" && kill -9 "$pid" >/dev/null 2>&1 || true
  done
}

stop_run_processes() {
  local pids
  pids="$(
    ps -axo pid=,command= |
      awk -v run_dir="$RUN_DATA_DIR/" '$0 ~ run_dir && $0 ~ /\/task_config\.ini/ {print $1}' |
      sort -u
  )"
  if [[ -z "$pids" ]]; then
    log "no self-check run processes are running"
    return
  fi
  for pid in $pids; do
    log "stopping self-check run process pid=$pid"
    kill "$pid" >/dev/null 2>&1 || true
  done
  for _ in $(seq 1 20); do
    local alive=""
    for pid in $pids; do
      if is_pid_alive "$pid"; then
        alive=1
        break
      fi
    done
    [[ -z "$alive" ]] && return
    sleep 0.5
  done
  for pid in $pids; do
    is_pid_alive "$pid" && kill -9 "$pid" >/dev/null 2>&1 || true
  done
}

port_is_listening() {
  local port="$1"
  lsof -nP -iTCP:"$port" -sTCP:LISTEN >/dev/null 2>&1
}

require_free_port() {
  local port="$1"
  if port_is_listening "$port"; then
    die "port $port is already in use; set CONSOLE_PORT/WEB_PORT or run destroy for this self-check"
  fi
}

wait_http() {
  local url="$1"
  local name="$2"
  for _ in $(seq 1 90); do
    if curl -fsS "$url" >/dev/null 2>&1; then
      log "$name is ready: $url"
      return 0
    fi
    sleep 1
  done
  die "$name did not become ready: $url"
}

seed_self_check_license() {
  [[ -f "$CONSOLE_DB_PATH" ]] || die "missing Console DB: $CONSOLE_DB_PATH"
  local times now expire_at
  times="$(python3 - <<'PY'
from datetime import datetime, timedelta, timezone

now = datetime.now(timezone.utc)
expire_at = now + timedelta(days=30)
fmt = lambda dt: dt.isoformat(timespec="milliseconds").replace("+00:00", "Z")
print(fmt(now))
print(fmt(expire_at))
PY
)"
  now="$(printf '%s\n' "$times" | sed -n '1p')"
  expire_at="$(printf '%s\n' "$times" | sed -n '2p')"
  sqlite3 "$CONSOLE_DB_PATH" <<SQL
DELETE FROM licenses WHERE id = 'self-check-license';
INSERT INTO licenses (
  id, sku, max_tasks, expire_at, activated_at,
  activation_code_hash, granted_to, created_at, updated_at
) VALUES (
  'self-check-license',
  'Console Self Check',
  1000,
  '$expire_at',
  '$now',
  NULL,
  'gaussdb-oracle-console-self-check',
  '$now',
  '$now'
);
SQL
  log "self-check license activated in isolated Console DB"
}

teardown() {
  stop_run_processes
  stop_pid_name web
  stop_pid_name console
  stop_port_listener "$WEB_PORT"
  stop_port_listener "$CONSOLE_PORT"
  log "teardown complete"
}

precheck() {
  cd "$ROOT"
  require_cmd cargo
  require_cmd curl
  require_cmd docker
  require_cmd lsof
  require_cmd npm
  require_cmd playwright-cli
  require_cmd python3
  require_cmd sqlite3

  [[ -f dt-tests/tests/.env.local ]] || die "missing dt-tests/tests/.env.local"
  [[ -d web-prototype/node_modules ]] || die "missing web-prototype/node_modules; run npm install in web-prototype"

  log "building debug dt-main for current workspace"
  cargo build -p dt-main
  [[ -x "$ENGINE_PATH" ]] || die "dt-main is still not executable: $ENGINE_PATH"
  [[ -n "$(gaussdb_candidate_hosts)" ]] ||
    die "missing gaussdb_pg_candidate_hosts; set it or add it to $ENV_LOCAL"

  docker inspect "$ORACLE_CONTAINER" >/dev/null 2>&1 ||
    die "missing Oracle sqlplus container: $ORACLE_CONTAINER"

  if ! docker inspect -f '{{.State.Running}}' "$ORACLE_CONTAINER" 2>/dev/null | grep -qx true; then
    die "Oracle sqlplus container is not running: $ORACLE_CONTAINER"
  fi

  require_free_port "$CONSOLE_PORT"
  require_free_port "$WEB_PORT"
  log "precheck passed"
}

init() {
  cd "$ROOT"
  teardown
  precheck
  mkdir -p "$PID_DIR" "$LOG_DIR" "$RUN_DATA_DIR"
  rm -f "$CONSOLE_DB_PATH"
  local candidate_hosts
  candidate_hosts="$(gaussdb_candidate_hosts)"

  log "starting Console on $CONSOLE_URL"
  start_detached console "$LOG_DIR/console.log" \
    env \
    CONSOLE_BIND_ADDR="127.0.0.1:${CONSOLE_PORT}" \
    CONSOLE_DB_PATH="$CONSOLE_DB_PATH" \
    CONSOLE_RUN_DATA_DIR="$RUN_DATA_DIR" \
    CONSOLE_ALLOW_PRIVATE_ENDPOINTS=1 \
    ORACLE_SQLPLUS_DOCKER_CONTAINER="$ORACLE_CONTAINER" \
    APE_DTS_BINARY_PATH="$ENGINE_PATH" \
    gaussdb_pg_candidate_hosts="$candidate_hosts" \
    cargo run -p dt-console-server
  wait_http "$CONSOLE_BIND_URL/api/healthz" "Console"
  seed_self_check_license

  log "starting Web UI on $WEB_URL"
  start_detached web "$LOG_DIR/web.log" \
    env \
    VITE_USE_MOCK=false \
    VITE_API_PROXY_TARGET="$CONSOLE_BIND_URL" \
    npm --prefix "$ROOT/web-prototype" run dev -- --host 127.0.0.1 --port "$WEB_PORT"
  wait_http "$WEB_BIND_URL/login" "Web UI"

  log "open $WEB_URL/login and sign in with admin / admin123"
}

license_self_check() {
  wait_http "$CONSOLE_BIND_URL/api/healthz" "Console"
  seed_self_check_license
}

test_normal() {
  cd "$ROOT"
  [[ -f "$(pid_file console)" ]] || die "Console is not initialized; run init first"
  [[ -f "$(pid_file web)" ]] || die "Web UI is not initialized; run init first"
  wait_http "$CONSOLE_BIND_URL/api/healthz" "Console"
  wait_http "$WEB_BIND_URL/login" "Web UI"

  log "preparing GaussDBOracle <-> Oracle normal scenario data"
  timeout 300 cargo test -p dt-tests --test integration_test -- "$PREPARE_TEST_NAME" --nocapture |
    tee "$LOG_DIR/gaussdb-oracle-prepare.log"

  log "scenario is ready; use the Console UI to create and start both snapshot+cdc tasks"
  log "open $WEB_URL/tasks/snapshot and create manual-gaussdb-oracle-to-oracle plus manual-oracle-to-gaussdb-oracle"
}

mutate_normal() {
  cd "$ROOT"
  log "applying CDC changes to both source sides"
  timeout 300 cargo test -p dt-tests --test integration_test -- "$MUTATE_TEST_NAME" --nocapture |
    tee "$LOG_DIR/gaussdb-oracle-mutate.log"
  log "CDC changes applied; observe running Console tasks, logs, metrics, and history in the Web UI"
}

verify_normal() {
  cd "$ROOT"
  log "verifying GaussDBOracle <-> Oracle final data consistency"
  timeout 300 cargo test -p dt-tests --test integration_test -- "$VERIFY_TEST_NAME" --nocapture |
    tee "$LOG_DIR/gaussdb-oracle-verify.log"
}

e2e_normal() {
  cd "$ROOT"
  [[ -f "$(pid_file console)" ]] || die "Console is not initialized; run init first"
  [[ -f "$(pid_file web)" ]] || die "Web UI is not initialized; run init first"
  wait_http "$CONSOLE_BIND_URL/api/healthz" "Console"
  wait_http "$WEB_BIND_URL/login" "Web UI"

  log "running GaussDBOracle <-> Oracle full+CDC E2E through Console and Playwright"
  APE_DTS_CONSOLE_E2E_WEB_URL="$WEB_URL" \
    APE_DTS_CONSOLE_E2E_RUNS_DIR="$RUN_DATA_DIR" \
    timeout 900 cargo test -p dt-tests --test integration_test -- "$TEST_NAME" --nocapture |
    tee "$LOG_DIR/gaussdb-oracle-e2e.log"

  log "test passed; open $WEB_URL/tasks/snapshot and look for e2e-gaussdb_oracle_to_oracle and e2e-oracle_to_gaussdb_oracle"
}

usage() {
  cat <<USAGE
Usage: bash scripts/e2e/gaussdb_oracle_console_self_check.sh <phase>

Phases:
  teardown   stop self-check Console/Web processes
  precheck   verify local tools, config, Oracle container, and free ports
  init       start isolated Console and Web UI
  license    activate a self-check license in the isolated Console DB
  test normal
             prepare normal scenario data for manual Console self-check
  mutate normal
             apply CDC changes to both source sides after manual tasks are running
  verify normal
             verify final source/target data consistency
  e2e normal
             run the fully automated Console + Playwright regression
  destroy    stop processes and remove self-check state

Defaults:
  CONSOLE_PORT=$CONSOLE_PORT
  WEB_PORT=$WEB_PORT
  ORACLE_SQLPLUS_DOCKER_CONTAINER=$ORACLE_CONTAINER
USAGE
}

destroy() {
  teardown
  rm -rf "$STATE_DIR"
  log "destroy complete"
}

phase="${1:-}"
case "$phase" in
  teardown) teardown ;;
  precheck) precheck ;;
  init) init ;;
  license) license_self_check ;;
  test)
    scenario="${2:-normal}"
    [[ "$scenario" == "normal" ]] || die "unknown scenario: $scenario"
    test_normal
    ;;
  mutate)
    scenario="${2:-normal}"
    [[ "$scenario" == "normal" ]] || die "unknown scenario: $scenario"
    mutate_normal
    ;;
  verify)
    scenario="${2:-normal}"
    [[ "$scenario" == "normal" ]] || die "unknown scenario: $scenario"
    verify_normal
    ;;
  e2e)
    scenario="${2:-normal}"
    [[ "$scenario" == "normal" ]] || die "unknown scenario: $scenario"
    e2e_normal
    ;;
  destroy) destroy ;;
  -h|--help|help|"") usage ;;
  *) usage; die "unknown phase: $phase" ;;
esac
