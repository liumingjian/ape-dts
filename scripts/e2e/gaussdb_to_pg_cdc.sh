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

parse_pg_url_parts() {
  local url="$1"
  python3 - "$url" <<'PY'
import sys
from urllib.parse import urlparse, unquote

u = sys.argv[1]
p = urlparse(u)
user = unquote(p.username or "")
pwd = unquote(p.password or "")
host = p.hostname or ""
port = p.port or 5432
db = (p.path or "").lstrip("/") or "postgres"
print("\t".join([user, pwd, host, str(port), db]))
PY
}

sanitize_url_for_log() {
  local url="$1"
  # Remove any userinfo in URL, e.g. postgres://user:pass@host:port/db -> postgres://host:port/db
  echo -n "$url" | sed -E 's#(postgres(ql)?://)[^@/]*@#\1#'
}

wait_for_log() {
  local file="$1"
  local pattern="$2"
  local timeout_secs="${3:-60}"
  for _ in $(seq 1 "$timeout_secs"); do
    if [[ -f "$file" ]] && grep -Fq "$pattern" "$file" 2>/dev/null; then
      return 0
    fi
    sleep 1
  done
  return 1
}

wait_for_replication_backend_pid() {
  local timeout_secs="${1:-60}"
  local pid=""
  for _ in $(seq 1 "$timeout_secs"); do
    pid="$(psql_src -tA -c "SELECT pid FROM pg_stat_activity WHERE application_name = 'gaussdb-replication' ORDER BY backend_start DESC LIMIT 1;" 2>/dev/null | tr -d '[:space:]' || true)"
    if [[ -n "$pid" ]]; then
      echo -n "$pid"
      return 0
    fi
    sleep 1
  done
  return 1
}

terminate_replication_backend() {
  local pid="$1"
  log "terminate gaussdb replication backend to force reconnect (pid=$pid)"
  # best-effort: may require elevated privileges on some environments.
  if ! psql_src -c "SELECT pg_terminate_backend(${pid});" >/dev/null 2>&1; then
    log "WARN: failed to terminate replication backend (pid=$pid); sticky reconnect test will be skipped"
    return 1
  fi
  return 0
}

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

# Optional: load local e2e env file (gitignored).
# - Default enabled so teammates can run without exporting many vars.
# - Set USE_LOCAL_E2E_ENV=0 to opt out.
DEFAULT_E2E_ENV_FILE="$REPO_ROOT/.local/e2e/.env"
if [[ "${USE_LOCAL_E2E_ENV:-1}" == "1" && -f "$DEFAULT_E2E_ENV_FILE" ]]; then
  set -a
  # shellcheck disable=SC1090
  source "$DEFAULT_E2E_ENV_FILE"
  set +a
fi

# Inputs (with defaults)
: "${TEST_SCHEMA:=ape_dts_manual}"
: "${TEST_TABLE:=gaussdb_to_pg_cdc_basic}"

: "${TEST_STICKY_RECONNECT:=0}"
: "${TEST_RESUME:=0}"
: "${TEST_FAILOVER:=0}"
: "${TEST_NEG_SLOT_ACTIVE:=0}"
: "${TEST_NEG_NO_REPL_USER:=0}"

: "${FAILOVER_TARGET_NODE:=2}"
: "${CM_SWITCHOVER_TIMEOUT_SECS:=600}"
: "${CM_SWITCHOVER_FAST:=0}"
: "${GAUSSDB_CM_REQUIRE_HEALTHY:=0}"

: "${SRC_GAUSS_URL:=}"
: "${SRC_GAUSS_PRIMARY_HOSTPORT:=10.250.0.51:8000}"
: "${SRC_GAUSS_DB:=postgres}"

: "${GAUSSDB_CM_SSH_HOST:=10.250.0.30}"
: "${GAUSSDB_CM_SSH_USER:=root}"
: "${GAUSSDB_CM_SSH_PASSWORD:=}"
: "${GAUSSDB_CM_RUBY_USER:=Ruby}"
: "${GAUSSDB_CM_ENV_FILE:=~/gauss_env_file}"
: "${GAUSSDB_CM_SSH_CONNECT_TIMEOUT_SECS:=30}"

: "${DST_PG_URL:=postgres://127.0.0.1:5434/postgres?options[statement_timeout]=10s}"
: "${DST_PG_USERNAME:=postgres}"
: "${DST_PG_PASSWORD:=postgres}"

: "${SRC_PGCONNECT_TIMEOUT:=10}"
: "${DST_PGCONNECT_TIMEOUT:=10}"

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
FAILOVER_PERFORMED=0
ORIG_PRIMARY_NODE=""
ORIG_PRIMARY_DN_INSTANCE=""
INITIAL_UNHEALTHY_NODES=""
TMP_NO_REPL_USER=""
TMP_NO_REPL_PASSWORD=""
RESTORE_OK=1

# For psql, strip query string by default (libpq may not understand options[...]=...).
DST_PG_PSQL_URL="${DST_PG_PSQL_URL:-${DST_PG_URL%%\?*}}"
SRC_GAUSS_PSQL_URL="${SRC_GAUSS_PSQL_URL:-postgres://${SRC_GAUSS_PRIMARY_HOSTPORT}/${SRC_GAUSS_DB}}"

