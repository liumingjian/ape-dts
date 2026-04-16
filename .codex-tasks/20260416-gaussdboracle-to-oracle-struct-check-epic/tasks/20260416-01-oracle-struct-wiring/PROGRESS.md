# Progress

- 完成内容：
  - `dt-common`：新增 `SinkerConfig::OracleStruct`，并允许 `DbType::Oracle + sink_type=struct` 解析。
  - `dt-connector`：新增 `OracleStructSinker`（从 `PgCreateTableStatement` 生成 Oracle DDL，并显式失败不支持结构/类型）。
  - `dt-task`：wiring（`ConnClient`/`SinkerUtil`）支持创建 `OracleStructSinker`。
- 验证：
  - `cargo test -p dt-task --no-run` PASS
