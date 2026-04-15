# Epic Specification

## Goal

- 交付 `Oracle ↔ GaussDBOracle` 的**双向链路（bootstrap）**：至少支持 `snapshot` 级别的全量迁移在两端可跑通，并提供 `dt-tests` 自动化回归入口。

## Non-Goals

- 本 Epic **不承诺** Oracle 侧 CDC（LogMiner/OGG 等）与双活防环（DataMarker 拓扑）的完整实现。
- 本 Epic **不承诺** Oracle 侧 Struct 同步的完整覆盖（视图/序列/存储过程等）。

## Constraints

- Oracle 环境：必须使用本机 Docker 镜像 `wnameless/oracle-xe-11g-r2:latest`（compose 已在 `dt-tests/docker-compose.oracle_xe.yml`）。
- GaussDBOracle 环境：沿用既有 GaussDB pg-wire 环境；远端 oracle-mode `testdb` 用于验证（通过本地 `dt-tests/tests/.env.local` 注入，禁止提交凭据）。
- 以 “可跑通 + 可回归（dt-tests 证据）” 为 bootstrap 验收边界；更完整能力留作后续增强 Epic。

## Risk Assessment

- Oracle 连接栈：仓库当前无 OCI/JDBC 驱动依赖，短期可能需要走 CLI/sqlplus 方案来打通 bootstrap（风险：能力受限、类型/转义覆盖不足）。
- 标识符大小写：Oracle 默认大写折叠，需通过 router `col_map`/命名约束消解与 PG/GaussDB 的小写差异。
- 环境稳定性：Oracle XE 启动慢且依赖健康检查；需要 compose 内置 init/unblock 逻辑避免 flaky。

## Child Deliverables

- `DbType::Oracle` + 最小可用 Oracle Snapshot Extractor / Oracle Write Sinker（用于 `snapshot` 双向链路）。
- `dt-tests`：新增 `oracle_to_gaussdb_oracle` 与 `gaussdb_oracle_to_oracle` 的 snapshot smoke，用 Oracle XE + 远端 GaussDBOracle 进行回归。
- 文档与 tracker：补齐入口与证据索引（避免后续上下文丢失）。

## Done-When

- [x] `SUBTASKS.csv` 全部为 `DONE`
- [x] `cargo test -p dt-tests --test integration_test -- oracle_to_gaussdb_oracle::snapshot_tests::test::smoke_test --nocapture` 通过
- [x] `cargo test -p dt-tests --test integration_test -- gaussdb_oracle_to_oracle::snapshot_tests::test::smoke_test --nocapture` 通过