# Credentials: prefer explicit env, then fall back to .local/manual/gaussdb_to_pg_cdc.ini (local-only).
if [[ ( -z "${SRC_GAUSS_USERNAME:-}" || -z "${SRC_GAUSS_PASSWORD:-}" ) && -n "${SRC_GAUSS_URL:-}" ]]; then
  require_cmd python3
  parsed="$(parse_pg_url_parts "$SRC_GAUSS_URL")"
  IFS=$'\t' read -r parsed_user parsed_pwd parsed_host parsed_port parsed_db <<<"$parsed"

  SRC_GAUSS_USERNAME="${SRC_GAUSS_USERNAME:-$parsed_user}"
  SRC_GAUSS_PASSWORD="${SRC_GAUSS_PASSWORD:-$parsed_pwd}"
  parsed_hostport="${parsed_host}:${parsed_port}"
  # If the caller didn't override the hostport/db explicitly, prefer parsing them from SRC_GAUSS_URL.
  if [[ -z "${SRC_GAUSS_PRIMARY_HOSTPORT:-}" || "${SRC_GAUSS_PRIMARY_HOSTPORT}" == "10.250.0.51:8000" ]]; then
    SRC_GAUSS_PRIMARY_HOSTPORT="$parsed_hostport"
  fi
  if [[ -z "${SRC_GAUSS_DB:-}" || "${SRC_GAUSS_DB}" == "postgres" ]]; then
    SRC_GAUSS_DB="$parsed_db"
  fi
fi
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
  PGPASSWORD="$SRC_GAUSS_PASSWORD" PGCONNECT_TIMEOUT="$SRC_PGCONNECT_TIMEOUT" psql "$SRC_GAUSS_PSQL_URL" -X -v ON_ERROR_STOP=1 -U "$SRC_GAUSS_USERNAME" "$@"
}

psql_dst() {
  PGPASSWORD="$DST_PG_PASSWORD" PGCONNECT_TIMEOUT="$DST_PGCONNECT_TIMEOUT" psql "$DST_PG_PSQL_URL" -X -v ON_ERROR_STOP=1 -U "$DST_PG_USERNAME" "$@"
}

refresh_src_psql_url() {
  SRC_GAUSS_PSQL_URL="postgres://${SRC_GAUSS_PRIMARY_HOSTPORT}/${SRC_GAUSS_DB}"
}

psql_src_hostport() {
  local hostport="$1"
  shift
  local url="postgres://${hostport}/${SRC_GAUSS_DB}"
  PGPASSWORD="$SRC_GAUSS_PASSWORD" PGCONNECT_TIMEOUT="$SRC_PGCONNECT_TIMEOUT" psql "$url" -X -v ON_ERROR_STOP=1 -U "$SRC_GAUSS_USERNAME" "$@"
}

psql_src_hostport_quick() {
  local hostport="$1"
  shift
  local url="postgres://${hostport}/${SRC_GAUSS_DB}"
  # For endpoint detection, prefer a short connect timeout so a single down node
  # won't stall the whole failover/resume workflow.
  PGPASSWORD="$SRC_GAUSS_PASSWORD" PGCONNECT_TIMEOUT=3 psql "$url" -X -v ON_ERROR_STOP=1 -U "$SRC_GAUSS_USERNAME" "$@"
}

candidate_src_hostports() {
  local default_port
  default_port="$(echo -n "$SRC_GAUSS_PRIMARY_HOSTPORT" | awk -F: '{print $NF}')"
  if [[ -n "${gaussdb_pg_candidate_hosts:-}" ]]; then
    echo "$gaussdb_pg_candidate_hosts" | tr ',' '\n' | sed -E 's/^[[:space:]]+//; s/[[:space:]]+$//' | awk -v p="$default_port" '
      NF==0 {next}
      $0 ~ /:[0-9]+$/ {print $0; next}
      {print $0 ":" p}
    '
  fi
  echo "$SRC_GAUSS_PRIMARY_HOSTPORT"
}

detect_rw_primary_hostport() {
  local hp
  while IFS= read -r hp; do
    hp="$(echo -n "$hp" | sed -E 's/^[[:space:]]+//; s/[[:space:]]+$//')"
    [[ -z "$hp" ]] && continue
    local out=""
    out="$(psql_src_hostport_quick "$hp" -tA -c "SELECT pg_is_in_recovery();" 2>/dev/null | tr -d '[:space:]' || true)"
    if [[ "$out" == "f" || "$out" == "false" || "$out" == "0" ]]; then
      echo -n "$hp"
      return 0
    fi
  done < <(candidate_src_hostports | awk '!seen[$0]++')
  return 1
}

ensure_src_primary_for_psql() {
  # Default timeout should tolerate one or more down candidates without
  # making callers pass a value everywhere.
  local timeout_secs="${1:-60}"
  local detected=""
  local deadline=$((SECONDS + timeout_secs))
  while (( SECONDS <= deadline )); do
    if detected="$(detect_rw_primary_hostport)"; then
      if [[ "$detected" != "$SRC_GAUSS_PRIMARY_HOSTPORT" ]]; then
        log "detected gaussdb RW primary endpoint: ${detected} (was ${SRC_GAUSS_PRIMARY_HOSTPORT})"
        SRC_GAUSS_PRIMARY_HOSTPORT="$detected"
        refresh_src_psql_url
      fi
      return 0
    fi
    sleep 1
  done

  log "failed to detect gaussdb RW primary within ${timeout_secs}s; diagnostics (pg_is_in_recovery):"
  local hp
  while IFS= read -r hp; do
    hp="$(echo -n "$hp" | sed -E 's/^[[:space:]]+//; s/[[:space:]]+$//')"
    [[ -z "$hp" ]] && continue
    local diag=""
    diag="$(psql_src_hostport_quick "$hp" -tA -c "SELECT pg_is_in_recovery();" 2>&1 || true)"
    diag="$(echo "$diag" | tr -d '\r' | head -n 1 | sed -E 's/[[:space:]]+$//')"
    if [[ -z "$diag" ]]; then
      diag="(no output)"
    fi
    log "  ${hp} -> ${diag}"
  done < <(candidate_src_hostports | awk '!seen[$0]++')

  die "cannot find RW primary among gaussdb_pg_candidate_hosts/base (SELECT pg_is_in_recovery() never returned false)"
}

try_ensure_src_primary_for_psql() {
  local detected=""
  if detected="$(detect_rw_primary_hostport)"; then
    SRC_GAUSS_PRIMARY_HOSTPORT="$detected"
    refresh_src_psql_url
    return 0
  fi
  return 1
}

