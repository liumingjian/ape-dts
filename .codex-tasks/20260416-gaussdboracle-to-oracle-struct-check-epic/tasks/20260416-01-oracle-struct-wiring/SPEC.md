# SPEC — Oracle Struct Wiring

## Goal

让 `DbType::Oracle` 支持 `sink_type=struct`：

- `dt-common` 增加 `SinkerConfig::OracleStruct` 并可从 `task_config.ini` 解析出来。
- `dt-task` wiring：`ConnClient` 能创建 `OracleSqlPlusClient`，`SinkerUtil` 能创建 `OracleStructSinker`。
- `dt-connector` 实现 `OracleStructSinker`（支持 basic table + primary key + 常见列类型）。

## Done-When

- `cargo test -p dt-task --no-run` 通过

