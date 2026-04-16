# Epic Specification

## Goal

补齐 `GaussDBOracle -> Oracle` 的 bootstrap 能力缺口：

- 支持 `sink_type=struct`：将源端（GaussDBOracle/pg-wire）结构同步到 Oracle XE（目标端）。
- 支持 `sink_type=check`：对账源端数据与 Oracle 目标端数据（basic）。
- 新增 `dt-tests` 回归入口并纳入一键脚本。

## Non-Goals

- 不做 DDL-CDC / resume / failover。
- 不承诺覆盖复杂类型/对象（以 basic 的 table + primary key + 常见列类型为边界）。

## Constraints

- Oracle 环境：本机 Docker 镜像 `wnameless/oracle-xe-11g-r2:latest`（compose：`dt-tests/docker-compose.oracle_xe.yml`）。
- GaussDBOracle 环境：沿用远端 oracle-mode `testdb`（通过 `dt-tests/tests/.env.local` 注入，禁止提交凭据）。
- Debug-first：不引入 silent fallback；遇到不支持类型/结构直接显式失败。

## Child Deliverables

- `dt-common`：`DbType::Oracle` 支持 `sink_type=struct/check` 的配置解析与 `SinkerConfig` 变体。
- `dt-connector`：新增 `OracleStructSinker` 与 `OracleChecker`（复用 `OracleSqlPlusClient`）。
- `dt-task`：sinker wiring（ConnClient + SinkerUtil）。
- `dt-tests`：新增 `gaussdb_oracle_to_oracle` 的 `struct basic` + `check basic` 回归入口。
- docs/scripts：更新 `gaussdb-progress-tracker.md` + `gaussdb-e2e-test-plan.md` + `scripts/e2e/oracle_gaussdboracle_bootstrap.sh`。

## Done-When

- [ ] `SUBTASKS.csv` 全部为 `DONE`
- [ ] `cargo test -p dt-tests --test integration_test -- gaussdb_oracle_to_oracle::struct_tests::test::struct_basic_test gaussdb_oracle_to_oracle::check_tests::test::check_basic_test --nocapture` 通过

