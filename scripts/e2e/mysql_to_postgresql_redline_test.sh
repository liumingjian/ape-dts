#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=mysql_to_postgresql_redline.sh
source "$SCRIPT_DIR/mysql_to_postgresql_redline.sh"

failures=0

assert_eq() {
  local name="$1"
  local expected="$2"
  local actual="$3"
  if [[ "$actual" != "$expected" ]]; then
    printf 'not ok - %s\nexpected: <%s>\nactual:   <%s>\n' "$name" "$expected" "$actual" >&2
    failures=$((failures + 1))
    return
  fi
  printf 'ok - %s\n' "$name"
}

assert_fails() {
  local name="$1"
  shift
  if "$@" >/dev/null 2>&1; then
    printf 'not ok - %s (command unexpectedly succeeded)\n' "$name" >&2
    failures=$((failures + 1))
    return
  fi
  printf 'ok - %s\n' "$name"
}

assert_eq "sanitize compose project name" \
  "ape-dts-mysql-pg-20260716-120102-42" \
  "$(sanitize_run_id 'APE DTS/mysql_pg@20260716_120102#42')"

assert_eq "parse IPv4 compose port" "49152" "$(parse_compose_port '127.0.0.1:49152')"
assert_eq "parse IPv6 compose port" "49153" "$(parse_compose_port '[::1]:49153')"
assert_fails "reject missing compose port" parse_compose_port ""
assert_fails "reject non-numeric compose port" parse_compose_port "127.0.0.1:http"

master_status=$'mysql-bin.000007\t154\t\t\t'
assert_eq "parse master status filename" "mysql-bin.000007" "$(parse_master_status "$master_status" filename)"
assert_eq "parse master status position" "154" "$(parse_master_status "$master_status" position)"
assert_fails "reject empty master status" parse_master_status "" filename
assert_fails "reject invalid master status position" parse_master_status $'mysql-bin.000007\tbad' position

snapshot_expected=$'1|ORD-001|Alice|100.50|created|<NULL>\n2|ORD-002|Bob|220.00|created|snapshot row\n3|ORD-003|Carol|19.99|created|will be deleted'
assert_eq "snapshot expected literal" "$snapshot_expected" "$(expected_rows snapshot)"

insert_expected="${snapshot_expected}"$'\n4|ORD-004|David|88.80|created|cdc insert'
assert_eq "insert expected literal" "$insert_expected" "$(expected_rows insert)"

update_expected=$'1|ORD-001|Alice|188.80|paid|cdc update\n2|ORD-002|Bob|220.00|created|snapshot row\n3|ORD-003|Carol|19.99|created|will be deleted\n4|ORD-004|David|88.80|created|cdc insert'
assert_eq "update expected literal" "$update_expected" "$(expected_rows update)"

final_expected=$'1|ORD-001|Alice|188.80|paid|cdc update\n2|ORD-002|Bob|220.00|created|snapshot row\n4|ORD-004|David|88.80|created|cdc insert'
assert_eq "delete expected literal" "$final_expected" "$(expected_rows delete)"
assert_eq "final expected literal" "$final_expected" "$(expected_rows final)"
assert_fails "reject unknown expected phase" expected_rows unknown

assert_eq "mysql normalized query" \
  "SELECT CONCAT_WS('|', id, order_no, customer_name, CAST(amount AS CHAR), status, COALESCE(note, '<NULL>')) FROM ape_dts_e2e.migration_redline_orders ORDER BY id;" \
  "$(mysql_dump_query)"
assert_eq "postgres normalized query" \
  "SELECT id || '|' || order_no || '|' || customer_name || '|' || to_char(amount, 'FM9999999990.00') || '|' || status || '|' || COALESCE(note, '<NULL>') FROM public.migration_redline_orders ORDER BY id;" \
  "$(postgres_dump_query)"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

snapshot_rows="$(expected_rows snapshot)"
printf '%s\n' "$snapshot_rows" >"$tmp_dir/ok-mysql.tsv"
printf '%s\n' "$snapshot_rows" >"$tmp_dir/ok-postgresql.tsv"
printf '%s\n' "$(expected_rows insert)" >"$tmp_dir/drift.tsv"

