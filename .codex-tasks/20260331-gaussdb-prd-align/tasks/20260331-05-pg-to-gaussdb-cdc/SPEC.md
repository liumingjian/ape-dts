# PG → GaussDBPg CDC（MVP）

## 目标

对齐 PRD MVP 同步矩阵，补齐 `PG → GaussDBPg` 的 CDC 链路：

- extractor：`PgCdc`（pgoutput）
- sinker：`PgWriter`（db_type=gaussdb_pg）
- dt-tests 覆盖：最小 `cdc_basic_test`（insert/update/delete）

## 成功标准

1. 新增 `dt-tests/tests/pg_to_gaussdb/cdc/basic_test`。
2. `dt-tests` 新增 `pg_to_gaussdb::cdc_tests::test::cdc_basic_test`。
3. 在真实环境（本机 Docker PG 作为源，远端 GaussDBPg 为目标）跑通并归档脱敏证据。

## 验收（最小）

```bash
cargo test -p dt-tests --test integration_test -- pg_to_gaussdb::cdc_tests::test::cdc_basic_test --nocapture
```

