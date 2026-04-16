# Epic Specification

## Goal

交付 `GaussDBOracle -> Oracle` 的 **CDC basic**（DML：insert/update/delete）：

- 扩展 `OracleSinker` 支持 `UPDATE/DELETE`（从 bootstrap snapshot 升级为可承载 CDC DML）。
- 新增 `dt-tests` 回归入口：`gaussdb_oracle_to_oracle::cdc_tests::test::cdc_basic_test`。

## Constraints

- Oracle 环境：本机 Docker 镜像 `wnameless/oracle-xe-11g-r2:latest`（compose：`dt-tests/docker-compose.oracle_xe.yml`）。
- GaussDBOracle 环境：沿用远端 oracle-mode `testdb`（通过 `dt-tests/tests/.env.local` 注入）。
- 本 Epic 只做 **CDC basic**，不承诺 resume/failover/DDL-CDC。

## Done-When

- `SUBTASKS.csv` 全部为 `DONE`
- `cargo test -p dt-tests --test integration_test -- gaussdb_oracle_to_oracle::cdc_tests::test::cdc_basic_test --nocapture` 通过

