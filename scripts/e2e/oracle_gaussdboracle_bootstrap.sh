#!/usr/bin/env bash
set -euo pipefail

# Oracle <-> GaussDBOracle bootstrap regression runner.
#
# Usage:
#   bash scripts/e2e/oracle_gaussdboracle_bootstrap.sh
#
# Notes:
# - dt-tests will load `dt-tests/tests/.env.local` (if present) and `dt-tests/tests/.env`.
# - This script ensures local Oracle XE is up (wnameless/oracle-xe-11g-r2:latest).

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

tests=(
  "oracle_to_gaussdb_oracle::snapshot_tests::test::smoke_test"
  "oracle_to_gaussdb_oracle::struct_tests::test::struct_basic_test"
  "oracle_to_gaussdb_oracle::check_tests::test::check_basic_test"
  "oracle_to_gaussdb_oracle::cdc_tests::test::cdc_basic_test"
  "oracle_to_gaussdb_oracle::precheck_tests::test::struct_supported_basic_test"
  "gaussdb_oracle_to_oracle::snapshot_tests::test::smoke_test"
  "gaussdb_oracle_to_oracle::struct_tests::test::struct_basic_test"
  "gaussdb_oracle_to_oracle::check_tests::test::check_basic_test"
  "gaussdb_oracle_to_oracle::cdc_tests::test::cdc_basic_test"
  "gaussdb_oracle_to_oracle::precheck_tests::test::struct_supported_basic_test"
)

echo "[oracle_gaussdboracle_bootstrap] ensuring local Oracle XE docker is up"
docker compose -f dt-tests/docker-compose.oracle_xe.yml up -d

echo "[oracle_gaussdboracle_bootstrap] running ${#tests[@]} tests"
payload=("${tests[@]}")

# Run all selected tests in one invocation to avoid repeated compilation.
cargo test -p dt-tests --test integration_test -- "${payload[@]}" --nocapture
