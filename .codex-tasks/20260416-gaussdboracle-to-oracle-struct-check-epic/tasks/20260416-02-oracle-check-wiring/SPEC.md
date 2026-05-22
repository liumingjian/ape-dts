# SPEC — Oracle Check Wiring

## Goal

让 `DbType::Oracle` 支持 `sink_type=check`：

- `dt-common` 增加 `SinkerConfig::OracleCheck` 并可从 `task_config.ini` 解析出来。
- `dt-connector` 实现 `OracleChecker`（对账 basic：按主键拉取目标端行并对比）。
- `dt-task` wiring：`SinkerUtil` 能创建 OracleChecker。

## Done-When

- `cargo test -p dt-task --no-run` 通过