require_failover_deps() {
  require_cmd sshpass
  require_cmd ssh
  if [[ -z "$GAUSSDB_CM_SSH_PASSWORD" ]]; then
    die "TEST_FAILOVER=1 requires GAUSSDB_CM_SSH_PASSWORD to be set (env only, do not commit)"
  fi
}

cm_ssh() {
  local host="$1"
  local cmd="$2"
  SSHPASS="$GAUSSDB_CM_SSH_PASSWORD" sshpass -e ssh \
    -o PreferredAuthentications=password \
    -o PubkeyAuthentication=no \
    -o StrictHostKeyChecking=no \
    -o UserKnownHostsFile=/dev/null \
    -o ConnectTimeout="${GAUSSDB_CM_SSH_CONNECT_TIMEOUT_SECS}" \
    "${GAUSSDB_CM_SSH_USER}@${host}" \
    "$cmd"
}

cm_run_as_ruby() {
  local host="$1"
  local inner="$2"
  cm_ssh "$host" "su - ${GAUSSDB_CM_RUBY_USER} -c \"bash -lc 'source ${GAUSSDB_CM_ENV_FILE} && ${inner}'\""
}

cm_query_cv() {
  local host="$1"
  cm_run_as_ruby "$host" "cm_ctl query -Cv"
}

cm_datanode_rows() {
  local host="$1"
  # output columns: "node ip instance role ha_status"
  cm_query_cv "$host" \
    | awk 'BEGIN{in_dn=0} /^\[  Datanode State/{in_dn=1; next} in_dn && /^\[/{exit} in_dn {print}' \
    | sed 's/|/\n/g' \
    | awk '$1 ~ /^[0-9]+$/ && $3 ~ /^[0-9]+$/ {print $1, $2, $3, $5, $6}'
}

cm_unhealthy_nodes() {
  local host="$1"
  cm_datanode_rows "$host" 2>/dev/null \
    | awk '$4=="Down" || $5!="Normal" {print $1}' \
    | sort -n | uniq | tr '\n' ' ' | sed -E 's/[[:space:]]+$//'
}

cm_capture_orig_primary() {
  local host="$1"
  local rows
  rows="$(cm_datanode_rows "$host")"
  ORIG_PRIMARY_NODE="$(echo "$rows" | awk '$4=="Primary"{print $1; exit}')"
  ORIG_PRIMARY_DN_INSTANCE="$(echo "$rows" | awk '$4=="Primary"{print $3; exit}')"
  if [[ -z "$ORIG_PRIMARY_NODE" || -z "$ORIG_PRIMARY_DN_INSTANCE" ]]; then
    die "failed to parse original primary from cm_ctl query -Cv output"
  fi
  log "cm primary before switchover: node=${ORIG_PRIMARY_NODE} dn_instance=${ORIG_PRIMARY_DN_INSTANCE}"
}

cm_dn_instance_for_node() {
  local host="$1"
  local node="$2"
  cm_datanode_rows "$host" | awk -v n="$node" '$1==n{print $3; exit}'
}

cm_switchover_to_node() {
  local host="$1"
  local node="$2"
  local dn_instance="$3"
  local fast_flag=""
  if [[ "${CM_SWITCHOVER_FAST}" == "1" ]]; then
    fast_flag="-f"
  fi
  cm_run_as_ruby "$host" "cm_ctl switchover -n ${node} -D/data/cluster/var/lib/engine/data1/data/dn_${dn_instance} ${fast_flag} -t ${CM_SWITCHOVER_TIMEOUT_SECS}"
}

cm_wait_primary_node() {
  local host="$1"
  local expected_node="$2"
  local timeout_secs="${3:-180}"
  for _ in $(seq 1 "$timeout_secs"); do
    local primary_node
    primary_node="$(cm_datanode_rows "$host" | awk '$4=="Primary"{print $1; exit}' || true)"
    if [[ "$primary_node" == "$expected_node" ]]; then
      return 0
    fi
    sleep 1
  done
  return 1
}

cm_query_dn_state_snippet() {
  local host="$1"
  # Avoid nested-quote pitfalls on the remote shell by filtering locally.
  cm_query_cv "$host" | grep -A5 "Datanode State" || true
}

cm_ssh_hosts() {
  # Prefer explicit hosts if caller provides them; otherwise derive from candidate list.
  if [[ -n "${GAUSSDB_CM_SSH_HOSTS:-}" ]]; then
    echo "$GAUSSDB_CM_SSH_HOSTS" | tr ',' '\n' | sed -E 's/^[[:space:]]+//; s/[[:space:]]+$//'
    return 0
  fi
  candidate_src_hostports | awk -F: '{print $1}' | awk '!seen[$0]++'
}

cm_wait_primary_node_any() {
  local expected_node="$1"
  local timeout_secs="${2:-240}"
  for _ in $(seq 1 "$timeout_secs"); do
    local host
    while IFS= read -r host; do
      host="$(echo -n "$host" | sed -E 's/^[[:space:]]+//; s/[[:space:]]+$//')"
      [[ -z "$host" ]] && continue
      local primary_node=""
      primary_node="$(cm_datanode_rows "$host" 2>/dev/null | awk '$4=="Primary"{print $1; exit}' || true)"
      if [[ "$primary_node" == "$expected_node" ]]; then
        return 0
      fi
    done < <(cm_ssh_hosts)
    sleep 1
  done
  return 1
}

cm_pick_reachable_host() {
  # Prefer GAUSSDB_CM_SSH_HOST if it works, otherwise scan all known CM SSH hosts
  # (derived from candidate list or GAUSSDB_CM_SSH_HOSTS).
  local host=""
  if [[ -n "${GAUSSDB_CM_SSH_HOST:-}" ]]; then
    if cm_query_cv "${GAUSSDB_CM_SSH_HOST}" >/dev/null 2>&1; then
      echo "${GAUSSDB_CM_SSH_HOST}"
      return 0
    fi
  fi
  while IFS= read -r host; do
    host="$(echo -n "$host" | sed -E 's/^[[:space:]]+//; s/[[:space:]]+$//')"
    [[ -z "$host" ]] && continue
    if cm_query_cv "$host" >/dev/null 2>&1; then
      echo "$host"
      return 0
    fi
  done < <(cm_ssh_hosts)
  return 1
}

