#!/usr/bin/env bash
set -euo pipefail

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
DEFAULT_BULK_COMMIT_EVERY="200"
DEFAULT_DELETE_START_ID="900000"
DEFAULT_DELETE_ROW_COUNT="200"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ENV_LOCAL="$ROOT/dt-tests/tests/.env.local"

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
BULK_COMMIT_EVERY="${BULK_COMMIT_EVERY:-$DEFAULT_BULK_COMMIT_EVERY}"
DELETE_START_ID="${DELETE_START_ID:-$DEFAULT_DELETE_START_ID}"
DELETE_ROW_COUNT="${DELETE_ROW_COUNT:-$DEFAULT_DELETE_ROW_COUNT}"

log() {
  printf '[gaussdb-oracle-bulk-cdc] %s\n' "$*"
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
  require_positive_integer BULK_COMMIT_EVERY "$BULK_COMMIT_EVERY"
  require_positive_integer DELETE_START_ID "$DELETE_START_ID"
  require_positive_integer DELETE_ROW_COUNT "$DELETE_ROW_COUNT"
  [[ -n "$(secret_value GAUSSDB_PASSWORD gaussdb_oracle_sinker_password)" ]] ||
    die "missing GAUSSDB_PASSWORD or gaussdb_oracle_sinker_password in $ENV_LOCAL"
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

generate_pg_sql() {
  local file="$1"
  local table="$2"

  {
    printf 'BEGIN;\n'
    printf 'DELETE FROM %s WHERE id >= 2;\n' "$table"
    printf "UPDATE %s SET tracer='bulk_update', payload='bulk_after_update' WHERE id=1;\n" "$table"
    printf 'COMMIT;\n'
    append_pg_insert_transactions "$table" bulk
    append_pg_insert_transactions "$table" delete_probe
    printf 'BEGIN;\n'
    printf 'DELETE FROM %s WHERE id BETWEEN %s AND %s;\n' "$table" "$DELETE_START_ID" "$(delete_end_id)"
    printf 'COMMIT;\n'
  } >"$file"
}

append_pg_insert_transactions() {
  local table="$1"
  local kind="$2"
  local start_id
  local row_count
  local tracer_prefix
  local chunk_start
  local chunk_count
  local offset
  local id

  case "$kind" in
    bulk)
      start_id="$BULK_START_ID"
      row_count="$BULK_ROW_COUNT"
      tracer_prefix="bulk_insert"
      ;;
    delete_probe)
      start_id="$DELETE_START_ID"
      row_count="$DELETE_ROW_COUNT"
      tracer_prefix="bulk_delete_probe"
      ;;
    *) die "unknown pg insert kind: $kind" ;;
  esac

  for chunk_start in $(seq 0 "$BULK_COMMIT_EVERY" "$((row_count - 1))"); do
    chunk_count="$BULK_COMMIT_EVERY"
    if ((chunk_start + chunk_count > row_count)); then
      chunk_count="$((row_count - chunk_start))"
    fi
    printf 'BEGIN;\n'
    printf 'INSERT INTO %s (id, tracer, payload) VALUES\n' "$table"
    for offset in $(seq "$chunk_start" "$((chunk_start + chunk_count - 1))"); do
      id="$((start_id + offset))"
      printf "  (%s, '%s_%06d', 'payload_%06d')%s\n" \
        "$id" "$tracer_prefix" "$((offset + 1))" "$((offset + 1))" \
        "$(insert_row_suffix "$((offset - chunk_start))" "$chunk_count")"
    done
    printf 'COMMIT;\n'
  done
}

insert_row_suffix() {
  local offset="$1"
  local row_count="$2"
  if [[ "$offset" -eq "$((row_count - 1))" ]]; then
    printf ';'
  else
    printf ','
  fi
}

generate_oracle_sql() {
  local file="$1"
  local table="$2"

  {
    printf 'WHENEVER SQLERROR EXIT SQL.SQLCODE\n'
    printf 'DELETE FROM %s WHERE ID >= 2;\n' "$table"
    printf "UPDATE %s SET TRACER='bulk_update', PAYLOAD='bulk_after_update' WHERE ID=1;\n" "$table"
    printf 'COMMIT;\n'
    append_oracle_insert_transactions "$table" bulk
    append_oracle_insert_transactions "$table" delete_probe
    printf 'DELETE FROM %s WHERE ID BETWEEN %s AND %s;\n' "$table" "$DELETE_START_ID" "$(delete_end_id)"
    printf 'COMMIT;\n'
    printf 'EXIT\n'
  } >"$file"
}

