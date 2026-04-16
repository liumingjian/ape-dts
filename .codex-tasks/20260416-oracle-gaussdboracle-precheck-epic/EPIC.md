# Epic Specification

## Goal

- 补齐 `Oracle ↔ GaussDBOracle`（bootstrap）在能力矩阵中的 **precheck**：
  - `dt-precheck` 新增 `DbType::Oracle` 的 prechecker（基于 `sqlplus`/docker exec）。
  - `dt-tests` 新增两条回归入口：
    - `oracle_to_gaussdb_oracle::precheck_tests::test::struct_supported_basic_test`
    - `gaussdb_oracle_to_oracle::precheck_tests::test::struct_supported_basic_test`
  - 同步更新 tracker/e2e-plan 的入口索引与证据路径。

## Non-Goals

- 不实现 Oracle 的 `struct/check` 模块（仍保持 bootstrap 范围）。
- 不实现 redo/logminer/OGG 级 CDC（现阶段 Oracle CDC 仍为 trigger-based bootstrap）。
- 不引入 OCI/JDBC 依赖；Oracle 交互继续走 `sqlplus` CLI。

## Constraints

- Oracle 环境必须使用本机 Docker 镜像：`wnameless/oracle-xe-11g-r2:latest`（容器：`oracle-xe-local`）。
- GaussDBOracle 环境沿用既有 pg-wire oracle-mode `testdb`（通过本地 `dt-tests/tests/.env.local` 注入，禁止提交凭据）。
- Debug-First：precheck 不做静默降级；失败必须可复现并给出明确错误信息。

## Child Deliverables

- `dt-precheck`：
  - 新增 `OracleFetcher` + `OraclePrechecker`，并在 `PrecheckerBuilder` 中接入 `DbType::Oracle`（source/sink）。
- `dt-tests`：
  - `PrecheckTestRunner` 支持 `DbType::Oracle` 的 prepare/clean。
  - 新增 Oracle ↔ GaussDBOracle 的 precheck basic testcase 目录与集成测试入口。
- Docs：
  - `docs/agent-summary/gaussdb-progress-tracker.md` 更新能力矩阵与证据入口。
  - `docs/agent-summary/gaussdb-e2e-test-plan.md` 补齐 quick 回归命令。

## Done-When

- [x] `SUBTASKS.csv` 全部为 `DONE`
- [x] `cargo test -p dt-precheck --no-run` 通过
- [x] `cargo test -p dt-tests --test integration_test -- oracle_to_gaussdb_oracle::precheck_tests::test::struct_supported_basic_test --nocapture` 通过
- [x] `cargo test -p dt-tests --test integration_test -- gaussdb_oracle_to_oracle::precheck_tests::test::struct_supported_basic_test --nocapture` 通过