perform_failover() {
  require_failover_deps
  # switchover must be initiated on the CURRENT primary DN host.
  ensure_src_primary_for_psql >/dev/null
  local switchover_host="${SRC_GAUSS_PRIMARY_HOSTPORT%:*}"

  log "cm switchover host (current primary DN): ${switchover_host}"
  cm_capture_orig_primary "$switchover_host"
  log "cm datanode state (before switchover):"
  cm_query_dn_state_snippet "$switchover_host" || true

  # Safety guard: optionally require a fully healthy cluster.
  # By default we allow running failover as long as there is a healthy standby to promote.
  INITIAL_UNHEALTHY_NODES="$(cm_unhealthy_nodes "$switchover_host" || true)"
  if [[ -n "$INITIAL_UNHEALTHY_NODES" ]]; then
    log "WARN: cm cluster is degraded before failover (unhealthy_nodes=${INITIAL_UNHEALTHY_NODES})"
  fi
  if [[ "${GAUSSDB_CM_REQUIRE_HEALTHY}" == "1" && -n "$INITIAL_UNHEALTHY_NODES" ]]; then
    die "cm cluster is not healthy (GAUSSDB_CM_REQUIRE_HEALTHY=1, unhealthy_nodes=${INITIAL_UNHEALTHY_NODES}). Refuse to run failover."
  fi

  local target_node="$FAILOVER_TARGET_NODE"
  if [[ -n "$ORIG_PRIMARY_NODE" && "$target_node" == "$ORIG_PRIMARY_NODE" ]]; then
    # If the requested target is already primary, pick another node to ensure we actually
    # exercise failover recovery in this run.
    for n in 1 2 3; do
      if [[ "$n" != "$ORIG_PRIMARY_NODE" ]]; then
        target_node="$n"
        break
      fi
    done
    log "FAILOVER_TARGET_NODE is already primary (node=${ORIG_PRIMARY_NODE}); switch target_node to ${target_node} for this run"
  fi
  local target_instance
  target_instance="$(cm_dn_instance_for_node "$switchover_host" "$target_node")"
  if [[ -z "$target_instance" ]]; then
    die "failed to resolve dn instance for target node=${target_node} from cm_ctl query -Cv"
  fi

  local target_role target_status
  read -r target_role target_status < <(cm_datanode_rows "$switchover_host" | awk -v n="$target_node" '$1==n{print $4, $5; exit}')
  if [[ "$target_role" != "Standby" || "$target_status" != "Normal" ]]; then
    die "target node is not a healthy standby: node=${target_node} role=${target_role} status=${target_status}"
  fi

  log "cm switchover: node=${target_node} dn_instance=${target_instance}"
  local sw_out=""
  local sw_rc=0
  set +e
  sw_out="$(cm_switchover_to_node "$switchover_host" "$target_node" "$target_instance" 2>&1)"
  sw_rc=$?
  set -e
  if [[ "$sw_rc" -ne 0 ]]; then
    log "WARN: cm switchover command returned rc=${sw_rc}, will still verify via query -Cv"
    echo "$sw_out" | tail -n 30 | sed -E 's/^/[cm_switchover] /' || true
    # If CM reports it's busy with another command, do not hang waiting for convergence.
    if echo "$sw_out" | grep -Eqi "another command\\([0-9]+\\) is running"; then
      die "cm_ctl is busy (another command is running); please wait for it to finish and retry TEST_FAILOVER=1"
    fi
    # Retry with fast switchover when the normal mode times out (common on degraded/shared HA envs).
    if [[ "${CM_SWITCHOVER_FAST}" != "1" ]] && echo "$sw_out" | grep -Eqi "switchover command timeout|command timeout"; then
      log "retrying cm switchover with fast mode (-f) due to timeout..."
      local saved_fast="${CM_SWITCHOVER_FAST}"
      CM_SWITCHOVER_FAST=1
      set +e
      sw_out="$(cm_switchover_to_node "$switchover_host" "$target_node" "$target_instance" 2>&1)"
      sw_rc=$?
      set -e
      CM_SWITCHOVER_FAST="${saved_fast}"
      if [[ "$sw_rc" -ne 0 ]]; then
        log "WARN: fast cm switchover still failed (rc=${sw_rc}), will continue to verify via query -Cv"
        echo "$sw_out" | tail -n 30 | sed -E 's/^/[cm_switchover_fast] /' || true
      fi
    fi
  fi

  log "waiting for cm primary to become node=${target_node} ..."
  # Prefer querying on the switchover host; only fall back to probing other hosts when needed.
  if ! cm_wait_primary_node "$switchover_host" "$target_node" 240; then
    log "WARN: cm_wait_primary_node failed on ${switchover_host}, fallback to probing any cm hosts..."
    if ! cm_wait_primary_node_any "$target_node" 240; then
      die "cm switchover did not converge to node=${target_node} within timeout"
    fi
  fi
  log "cm datanode state (after switchover):"
  # After switchover, the old primary becomes standby; query may still work there. Best-effort.
  cm_query_dn_state_snippet "$switchover_host" || true

  FAILOVER_PERFORMED=1
  log "cm switchover done"
}

