#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ENV_LOCAL="$ROOT/dt-tests/tests/.env.local"

DEFAULT_GAUSSDB_RW_HOST="10.250.0.157"
DEFAULT_GAUSSDB_PORT="8000"
DEFAULT_GAUSSDB_DB="db_ora_mode"
DEFAULT_GAUSSDB_USER="root"
DEFAULT_ORACLE_CONTAINER="oracle-xe-local"
DEFAULT_ORACLE_HOST="127.0.0.1"
DEFAULT_ORACLE_PORT="1521"
DEFAULT_ORACLE_SERVICE="XE"
DEFAULT_ORACLE_USER="APE_DTS"
DEFAULT_BULK_START_ID="1000"
DEFAULT_BULK_ROW_COUNT="3000"
DEFAULT_DELETE_START_ID="900000"
DEFAULT_DELETE_ROW_COUNT="200"
DEFAULT_VERIFY_TIMEOUT_SECS="900"
DEFAULT_VERIFY_POLL_SECS="10"

GAUSSDB_RW_HOST="${GAUSSDB_RW_HOST:-$DEFAULT_GAUSSDB_RW_HOST}"
GAUSSDB_PORT="${GAUSSDB_PORT:-$DEFAULT_GAUSSDB_PORT}"
GAUSSDB_DB="${GAUSSDB_DB:-$DEFAULT_GAUSSDB_DB}"
GAUSSDB_USER="${GAUSSDB_USER:-$DEFAULT_GAUSSDB_USER}"
ORACLE_CONTAINER="${ORACLE_SQLPLUS_DOCKER_CONTAINER:-$DEFAULT_ORACLE_CONTAINER}"
ORACLE_HOST="${ORACLE_HOST:-$DEFAULT_ORACLE_HOST}"
ORACLE_PORT="${ORACLE_PORT:-$DEFAULT_ORACLE_PORT}"
ORACLE_SERVICE="${ORACLE_SERVICE:-$DEFAULT_ORACLE_SERVICE}"
ORACLE_USER="${ORACLE_USER:-$DEFAULT_ORACLE_USER}"
BULK_START_ID="${BULK_START_ID:-$DEFAULT_BULK_START_ID}"
BULK_ROW_COUNT="${BULK_ROW_COUNT:-$DEFAULT_BULK_ROW_COUNT}"
DELETE_START_ID="${DELETE_START_ID:-$DEFAULT_DELETE_START_ID}"
DELETE_ROW_COUNT="${DELETE_ROW_COUNT:-$DEFAULT_DELETE_ROW_COUNT}"
VERIFY_TIMEOUT_SECS="${VERIFY_TIMEOUT_SECS:-$DEFAULT_VERIFY_TIMEOUT_SECS}"
VERIFY_POLL_SECS="${VERIFY_POLL_SECS:-$DEFAULT_VERIFY_POLL_SECS}"

log() {
  printf '[gaussdb-oracle-bulk-verify] %s\n' "$*"
}

die() {
  printf 'ERROR: %s\n' "$*" >&2
  exit 1
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing dependency: $1"
}

require_positive_integer() {
  local name="$1"
  local value="$2"
  [[ "$value" =~ ^[1-9][0-9]*$ ]] || die "$name must be a positive integer: $value"
}

env_file_value() {
  local key="$1"
  [[ -f "$ENV_LOCAL" ]] || return 0
  awk -F= -v key="$key" '$1 == key {print substr($0, length(key) + 2); exit}' "$ENV_LOCAL"
}

secret_value() {
  local env_name="$1"
  local file_key="$2"
  local value="${!env_name:-}"
  [[ -n "$value" ]] && {
    printf '%s' "$value"
    return
  }
  env_file_value "$file_key"
}

validate_config() {
  require_positive_integer BULK_START_ID "$BULK_START_ID"
  require_positive_integer BULK_ROW_COUNT "$BULK_ROW_COUNT"
  require_positive_integer DELETE_START_ID "$DELETE_START_ID"
  require_positive_integer DELETE_ROW_COUNT "$DELETE_ROW_COUNT"
  require_positive_integer VERIFY_TIMEOUT_SECS "$VERIFY_TIMEOUT_SECS"
  require_positive_integer VERIFY_POLL_SECS "$VERIFY_POLL_SECS"
  [[ -n "$(secret_value ORACLE_PASSWORD oracle_sinker_password)" ]] ||
    die "missing ORACLE_PASSWORD or oracle_sinker_password in $ENV_LOCAL"
}

