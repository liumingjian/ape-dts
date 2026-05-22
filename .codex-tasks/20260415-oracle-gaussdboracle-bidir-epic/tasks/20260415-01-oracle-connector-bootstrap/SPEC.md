# SPEC — Oracle Connector Bootstrap (Snapshot/Write)

## Goal

在现有 `ape-dts` 框架内新增 `DbType::Oracle`，并交付可运行的 **bootstrap 级** Oracle 连接能力：

- Oracle 作为 **源端**：支持 `extract_type=snapshot` 读取表数据并产出 `RowData::Insert`。
- Oracle 作为 **目标端**：支持 `sink_type=write` 将 `RowData::Insert` 写入 Oracle（满足 snapshot 迁移）。

用于后续 `Oracle -> GaussDBOracle` 与 `GaussDBOracle -> Oracle` 的 dt-tests 链路回归。

## Scope

- `DbType::Oracle` 枚举值 + 配置解析/路由接入
- `ExtractorConfig::OracleSnapshot`
- `SinkerConfig::Oracle`（write）
- `ConnClient` 支持 Oracle（非 sqlx 连接池，采用 CLI/sqlplus 执行）
- `TaskUtil::list_schemas/list_tbs` 对 Oracle 的最小实现（满足按 filter 指定的表可跑通）
- `dt-connector` 新增 Oracle snapshot extractor + oracle sinker

## Constraints / Assumptions

- 短期以 **sqlplus CLI** 作为连接手段（不引入 OCI/JDBC 重依赖）；在测试环境通过
  `ORACLE_SQLPLUS_DOCKER_CONTAINER=oracle-xe-local` 走 `docker exec` 执行 sqlplus。
- bootstrap 先支持 `NUMBER` / `VARCHAR2` 等基础类型；复杂类型后续扩展。
- snapshot 阶段仅处理 `INSERT`，不覆盖 update/delete（CDC 后续再做）。

## Acceptance Criteria

- `cargo test -p dt-common --no-run` 通过
- `cargo test -p dt-connector --no-run` 通过
- `cargo test -p dt-task --no-run` 通过
- 能通过一个最小 task_config（Oracle snapshot -> Pg/GaussDB sink）跑通并产生写入（在 dt-tests 子任务中验证）

