# SPEC — dt-tests GaussDBOracle -> Oracle Snapshot Smoke

## Goal

新增 `dt-tests` 用例：`gaussdb_oracle_to_oracle::snapshot::smoke_test`，在远端 GaussDBOracle (oracle-mode testdb) + 本机 Oracle XE 上跑通：

- GaussDBOracle 源端插入数据
- ape-dts snapshot 迁移到 Oracle（使用新 `OracleSinker`）
- compare_data PASS（通过 router `tb_map/col_map` 处理大小写差异）

## Acceptance Criteria

- `cargo test -p dt-tests --test integration_test -- gaussdb_oracle_to_oracle::snapshot_tests::test::smoke_test --nocapture` PASS