gaussdb_conn_url() {
  printf 'postgres://%s@%s:%s/%s?sslmode=require&protocolVersion=351' \
    "$GAUSSDB_USER" "$GAUSSDB_RW_HOST" "$GAUSSDB_PORT" "$GAUSSDB_DB"
}

bulk_end_id() {
  printf '%s' "$((BULK_START_ID + BULK_ROW_COUNT - 1))"
}

delete_end_id() {
  printf '%s' "$((DELETE_START_ID + DELETE_ROW_COUNT - 1))"
}

expected_total() {
  printf '%s' "$((BULK_ROW_COUNT + 1))"
}

expected_sample() {
  local mid_id="$((BULK_START_ID + BULK_ROW_COUNT / 2))"
  local mid_offset="$((mid_id - BULK_START_ID + 1))"
  printf '1|bulk_update|bulk_after_update\n'
  printf '%s|bulk_insert_%06d|payload_%06d\n' \
    "$BULK_START_ID" 1 1
  printf '%s|bulk_insert_%06d|payload_%06d\n' \
    "$((BULK_START_ID + 1))" 2 2
  printf '%s|bulk_insert_%06d|payload_%06d\n' \
    "$mid_id" "$mid_offset" "$mid_offset"
  printf '%s|bulk_insert_%06d|payload_%06d\n' \
    "$(bulk_end_id)" "$BULK_ROW_COUNT" "$BULK_ROW_COUNT"
}

pg_sql_summary() {
  local table="$1"
  cat <<SQL
SELECT 'summary|' || COUNT(*)::text || '|' || COALESCE(MIN(id), 0)::text || '|' || COALESCE(MAX(id), 0)::text
FROM $table;
SELECT 'delete_probe|' || COUNT(*)
FROM $table
WHERE id BETWEEN $DELETE_START_ID AND $(delete_end_id);
SELECT 'sample|' || id::text || '|' || tracer || '|' || payload
FROM $table
WHERE id IN (1, $BULK_START_ID, $((BULK_START_ID + 1)), $((BULK_START_ID + BULK_ROW_COUNT / 2)), $(bulk_end_id))
ORDER BY id;
SQL
}

oracle_sql_summary() {
  local table="$1"
  cat <<SQL
SELECT 'summary|' || TO_CHAR(COUNT(*)) || '|' || TO_CHAR(COALESCE(MIN(ID), 0)) || '|' || TO_CHAR(COALESCE(MAX(ID), 0))
FROM $table;
SELECT 'delete_probe|' || TO_CHAR(COUNT(*))
FROM $table
WHERE ID BETWEEN $DELETE_START_ID AND $(delete_end_id);
SELECT 'sample|' || TO_CHAR(ID) || '|' || TRACER || '|' || PAYLOAD
FROM $table
WHERE ID IN (1, $BULK_START_ID, $((BULK_START_ID + 1)), $((BULK_START_ID + BULK_ROW_COUNT / 2)), $(bulk_end_id))
ORDER BY ID;
SQL
}

gaussdb_query() {
  local table="$1"
  local sql
  sql="$(pg_sql_summary "$table")"
  gaussdb_admin "$sql" |
    sed 's/^[^=]*=//'
}

gaussdb_admin() {
  local sql="$1"
  if [[ -x "$ROOT/target/debug/gaussdb_admin" ]]; then
    GAUSSDB_ADMIN_PREFIX=gaussdb_oracle_sinker \
      GAUSSDB_ADMIN_URL="$(gaussdb_conn_url)" \
      GAUSSDB_ADMIN_SQL="$sql" \
      "$ROOT/target/debug/gaussdb_admin" --quiet
    return
  fi

  GAUSSDB_ADMIN_PREFIX=gaussdb_oracle_sinker \
    GAUSSDB_ADMIN_URL="$(gaussdb_conn_url)" \
    GAUSSDB_ADMIN_SQL="$sql" \
    cargo run -p dt-tests --bin gaussdb_admin --quiet
}

oracle_query() {
  local table="$1"
  local password
  local sql
  password="$(secret_value ORACLE_PASSWORD oracle_sinker_password)"
  sql="$(oracle_sql_summary "$table")"

  docker exec -i "$ORACLE_CONTAINER" bash -lc "
export ORACLE_HOME=/u01/app/oracle/product/11.2.0/xe
export PATH=\$ORACLE_HOME/bin:\$PATH
export LD_LIBRARY_PATH=\$ORACLE_HOME/lib
sqlplus -s '$ORACLE_USER/$password@//$ORACLE_HOST:$ORACLE_PORT/$ORACLE_SERVICE'
" <<SQL | sed '/^[[:space:]]*$/d' | sed 's/^[[:space:]]*//;s/[[:space:]]*$//'
WHENEVER SQLERROR EXIT SQL.SQLCODE
SET PAGESIZE 0
SET FEEDBACK OFF
SET VERIFY OFF
SET HEADING OFF
SET ECHO OFF
SET TRIMSPOOL ON
SET TRIMOUT ON
SET LINESIZE 32767
$sql
EXIT
SQL
}