maybe_restore_primary() {
  if [[ "$FAILOVER_PERFORMED" != "1" ]]; then
    return 0
  fi
  if [[ -z "$ORIG_PRIMARY_NODE" || -z "$ORIG_PRIMARY_DN_INSTANCE" ]]; then
    return 0
  fi

  # Restore must ideally be initiated on the current primary DN host, but in some
  # environments SSH may not be reachable on that host. Fall back to any reachable
  # CM host (GAUSSDB_CM_SSH_HOST / host scan) in a best-effort way.
  local restore_host=""
  if try_ensure_src_primary_for_psql; then
    restore_host="${SRC_GAUSS_PRIMARY_HOSTPORT%:*}"
    if ! cm_query_cv "$restore_host" >/dev/null 2>&1; then
      if restore_host="$(cm_pick_reachable_host)"; then
        log "WARN: current primary host is not reachable via SSH; restore will run on ${restore_host}"
      fi
    fi
  else
    restore_host="$(cm_pick_reachable_host || true)"
  fi
  if [[ -z "$restore_host" ]]; then
    log "WARN: no reachable CM host found for restore (best-effort); please restore manually if needed"
    RESTORE_OK=0
    return 0
  fi

  log "best-effort restore cm primary: node=${ORIG_PRIMARY_NODE} dn_instance=${ORIG_PRIMARY_DN_INSTANCE}"
  local restore_out=""
  local restore_rc=0
  set +e
  restore_out="$(cm_switchover_to_node "$restore_host" "$ORIG_PRIMARY_NODE" "$ORIG_PRIMARY_DN_INSTANCE" 2>&1)"
  restore_rc=$?
  set -e
  if [[ "$restore_rc" -eq 0 ]]; then
    if ! cm_wait_primary_node "$restore_host" "$ORIG_PRIMARY_NODE" 240 >/dev/null 2>&1; then
      if ! cm_wait_primary_node_any "$ORIG_PRIMARY_NODE" 240 >/dev/null 2>&1; then
        log "ERROR: cm restore did not converge to original primary (node=${ORIG_PRIMARY_NODE}) within timeout"
        RESTORE_OK=0
      fi
    fi
    log "cm restore attempted"
  else
    log "WARN: cm restore failed (best-effort)"
    echo "$restore_out" | tail -n 30 | sed -E 's/^/[cm_restore] /' || true
    RESTORE_OK=0
  fi
}

cleanup_tmp_no_repl_user() {
  if [[ -n "$TMP_NO_REPL_USER" ]]; then
    log "cleanup temp role: ${TMP_NO_REPL_USER}"
    ensure_src_primary_for_psql >/dev/null 2>&1 || true
    psql_src -c "DROP ROLE IF EXISTS ${TMP_NO_REPL_USER};" >/dev/null 2>&1 || true
  fi
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
  cleanup_tmp_no_repl_user
  maybe_restore_primary
  # After potential failover/restore, refresh the psql endpoint to the current RW primary
  # so cleanup can drop slot/schema on the right node.
  try_ensure_src_primary_for_psql >/dev/null 2>&1 || true
  cleanup_src
  cleanup_dst
  cleanup_pg_container

  if [[ "$FAILOVER_PERFORMED" == "1" && -n "$ORIG_PRIMARY_NODE" ]]; then
    # Best-effort final verification: primary restored + no new unhealthy nodes introduced.
    local verify_host=""
    if try_ensure_src_primary_for_psql; then
      verify_host="${SRC_GAUSS_PRIMARY_HOSTPORT%:*}"
    else
      verify_host="${GAUSSDB_CM_SSH_HOST}"
    fi
    local final_primary=""
    final_primary="$(cm_datanode_rows "$verify_host" 2>/dev/null | awk '$4=="Primary"{print $1; exit}' || true)"
    if [[ -z "$final_primary" ]]; then
      log "ERROR: failed to verify cm primary node after restore (verify_host=${verify_host})"
      RESTORE_OK=0
    elif [[ "$final_primary" != "$ORIG_PRIMARY_NODE" ]]; then
      log "ERROR: cm primary node not restored (orig=${ORIG_PRIMARY_NODE}, final=${final_primary})"
      RESTORE_OK=0
    fi

    local final_unhealthy=""
    final_unhealthy="$(cm_unhealthy_nodes "$verify_host" || true)"
    if [[ "${GAUSSDB_CM_REQUIRE_HEALTHY}" == "1" ]]; then
      if [[ -n "$final_unhealthy" ]]; then
        log "ERROR: cm cluster is unhealthy after test while GAUSSDB_CM_REQUIRE_HEALTHY=1 (unhealthy_nodes=${final_unhealthy})"
        RESTORE_OK=0
      fi
    else
      if [[ -n "$final_unhealthy" && -n "$INITIAL_UNHEALTHY_NODES" ]]; then
        for n in $final_unhealthy; do
          if [[ " $INITIAL_UNHEALTHY_NODES " != *" $n "* ]]; then
            log "ERROR: cm unhealthy nodes became worse after test (new_unhealthy_node=${n}, initial=${INITIAL_UNHEALTHY_NODES}, final=${final_unhealthy})"
            RESTORE_OK=0
          fi
        done
      elif [[ -n "$final_unhealthy" && -z "$INITIAL_UNHEALTHY_NODES" ]]; then
        log "ERROR: cm cluster became unhealthy after test (final_unhealthy_nodes=${final_unhealthy})"
        RESTORE_OK=0
      fi
    fi
  fi

  if [[ "$RESTORE_OK" != "1" ]]; then
    log "ERROR: failover cleanup/restore verification failed; please restore/repair the cluster manually"
    exit 1
  fi
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

  if [[ "$TEST_RESUME" == "1" ]]; then
    cat >>"$config_path" <<EOF

[resumer]
resume_type=from_log
log_dir=${LOG_DIR}
config_file=${LOG_DIR}/position.log
EOF
  fi
}

write_dt_main_config_with_log_dir() {
  local config_path="$1"
  local log_dir="$2"

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
log_dir=${log_dir}
EOF
}

