# Progress

- 完成内容：
  - `dt-common`：新增 `SinkerConfig::OracleCheck`，并允许 `DbType::Oracle + sink_type=check` 解析。
  - `dt-connector`：
    - `CheckerTbMeta` 增加 `Oracle` 变体，revise sql 复用 `OracleSinker` DML builder。
    - 新增 `OracleChecker`：用 `sqlplus` 查询目标端行并按源端 `ColValue` 类型解析，避免类型不一致导致误报 diff。
  - `dt-task`：
    - `SinkerUtil` 支持创建 `OracleChecker`。
    - `TaskRunner::init_log4rs` 支持 `OracleCheck`（check logger 目录替换）。
- 验证：
  - `cargo test -p dt-task --no-run` PASS