validate_state() {
  local label="$1"
  local state="$2"
  local summary delete_probe sample
  summary="$(printf '%s\n' "$state" | awk -F'|' '$1 == "summary" {print $0; exit}')"
  delete_probe="$(printf '%s\n' "$state" | awk -F'|' '$1 == "delete_probe" {print $0; exit}')"
  sample="$(printf '%s\n' "$state" | awk -F'|' '$1 == "sample" {print $2 "|" $3 "|" $4}')"

  [[ "$summary" == "summary|$(expected_total)|1|$(bulk_end_id)" ]] || return 1
  [[ "$delete_probe" == "delete_probe|0" ]] || return 1
  [[ "$sample" == "$(expected_sample)" ]] || return 1
  log "$label converged"
}

direction_converged() {
  local label="$1"
  local source="$2"
  local target="$3"

  validate_state "$label source" "$source" || return 1
  validate_state "$label target" "$target" || return 1
  [[ "$source" == "$target" ]] || return 1
}

read_all_states() {
  G2O_SOURCE_STATE="$(gaussdb_query public.t_gaussdb_oracle_to_oracle)"
  G2O_TARGET_STATE="$(oracle_query APE_DTS.T_GAUSSDB_ORACLE_TO_ORACLE)"
  O2G_SOURCE_STATE="$(oracle_query APE_DTS.T_ORACLE_TO_GAUSSDB_ORACLE)"
  O2G_TARGET_STATE="$(gaussdb_query public.t_oracle_to_gaussdb_oracle)"
}

print_state() {
  local label="$1"
  local state="$2"
  printf '%s\n%s\n' "$label" "$state"
}

verify_bulk_result() {
  local deadline
  local attempt=1
  deadline=$((SECONDS + VERIFY_TIMEOUT_SECS))
  while ((SECONDS <= deadline)); do
    log "poll $attempt"
    read_all_states
    if direction_converged "GaussDBOracle -> Oracle" "$G2O_SOURCE_STATE" "$G2O_TARGET_STATE" &&
      direction_converged "Oracle -> GaussDBOracle" "$O2G_SOURCE_STATE" "$O2G_TARGET_STATE"; then
      log "bulk CDC verification passed"
      return 0
    fi
    attempt=$((attempt + 1))
    sleep "$VERIFY_POLL_SECS"
  done

  print_state "GaussDBOracle source:" "$G2O_SOURCE_STATE"
  print_state "Oracle target:" "$G2O_TARGET_STATE"
  print_state "Oracle source:" "$O2G_SOURCE_STATE"
  print_state "GaussDBOracle target:" "$O2G_TARGET_STATE"
  die "bulk CDC verification did not converge within ${VERIFY_TIMEOUT_SECS}s"
}

usage() {
  cat <<USAGE
Usage: bash scripts/e2e/gaussdb_oracle_bulk_cdc_verify.sh

Environment overrides:
  GAUSSDB_RW_HOST=$GAUSSDB_RW_HOST
  GAUSSDB_PORT=$GAUSSDB_PORT
  GAUSSDB_DB=$GAUSSDB_DB
  GAUSSDB_USER=$GAUSSDB_USER
  ORACLE_SQLPLUS_DOCKER_CONTAINER=$ORACLE_CONTAINER
  ORACLE_HOST=$ORACLE_HOST
  ORACLE_PORT=$ORACLE_PORT
  ORACLE_SERVICE=$ORACLE_SERVICE
  ORACLE_USER=$ORACLE_USER
  ORACLE_PASSWORD=<from env or dt-tests/tests/.env.local>
  BULK_START_ID=$BULK_START_ID
  BULK_ROW_COUNT=$BULK_ROW_COUNT
  DELETE_START_ID=$DELETE_START_ID
  DELETE_ROW_COUNT=$DELETE_ROW_COUNT
  VERIFY_TIMEOUT_SECS=$VERIFY_TIMEOUT_SECS
  VERIFY_POLL_SECS=$VERIFY_POLL_SECS
USAGE
}

main() {
  case "${1:-}" in
    -h | --help | help)
      usage
      ;;
    "")
      require_cmd docker
      require_cmd cargo
      validate_config
      verify_bulk_result
      ;;
    *)
      usage
      die "unknown argument: $1"
      ;;
  esac
}

main "$@"