append_oracle_insert_transactions() {
  local table="$1"
  local kind="$2"
  local start_id
  local row_count
  local tracer_prefix
  local chunk_start
  local chunk_count
  local offset
  local id

  case "$kind" in
    bulk)
      start_id="$BULK_START_ID"
      row_count="$BULK_ROW_COUNT"
      tracer_prefix="bulk_insert"
      ;;
    delete_probe)
      start_id="$DELETE_START_ID"
      row_count="$DELETE_ROW_COUNT"
      tracer_prefix="bulk_delete_probe"
      ;;
    *) die "unknown oracle insert kind: $kind" ;;
  esac

  for chunk_start in $(seq 0 "$BULK_COMMIT_EVERY" "$((row_count - 1))"); do
    chunk_count="$BULK_COMMIT_EVERY"
    if ((chunk_start + chunk_count > row_count)); then
      chunk_count="$((row_count - chunk_start))"
    fi
    for offset in $(seq "$chunk_start" "$((chunk_start + chunk_count - 1))"); do
      id="$((start_id + offset))"
      printf "INSERT INTO %s (ID, TRACER, PAYLOAD) VALUES (%s, '%s_%06d', 'payload_%06d');\n" \
        "$table" "$id" "$tracer_prefix" "$((offset + 1))" "$((offset + 1))"
    done
    printf 'COMMIT;\n'
  done
}

apply_gaussdb_source_changes() {
  local sql_file="$1"
  local url

  log "applying GaussDBOracle source CDC mock rows on $GAUSSDB_RW_HOST:$GAUSSDB_PORT"
  url="$(gaussdb_conn_url)"
  GAUSSDB_ADMIN_PREFIX=gaussdb_oracle_sinker \
    GAUSSDB_ADMIN_URL="$url" \
    GAUSSDB_ADMIN_SQL_FILE="$sql_file" \
    cargo run -p dt-tests --bin gaussdb_admin --quiet
}

apply_oracle_source_changes() {
  local sql_file="$1"
  local password
  password="$(secret_value ORACLE_PASSWORD oracle_sinker_password)"

  log "applying Oracle source CDC mock rows through container $ORACLE_CONTAINER"
  docker exec -i "$ORACLE_CONTAINER" bash -lc "
export ORACLE_HOME=/u01/app/oracle/product/11.2.0/xe
export PATH=\$ORACLE_HOME/bin:\$PATH
export LD_LIBRARY_PATH=\$ORACLE_HOME/lib
sqlplus -s '$ORACLE_USER/$password@//$ORACLE_HOST:$ORACLE_PORT/$ORACLE_SERVICE'
" <"$sql_file"
}

apply_bulk_changes() {
  local work_dir
  local gaussdb_sql
  local oracle_sql
  work_dir="$(mktemp -d)"
  gaussdb_sql="$work_dir/gaussdb-oracle-source.sql"
  oracle_sql="$work_dir/oracle-source.sql"
  trap "rm -rf '$work_dir'" EXIT

  generate_pg_sql "$gaussdb_sql" "public.t_gaussdb_oracle_to_oracle"
  generate_oracle_sql "$oracle_sql" "APE_DTS.T_ORACLE_TO_GAUSSDB_ORACLE"
  apply_gaussdb_source_changes "$gaussdb_sql"
  apply_oracle_source_changes "$oracle_sql"

  log "bulk CDC mock data applied"
  log "expected final row count per table: $((BULK_ROW_COUNT + 1))"
  log "expected bulk id range: $BULK_START_ID..$(bulk_end_id)"
  log "expected deleted probe range: $DELETE_START_ID..$(delete_end_id)"
  log "source transaction chunk size: $BULK_COMMIT_EVERY rows"
}

usage() {
  cat <<USAGE
Usage: bash scripts/e2e/gaussdb_oracle_bulk_cdc_mock.sh apply

Key overrides: GAUSSDB_RW_HOST, BULK_ROW_COUNT, BULK_COMMIT_EVERY,
DELETE_ROW_COUNT, ORACLE_SQLPLUS_DOCKER_CONTAINER, GAUSSDB_PASSWORD,
ORACLE_PASSWORD. Passwords default to dt-tests/tests/.env.local.
USAGE
}

main() {
  local phase="${1:-}"
  case "$phase" in
    apply)
      require_cmd docker
      require_cmd cargo
      validate_config
      apply_bulk_changes
      ;;
    -h | --help | help | "")
      usage
      ;;
    *)
      usage
      die "unknown phase: $phase"
      ;;
  esac
}

main "$@"
