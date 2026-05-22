#!/usr/bin/env bash
set -euo pipefail

# GaussDBPg -> MySQL bootstrap regression runner.
#
# Usage:
#   bash scripts/e2e/gaussdb_to_mysql_bootstrap.sh          # quick
#   SUITE=full bash scripts/e2e/gaussdb_to_mysql_bootstrap.sh
#
# Notes:
# - dt-tests will load `dt-tests/tests/.env.local` (if present) and `dt-tests/tests/.env`.
# - To speed up RW probing in shared HA environments you may optionally set:
#     export gaussdb_pg_candidate_hosts=10.250.0.51:8000

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

SUITE="${SUITE:-quick}"

tests_quick=(
  "gaussdb_to_mysql::snapshot_tests::test::snapshot_basic_test"
  "gaussdb_to_mysql::struct_tests::test::struct_basic_test"
  "gaussdb_to_mysql::check_tests::test::check_basic_test"
  "gaussdb_to_mysql::cdc_tests::test::cdc_basic_test"
  "gaussdb_to_mysql::precheck_tests::test::struct_supported_basic_test"
)

tests_full=(
  "${tests_quick[@]}"
  "gaussdb_to_mysql::struct_tests::test::struct_advanced_test"
)

case "$SUITE" in
  quick)
    selected=("${tests_quick[@]}")
    ;;
  full)
    selected=("${tests_full[@]}")
    ;;
  *)
    echo "Unknown SUITE=$SUITE (expected: quick|full)" >&2
    exit 2
    ;;
esac

echo "[gaussdb_to_mysql_bootstrap] suite=$SUITE"
echo "[gaussdb_to_mysql_bootstrap] running ${#selected[@]} tests"

# Run all selected tests in one invocation to avoid repeated compilation.
cargo test -p dt-tests --test integration_test -- "${selected[@]}" --nocapture