write_precheck_config() {
  local config_path="$1"
  local src_username="$2"
  local src_password="$3"

  local extractor_url="postgres://${SRC_GAUSS_PRIMARY_HOSTPORT}/${SRC_GAUSS_DB}?options[statement_timeout]=10s"

  cat >"$config_path" <<EOF
[precheck]
do_struct_init=false
do_cdc=true

[extractor]
db_type=gaussdb_pg
extract_type=cdc
url=${extractor_url}
username=${src_username}
password=${src_password}
slot_name=${SLOT_NAME}
start_lsn=
recreate_slot_if_exists=false
keepalive_interval_secs=10
heartbeat_interval_secs=0
heartbeat_tb=

[filter]
do_dbs=${TEST_SCHEMA}
ignore_dbs=
do_tbs=
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

dt_main_bin() {
  echo -n "$REPO_ROOT/target/debug/dt-main"
}

ensure_dt_main_binary() {
  local bin
  bin="$(dt_main_bin)"
  if [[ -x "$bin" ]]; then
    return 0
  fi
  require_cmd cargo
  log "building dt-main binary..."
  (cd "$REPO_ROOT" && cargo build -p dt-main) >/dev/null
}

start_dt_main() {
  local config_path="$1"
  ensure_dt_main_binary
  local bin
  bin="$(dt_main_bin)"

  log "starting dt-main (config=${config_path})"
  log "dt-main logs: ${LOG_DIR}"
  "$bin" "$config_path" >"$DTMAIN_STDOUT_LOG" 2>"$DTMAIN_STDERR_LOG" &
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

wait_for_slot_inactive() {
  local timeout_secs="${1:-90}"
  log "waiting for replication slot to become inactive (slot=${SLOT_NAME})"
  for _ in $(seq 1 "$timeout_secs"); do
    local active=""
    active="$(psql_src -tA -c "SELECT active FROM pg_replication_slots WHERE slot_name='${SLOT_NAME}' LIMIT 1;" 2>/dev/null | tr -d '[:space:]' || true)"
    # If slot no longer exists, treat it as inactive for resume purposes.
    if [[ -z "$active" || "$active" == "f" || "$active" == "false" || "$active" == "0" ]]; then
      return 0
    fi
    sleep 1
  done
  die "replication slot still active after timeout (slot=${SLOT_NAME})"
}

wait_for_streaming_started() {
  local default_log="$LOG_DIR/default.log"
  log "waiting for replication streaming to start..."
  if wait_for_log "$default_log" "gaussdb cdc replication streaming started:" 180; then
    return 0
  fi

  log "replication streaming did not start within timeout"
  log "dt-main stdout (tail):"
  tail -n 120 "$DTMAIN_STDOUT_LOG" || true
  log "dt-main stderr (tail):"
  tail -n 120 "$DTMAIN_STDERR_LOG" || true
  if [[ -f "$default_log" ]]; then
    log "default.log (tail):"
    tail -n 120 "$default_log" || true
  fi
  die "replication streaming not started"
}

extract_checkpoint_lsn_from_position_log() {
  local file="$1"
  if [[ ! -f "$file" ]]; then
    return 1
  fi
  grep -F "checkpoint_position" "$file" 2>/dev/null \
    | grep -F "\"type\":\"PgCdc\"" 2>/dev/null \
    | tail -n 1 \
    | sed -n 's/.*"lsn":"\([^"]*\)".*/\1/p'
}

wait_for_checkpoint_lsn() {
  local timeout_secs="${1:-90}"
  local pos_log="$LOG_DIR/position.log"
  log "waiting for checkpoint_position PgCdc lsn in position.log..." >&2
  for _ in $(seq 1 "$timeout_secs"); do
    local lsn
    lsn="$(extract_checkpoint_lsn_from_position_log "$pos_log" || true)"
    if [[ -n "$lsn" ]]; then
      echo -n "$lsn"
      return 0
    fi
    sleep 1
  done
  die "checkpoint lsn not found within timeout (position.log=${pos_log})"
}

wait_for_recovery_log() {
  local expected_lsn="$1"
  local timeout_secs="${2:-90}"
  local default_log="$LOG_DIR/default.log"
  local needle="cdc recovery from lsn:[${expected_lsn}]"
  log "waiting for recovery evidence in default.log: ${needle}"
  if wait_for_log "$default_log" "$needle" "$timeout_secs"; then
    return 0
  fi
  die "recovery evidence not found within timeout (default.log=${default_log})"
}

wait_for_streaming_started_on() {
  local host="$1"
  local ha_port="$2"
  local timeout_secs="${3:-180}"
  local default_log="$LOG_DIR/default.log"
  local needle="gaussdb cdc replication streaming started: ${host}:${ha_port}"
  log "waiting for streaming started on new primary: ${needle}"
  if wait_for_log "$default_log" "$needle" "$timeout_secs"; then
    return 0
  fi
  die "failover recovery evidence not found within timeout (expected='${needle}', default.log=${default_log})"
}

run_source_dml_phase1() {
  log "run source DML phase1 (insert/update/delete)"
  psql_src -c "INSERT INTO ${TEST_SCHEMA}.${TEST_TABLE} (id, val) VALUES (1, 'a'), (2, 'b');"
  psql_src -c "UPDATE ${TEST_SCHEMA}.${TEST_TABLE} SET val = 'c' WHERE id = 2;"
  psql_src -c "DELETE FROM ${TEST_SCHEMA}.${TEST_TABLE} WHERE id = 1;"
}

run_source_dml_phase2() {
  log "run source DML phase2 (verify resume/failover continues)"
  psql_src -c "UPDATE ${TEST_SCHEMA}.${TEST_TABLE} SET val = 'e' WHERE id = 2;"
  psql_src -c "INSERT INTO ${TEST_SCHEMA}.${TEST_TABLE} (id, val) VALUES (3, 'd');"
  psql_src -c "DELETE FROM ${TEST_SCHEMA}.${TEST_TABLE} WHERE id = 3;"
}

assert_destination_rows() {
  local expected="$1"
  log "assert destination state (expect rows: ${expected})"
  for _ in $(seq 1 60); do
    local out
    out="$(psql_dst -tA -F '|' -c "SELECT id, val FROM ${TEST_SCHEMA}.${TEST_TABLE} ORDER BY id;" 2>/dev/null || true)"
    if [[ "$out" == "$expected" ]]; then
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

assert_e2e_logs() {
  local default_log="$LOG_DIR/default.log"
  if [[ ! -f "$default_log" ]]; then
    log "WARN: default.log not found under ${LOG_DIR}, skip log assertions"
    return 0
  fi

  log "assert dt-main logs (candidate-first + HA port + NoTLS)"
  if [[ -n "${gaussdb_pg_candidate_hosts:-}" ]]; then
    grep -Fq "gaussdb cdc endpoint selection: prefer_candidates=true" "$default_log" \
      || die "missing evidence: prefer_candidates=true (candidate-first selection)"
  fi
  grep -Fq "gaussdb replication connection starts (ssl=off" "$default_log" \
    || die "missing evidence: replication NoTLS (ssl=off)"

  # Heuristic: HA port is usually sql_port+1 (e.g. 8001). This is the key behavior.
  grep -Fq "port=8001" "$default_log" \
    || log "WARN: did not find port=8001 in default.log (verify HA port manually if sql_port is not 8000)"
}

assert_sticky_reconnect_logs() {
  local default_log="$LOG_DIR/default.log"
  if [[ ! -f "$default_log" ]]; then
    log "WARN: default.log not found under ${LOG_DIR}, skip sticky reconnect log assertions"
    return 0
  fi

  log "waiting for reconnect evidence (sticky last_success + probe order)"
  for _ in $(seq 1 60); do
    local last_success
    last_success="$(grep -F "gaussdb cdc endpoint selection:" "$default_log" | tail -n 1 | sed -E 's/.*last_success=([^ ]+).*/\1/' || true)"
    if [[ -n "$last_success" && "$last_success" != "none" ]]; then
      grep -Fq "gaussdb cdc probe order: ${last_success}," "$default_log" \
        || die "sticky reconnect: probe order does not start with last_success=${last_success}"
      return 0
    fi
    sleep 1
  done
  die "sticky reconnect: expected last_success to be set after reconnect, but it stayed 'none'"
}

start_dt_main_background() {
  local config_path="$1"
  local stdout_log="$2"
  local stderr_log="$3"

  ensure_dt_main_binary
  local bin
  bin="$(dt_main_bin)"
  "$bin" "$config_path" >"$stdout_log" 2>"$stderr_log" &
  echo -n "$!"
}

wait_for_slot_active_error() {
  local file="$1"
  local timeout_secs="${2:-90}"
  for _ in $(seq 1 "$timeout_secs"); do
    if [[ -f "$file" ]] && grep -Eqi "slot.*active|replication slot.*active" "$file" 2>/dev/null; then
      return 0
    fi
    sleep 1
  done
  return 1
}

wait_for_slot_active_error_any() {
  local file1="$1"
  local file2="$2"
  local timeout_secs="${3:-90}"
  for _ in $(seq 1 "$timeout_secs"); do
    for f in "$file1" "$file2"; do
      if [[ -f "$f" ]] && grep -Eqi "slot.*active|replication slot.*active" "$f" 2>/dev/null; then
        return 0
      fi
    done
    sleep 1
  done
  return 1
}

run_precheck_expect_fail_contains() {
  local cfg="$1"
  local pattern="$2"
  local out_file="$3"

  ensure_dt_main_binary
  set +e
  "$(dt_main_bin)" "$cfg" >"$out_file" 2>&1
  local code=$?
  set -e

  if [[ "$code" -eq 0 ]]; then
    die "precheck unexpectedly passed (cfg=${cfg})"
  fi
  if grep -Eqi "$pattern" "$out_file" 2>/dev/null; then
    return 0
  fi
  log "precheck output (tail):"
  tail -n 200 "$out_file" || true
  die "precheck failed but missing expected evidence: /${pattern}/ (cfg=${cfg})"
}

run_negative_slot_active() {
  log "negative test: slot active (start 2nd dt-main with same slot)"

  local run2_dir="$RUN_DIR/neg_slot2"
  local log2_dir="$run2_dir/logs"
  mkdir -p "$log2_dir"

  # Also validate precheck fail-fast path while slot is active.
  local precheck_cfg="$run2_dir/precheck_slot_active.ini"
  local precheck_log="$run2_dir/precheck_slot_active.log"
  write_precheck_config "$precheck_cfg" "$SRC_GAUSS_USERNAME" "$SRC_GAUSS_PASSWORD"
  log "running precheck against active slot (expect FAIL) ..."
  run_precheck_expect_fail_contains "$precheck_cfg" "replication slot.*active" "$precheck_log"

  local cfg2="$run2_dir/task_config.ini"
  local out2="$run2_dir/dt-main2.stdout.log"
  local err2="$run2_dir/dt-main2.stderr.log"
  write_dt_main_config_with_log_dir "$cfg2" "$log2_dir"

  local pid2
  pid2="$(start_dt_main_background "$cfg2" "$out2" "$err2")"
  log "2nd dt-main started (pid=$pid2), waiting for slot-active failure evidence..."

  local default2="$log2_dir/default.log"
  if wait_for_slot_active_error_any "$err2" "$default2" 120; then
    log "negative ok: found slot-active evidence in logs"
  else
    log "2nd dt-main stdout (tail):"
    tail -n 120 "$out2" || true
    log "2nd dt-main stderr (tail):"
    tail -n 120 "$err2" || true
    if [[ -f "$default2" ]]; then
      log "2nd default.log (tail):"
      tail -n 120 "$default2" || true
    fi
    kill "$pid2" >/dev/null 2>&1 || true
    die "negative slot-active test failed: did not find expected error evidence"
  fi

  kill "$pid2" >/dev/null 2>&1 || true
}

run_negative_no_repl_user() {
  log "negative test: no replication privilege user (precheck should fail fast)"
  ensure_src_primary_for_psql

  TMP_NO_REPL_USER="ape_dts_no_repl_${RUN_ID}"
  TMP_NO_REPL_PASSWORD="tmp_no_repl_${RUN_ID}"

  log "creating temp role (login only): ${TMP_NO_REPL_USER}"
  psql_src -c "DROP ROLE IF EXISTS ${TMP_NO_REPL_USER};" >/dev/null 2>&1 || true
  psql_src -c "CREATE ROLE ${TMP_NO_REPL_USER} LOGIN PASSWORD '${TMP_NO_REPL_PASSWORD}';" >/dev/null
  psql_src -c "GRANT CONNECT ON DATABASE ${SRC_GAUSS_DB} TO ${TMP_NO_REPL_USER};" >/dev/null 2>&1 || true

  local precheck_cfg="$RUN_DIR/precheck_no_repl.ini"
  write_precheck_config "$precheck_cfg" "$TMP_NO_REPL_USER" "$TMP_NO_REPL_PASSWORD"

  local precheck_log="$RUN_DIR/precheck_no_repl.log"
  log "running precheck (expect FAIL) ..."
  set +e
  ensure_dt_main_binary
  "$(dt_main_bin)" "$precheck_cfg" >"$precheck_log" 2>&1
  local code=$?
  set -e
  if [[ "$code" -eq 0 ]]; then
    die "negative no-repl-user test failed: precheck unexpectedly passed"
  fi

  if grep -Fq "insufficient permission for CDC" "$precheck_log" 2>/dev/null; then
    log "negative ok: precheck reported insufficient permission for CDC"
  else
    log "precheck output (tail):"
    tail -n 160 "$precheck_log" || true
    die "negative no-repl-user test failed: missing expected permission error message"
  fi

  # Cleanup early (trap will also best-effort cleanup).
  psql_src -c "DROP ROLE IF EXISTS ${TMP_NO_REPL_USER};" >/dev/null 2>&1 || true
  TMP_NO_REPL_USER=""
  TMP_NO_REPL_PASSWORD=""
}

main() {
  require_cmd psql

  refresh_src_psql_url
  ensure_src_primary_for_psql

  if [[ "$TEST_RESUME" == "1" && "$TEST_FAILOVER" == "1" ]]; then
    die "please run TEST_RESUME=1 and TEST_FAILOVER=1 in separate runs (avoid multi-phase ambiguity)"
  fi
  if [[ "$TEST_NEG_SLOT_ACTIVE" == "1" && "$TEST_NEG_NO_REPL_USER" == "1" ]]; then
    die "please run negative cases separately: TEST_NEG_SLOT_ACTIVE=1 or TEST_NEG_NO_REPL_USER=1"
  fi

  log "run_dir: ${RUN_DIR}"
  log "source: ${SRC_GAUSS_PRIMARY_HOSTPORT}/${SRC_GAUSS_DB} (user=${SRC_GAUSS_USERNAME})"
  log "destination: $(sanitize_url_for_log "$DST_PG_PSQL_URL") (user=${DST_PG_USERNAME})"
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

  if [[ "$TEST_NEG_NO_REPL_USER" == "1" ]]; then
    run_negative_no_repl_user
    log "negative succeeded"
    return 0
  fi

  start_dt_main "$config_path"
  wait_for_slot
  wait_for_streaming_started

  if [[ "$TEST_NEG_SLOT_ACTIVE" == "1" ]]; then
    run_negative_slot_active
    log "negative succeeded"
    return 0
  fi

  # Optional: terminate replication backend to ensure reconnect uses sticky endpoint selection.
  if [[ "$TEST_STICKY_RECONNECT" == "1" ]]; then
    log "sticky reconnect enabled: waiting for replication to start..."
    local pid=""
    if pid="$(wait_for_replication_backend_pid 60)"; then
      if terminate_replication_backend "$pid"; then
        # Wait for reconnect to happen and then verify logs reflect sticky endpoint.
        assert_sticky_reconnect_logs
      fi
    else
      log "WARN: could not find replication backend pid; sticky reconnect test will be skipped"
    fi
  fi

  run_source_dml_phase1
  assert_destination_rows "2|c"

  if [[ "$TEST_RESUME" == "1" ]]; then
    local checkpoint_lsn_before
    checkpoint_lsn_before="$(wait_for_checkpoint_lsn 180)"
    log "checkpoint lsn (before stop): ${checkpoint_lsn_before}"

    stop_dt_main
    # Ensure the previous replication connection is released; otherwise restart will hit
    # "replication slot is already active" and never progress.
    wait_for_slot_inactive 180

    local checkpoint_lsn
    checkpoint_lsn="$(extract_checkpoint_lsn_from_position_log "$LOG_DIR/position.log" || true)"
    if [[ -z "$checkpoint_lsn" ]]; then
      die "failed to re-read checkpoint lsn from position.log after stop"
    fi
    log "checkpoint lsn (after stop): ${checkpoint_lsn}"

    start_dt_main "$config_path"
    wait_for_recovery_log "$checkpoint_lsn" 180
    wait_for_streaming_started

    run_source_dml_phase2
    assert_destination_rows "2|e"
  fi

  if [[ "$TEST_FAILOVER" == "1" ]]; then
    local old_primary="$SRC_GAUSS_PRIMARY_HOSTPORT"
    log "failover enabled: current primary is ${old_primary}"

    perform_failover
    # After switchover, the promoted primary may take some time to become SQL-ready.
    ensure_src_primary_for_psql 240
    local new_primary="$SRC_GAUSS_PRIMARY_HOSTPORT"
    if [[ "$new_primary" == "$old_primary" ]]; then
      die "failover did not change RW primary endpoint (still ${new_primary})"
    fi
    log "failover complete: new primary is ${new_primary}"

    local new_host="${new_primary%:*}"
    local new_sql_port="${new_primary##*:}"
    local new_ha_port=$((new_sql_port + 1))
    wait_for_streaming_started_on "$new_host" "$new_ha_port" 240

    run_source_dml_phase2
    assert_destination_rows "2|e"
  fi

  assert_e2e_logs
  log "e2e succeeded"
}

main "$@"
