# Spec

## Goal

为 `Oracle ↔ GaussDBOracle`（bootstrap）补齐 `dt-tests` precheck 回归入口，确保 `dt-precheck` 在 Oracle source/sink 场景可执行。

## Scope

- `PrecheckTestRunner` 支持 `DbType::Oracle` 的 prepare/clean
- 新增 testcase 目录：
  - `dt-tests/tests/oracle_to_gaussdb_oracle/precheck/struct_supported_basic_test`
  - `dt-tests/tests/gaussdb_oracle_to_oracle/precheck/struct_supported_basic_test`
- 新增集成测试入口：
  - `oracle_to_gaussdb_oracle::precheck_tests::test::struct_supported_basic_test`
  - `gaussdb_oracle_to_oracle::precheck_tests::test::struct_supported_basic_test`

## Acceptance

- 两条 precheck 测试均 PASS