assert_eq "phase_mismatch_reason silent when both sides match expectation" "" \
  "$(phase_mismatch_reason snapshot "$tmp_dir/ok-mysql.tsv" "$tmp_dir/ok-postgresql.tsv" "$tmp_dir/ok.diff")"
assert_eq "phase_mismatch_reason names the source side" \
  "snapshot source rows differ from fixed expectation" \
  "$(phase_mismatch_reason snapshot "$tmp_dir/drift.tsv" "$tmp_dir/ok-postgresql.tsv" "$tmp_dir/src.diff" || true)"
assert_eq "phase_mismatch_reason names the target side" \
  "snapshot target rows differ from fixed expectation" \
  "$(phase_mismatch_reason snapshot "$tmp_dir/ok-mysql.tsv" "$tmp_dir/drift.tsv" "$tmp_dir/tgt.diff" || true)"
assert_fails "phase_mismatch_reason rejects unknown phase" \
  phase_mismatch_reason unknown "$tmp_dir/ok-mysql.tsv" "$tmp_dir/ok-postgresql.tsv" "$tmp_dir/unknown.diff"

# dump_phase must report an unreachable database as such, not as "rows differ".
dump_phase_dump_failure_reason() {
  (
    RUN_DIR="$tmp_dir"
    mkdir -p "$RUN_DIR/dumps" "$RUN_DIR/diffs"
    dump_mysql() { return 1; }
    dump_postgres() { printf '%s\n' "$(expected_rows snapshot)"; }
    dump_phase snapshot || true
    printf '%s' "$PHASE_MISMATCH_REASON"
  )
}
assert_eq "dump_phase distinguishes an unreachable source from a data mismatch" \
  "snapshot source dump failed (mysql unreachable)" \
  "$(dump_phase_dump_failure_reason)"

# The regression this ticket fixes: on timeout the run must fail with the
# timeout reason, not with whatever assert_phase's diff happened to say.
wait_for_phase_timeout_output() {
  (
    RUN_DIR="$tmp_dir"
    require_cdc_alive() { :; }
    phase_matches() { return 1; }
    dump_phase() {
      PHASE_DIFF_FILE="$RUN_DIR/diffs/insert.diff"
      PHASE_MISMATCH_REASON="insert source and target rows differ"
      return 1
    }
    wait_for_phase insert 0 2>&1
  )
}
timeout_output="$(wait_for_phase_timeout_output || true)"
timeout_status=0
wait_for_phase_timeout_output >/dev/null 2>&1 || timeout_status=$?

assert_eq "wait_for_phase timeout exits non-zero" "1" "$timeout_status"
case "$timeout_output" in
  *"insert did not converge within 0s"*)
    printf 'ok - %s\n' "wait_for_phase timeout reports the timeout as the reason" ;;
  *)
    printf 'not ok - %s\nactual: <%s>\n' "wait_for_phase timeout reports the timeout as the reason" "$timeout_output" >&2
    failures=$((failures + 1)) ;;
esac
case "$timeout_output" in
  *"last observed: insert source and target rows differ"*)
    printf 'ok - %s\n' "wait_for_phase timeout keeps the last observed mismatch as detail" ;;
  *)
    printf 'not ok - %s\nactual: <%s>\n' "wait_for_phase timeout keeps the last observed mismatch as detail" "$timeout_output" >&2
    failures=$((failures + 1)) ;;
esac

# assert_phase still fails loudly, and with the specific reason.
assert_phase_failure_output() {
  (
    RUN_DIR="$tmp_dir"
    dump_phase() {
      PHASE_DIFF_FILE="$RUN_DIR/diffs/snapshot.diff"
      PHASE_MISMATCH_REASON="snapshot target rows differ from fixed expectation"
      return 1
    }
    assert_phase snapshot 2>&1
  )
}
assert_phase_output="$(assert_phase_failure_output || true)"
case "$assert_phase_output" in
  *"snapshot target rows differ from fixed expectation"*)
    printf 'ok - %s\n' "assert_phase dies with the mismatch reason" ;;
  *)
    printf 'not ok - %s\nactual: <%s>\n' "assert_phase dies with the mismatch reason" "$assert_phase_output" >&2
    failures=$((failures + 1)) ;;
esac

if (( failures > 0 )); then
  printf '%d test(s) failed\n' "$failures" >&2
  exit 1
fi

printf 'all tests passed\n'
