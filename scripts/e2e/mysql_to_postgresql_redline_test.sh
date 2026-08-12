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

# --- readiness race (ticket 34) ---------------------------------------------
# The MySQL image serves a temporary server during initialization and then
# restarts it. A single successful probe must not be mistaken for readiness.

# Drives wait_for_databases against a scripted probe sequence.
# $1: space-separated per-tick outcomes for mysql ("ok"/"fail")
# $2: required streak
# Prints "<status> <ticks-consumed>".
run_readiness() {
  local script="$1"
  local required="$2"
  (
    DOCKER_TIMEOUT_SECS=30
    DB_READY_STREAK_REQUIRED="$required"
    DB_READY_PROBE_INTERVAL_SECS=0
    tick=0
    outcomes=($script)
    services_healthy() { :; }
    probe_postgres() { :; }
    probe_mysql() {
      local outcome="${outcomes[$tick]:-fail}"
      tick=$((tick + 1))
      [[ "$outcome" == "ok" ]]
    }
    sleep() { :; }
    log() { :; }
    status=0
    wait_for_databases >/dev/null 2>&1 || status=$?
    printf '%s %s' "$status" "$tick"
  )
}

assert_eq "readiness needs consecutive successes, not just one" \
  "0 5" "$(run_readiness "ok fail ok ok ok" 3)"
assert_eq "readiness returns as soon as the streak is met" \
  "0 3" "$(run_readiness "ok ok ok" 3)"

# A probe that flaps forever must time out, and say why.
readiness_timeout_output() {
  (
    DOCKER_TIMEOUT_SECS=3
    DB_READY_STREAK_REQUIRED=3
    DB_READY_PROBE_INTERVAL_SECS=0
    services_healthy() { :; }
    probe_mysql() { :; }
    probe_postgres() { return 1; }
    sleep() { SECONDS=$((SECONDS + 1)); }
    log() { :; }
    wait_for_databases 2>&1
  )
}
readiness_output="$(readiness_timeout_output || true)"
case "$readiness_output" in
  *"database readiness exceeded"*"postgresql not accepting connections yet"*)
    printf 'ok - %s\n' "readiness timeout names the side that never came up" ;;
  *)
    printf 'not ok - %s\nactual: <%s>\n' "readiness timeout names the side that never came up" "$readiness_output" >&2
    failures=$((failures + 1)) ;;
esac

# An unhealthy container must never be counted toward the streak.
readiness_unhealthy_output() {
  (
    DOCKER_TIMEOUT_SECS=3
    DB_READY_STREAK_REQUIRED=1
    DB_READY_PROBE_INTERVAL_SECS=0
    services_healthy() { return 1; }
    probe_mysql() { :; }
    probe_postgres() { :; }
    sleep() { SECONDS=$((SECONDS + 1)); }
    log() { :; }
    wait_for_databases 2>&1
  )
}
readiness_unhealthy="$(readiness_unhealthy_output || true)"
case "$readiness_unhealthy" in
  *"container healthcheck not healthy yet"*)
    printf 'ok - %s\n' "readiness gates on the compose healthcheck" ;;
  *)
    printf 'not ok - %s\nactual: <%s>\n' "readiness gates on the compose healthcheck" "$readiness_unhealthy" >&2
    failures=$((failures + 1)) ;;
esac

# services_healthy accepts an image without a healthcheck, rejects "starting".
assert_eq "services_healthy rejects a starting container" "not-ready" \
  "$(service_health() { printf 'starting'; }; services_healthy && printf ready || printf not-ready)"
assert_eq "services_healthy accepts an image with no healthcheck" "ready" \
  "$(service_health() { printf 'none'; }; services_healthy && printf ready || printf not-ready)"
assert_eq "services_healthy accepts healthy containers" "ready" \
  "$(service_health() { printf 'healthy'; }; services_healthy && printf ready || printf not-ready)"
assert_eq "services_healthy rejects an unknown container" "not-ready" \
  "$(service_health() { return 1; }; services_healthy && printf ready || printf not-ready)"

# --- failure reason must survive a stage that bypasses die() ------------------
assert_eq "summary_reason keeps the explicit die reason" \
  "snapshot dt-main failed with exit code 3" \
  "$(FAILURE_REASON="snapshot dt-main failed with exit code 3" LAST_ERR_REASON="" summary_reason 1)"
