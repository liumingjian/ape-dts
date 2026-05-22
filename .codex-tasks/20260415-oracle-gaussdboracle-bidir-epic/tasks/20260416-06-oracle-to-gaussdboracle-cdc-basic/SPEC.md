# Spec: Oracle -> GaussDBOracle CDC basic (bootstrap)

## Goal

在现有 `OracleSqlPlusClient` + `Oracle XE 11g docker` 环境基础上，补齐 `Oracle -> GaussDBOracle` 的 **CDC basic**：

- 支持最小 DML 链路：`insert / update / delete`
- 提供 `dt-tests` 可回归入口：`oracle_to_gaussdb_oracle::cdc_tests::test::cdc_basic_test`

## Constraints / Assumptions

- Oracle 连接栈不引入 OCI/JDBC 依赖，继续使用 `sqlplus`（容器内 `docker exec`）。
- CDC 方案采用 **trigger-based**（bootstrap）：在源端为目标表创建触发器，将变更写入日志表；extractor 轮询日志表并转成 `RowData`。
- 不承诺 resume/failover/DDL-CDC；仅交付 basic 用例可跑通并可回归。

## Non-Goals

- 不实现 LogMiner/OGG 级别的 redo 挖掘 CDC
- 不实现多表复杂类型覆盖（本任务只做 basic fixture 覆盖）

## Deliverables

- `dt-common`：`DbType::Oracle + extract_type=cdc` 配置解析支持
- `dt-connector`：新增 `OracleCdcExtractor`（trigger-based）
- `dt-task`：打通 `ConnClient` + `ExtractorUtil` 路由
- `dt-tests`：新增 `oracle_to_gaussdb_oracle/cdc/basic_test` fixture + `cdc_basic_test`
- `dt-tests/docker-compose.oracle_xe.yml`：补齐 CDC 所需权限（如 CREATE TRIGGER）

## Acceptance / Validation

- `cargo test -p dt-tests --test integration_test -- oracle_to_gaussdb_oracle::cdc_tests::test::cdc_basic_test --nocapture` PASS

