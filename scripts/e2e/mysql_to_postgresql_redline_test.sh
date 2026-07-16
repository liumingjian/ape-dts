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

if (( failures > 0 )); then
  printf '%d test(s) failed\n' "$failures" >&2
  exit 1
fi

printf 'all tests passed\n'