assert_eq "summary_reason falls back to the trapped command on a set -e abort" \
  "stage snapshot-prepare aborted: \`mysql_sql\` exited with status 1 (redline.sh:42)" \
  "$(FAILURE_REASON="" LAST_ERR_REASON="stage snapshot-prepare aborted: \`mysql_sql\` exited with status 1 (redline.sh:42)" summary_reason 1)"
assert_eq "summary_reason never reports none on a failed run" \
  "unknown failure (no reason recorded)" \
  "$(FAILURE_REASON="" LAST_ERR_REASON="" summary_reason 1)"
assert_eq "summary_reason reports none on a passing run" "none" \
  "$(FAILURE_REASON="" LAST_ERR_REASON="stray" summary_reason 0)"

record_err_reason() {
  (
    CURRENT_STAGE="snapshot-prepare"
    record_err 1 42 "mysql_sql"
    printf '%s' "$LAST_ERR_REASON"
  )
}
case "$(record_err_reason)" in
  "stage snapshot-prepare aborted: \`mysql_sql\` exited with status 1 ("*)
    printf 'ok - %s\n' "record_err names the stage and the failing command" ;;
  *)
    printf 'not ok - %s\nactual: <%s>\n' "record_err names the stage and the failing command" "$(record_err_reason)" >&2
    failures=$((failures + 1)) ;;
esac

# cleanup runs stop_cdc before write_summary; neither may lose the die reason.
summary_survives_shutdown() {
  (
    RUN_DIR="$tmp_dir"
    CURRENT_STAGE="snapshot-prepare"
    FAILURE_REASON="schema and fixture preparation failed"
    LAST_ERR_REASON=""
    CDC_PID=""
    stop_cdc
    write_summary 1
    grep -F -- '- Reason:' "$RUN_DIR/summary.md"
  )
}
assert_eq "shutdown path preserves the failure reason in summary.md" \
  "- Reason: schema and fixture preparation failed" \
  "$(summary_survives_shutdown)"

# Graceful stop (SIGTERM): the acceptance is "drained + position recorded + non-zero exit",
# and each failure mode must name itself instead of collapsing into one vague message.
printf 'checkpoint_position | mysql-bin.000003:100\n' >"$tmp_dir/position.log"
printf 'checkpoint_position | mysql-bin.000003:842\n' >>"$tmp_dir/position.log"
printf 'other line\n' >>"$tmp_dir/position.log"
assert_eq "last_checkpoint_position reads the newest recorded position" \
  "mysql-bin.000003:842" "$(last_checkpoint_position "$tmp_dir/position.log")"
assert_eq "last_checkpoint_position is empty when nothing was recorded" \
  "" "$(last_checkpoint_position "$tmp_dir/missing-position.log")"

assert_eq "graceful_stop_reason accepts a clean shutdown" "" \
  "$(graceful_stop_reason 143 "$GRACEFUL_PROBE_ROW" "$GRACEFUL_PROBE_ROW" "mysql-bin.000003:100" "mysql-bin.000003:842")"
assert_eq "graceful_stop_reason rejects a zero exit code after SIGTERM" \
  "cdc dt-main exited with code 0 after SIGTERM, expected 143" \
  "$(graceful_stop_reason 0 "$GRACEFUL_PROBE_ROW" "$GRACEFUL_PROBE_ROW" "mysql-bin.000003:100" "mysql-bin.000003:842")"
assert_eq "graceful_stop_reason rejects a hard kill exit code" \
  "cdc dt-main exited with code 137 after SIGTERM, expected 143" \
  "$(graceful_stop_reason 137 "$GRACEFUL_PROBE_ROW" "$GRACEFUL_PROBE_ROW" "mysql-bin.000003:100" "mysql-bin.000003:842")"
assert_eq "graceful_stop_reason blames the fixture when the probe never reached mysql" \
  "the graceful-stop probe row never landed in mysql, so the shutdown was not exercised" \
  "$(graceful_stop_reason 143 "" "" "mysql-bin.000003:100" "mysql-bin.000003:842")"
assert_eq "graceful_stop_reason reports data dropped by the shutdown" \
  "the row written just before SIGTERM never reached postgresql: the shutdown dropped buffered data" \
  "$(graceful_stop_reason 143 "$GRACEFUL_PROBE_ROW" "" "mysql-bin.000003:100" "mysql-bin.000003:842")"
assert_eq "graceful_stop_reason reports a missing resume point" \
  "no checkpoint position was recorded, the shutdown left no resume point" \
  "$(graceful_stop_reason 143 "$GRACEFUL_PROBE_ROW" "$GRACEFUL_PROBE_ROW" "" "")"
assert_eq "graceful_stop_reason reports a position that never advanced" \
  "the checkpoint position did not advance past the pre-shutdown position (mysql-bin.000003:100): the final position was not recorded" \
  "$(graceful_stop_reason 143 "$GRACEFUL_PROBE_ROW" "$GRACEFUL_PROBE_ROW" "mysql-bin.000003:100" "mysql-bin.000003:100")"

if (( failures > 0 )); then
  printf '%d test(s) failed\n' "$failures" >&2
  exit 1
fi

printf 'all tests passed\n'
