# Spec: dt-tests（GaussDBOracle -> Oracle）CDC basic

## Goal

新增 `dt-tests` 集成用例 `gaussdb_oracle_to_oracle::cdc_tests::test::cdc_basic_test`，
覆盖 `insert/update/delete`，验证 `GaussDBOracle (cdc) -> OracleSinker` 主链路可跑通。

## Constraints

- 源端：复用既有 `GaussDBOracle`（oracle-mode）测试库。
- 目标端：本机 Oracle XE 11g docker（`ORACLE_SQLPLUS_DOCKER_CONTAINER` + `sqlplus`）。
- 不引入 OCI/JDBC 等新依赖。

## Acceptance / Validation

- `cargo test -p dt-tests --test integration_test -- gaussdb_oracle_to_oracle::cdc_tests::test::cdc_basic_test --nocapture` PASS

