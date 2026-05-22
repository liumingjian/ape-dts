# SPEC — dt-tests Oracle -> GaussDBOracle Snapshot Smoke

## Goal

新增 `dt-tests` 用例：`oracle_to_gaussdb_oracle::snapshot::smoke_test`，在本机 Oracle XE + 远端 GaussDBOracle (oracle-mode testdb) 上跑通：

- Oracle 源端插入数据
- ape-dts snapshot 迁移到 GaussDBOracle
- compare_data PASS（通过 router `tb_map/col_map` 处理大小写差异）

## Acceptance Criteria

- `cargo test -p dt-tests --test integration_test -- oracle_to_gaussdb_oracle::snapshot_tests::test::smoke_test --nocapture` PASS

