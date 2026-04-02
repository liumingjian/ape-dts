# GaussDBMySQL Bootstrap（Epic）

## Goal

以 `MySQL -> GaussDBMySQL` **目标端优先** 为边界，完成 `DbType::GaussDBMySQL` 的首波落地：

1. 对齐 docs / tracker / 环境契约。
2. 打通 `gaussdb_mysql` 的配置解析、路由与 smoke 测试入口。
3. 后续在本 epic 内继续推进 `snapshot basic`、`struct + check basic`。

## Non-Goals

- 不做 CDC。
- 不做 `GaussDBMySQL -> MySQL` 源端抽取。
- 不与 `GaussDBOracle` 混做。

## Constraints / Decisions

- 源端 MySQL 使用本机 Docker。
- 目标端使用现有可写的 GaussDB MySQL 兼容实例。
- 新确认的环境事实：
  - GaussDB 兼容模式在建库时指定，属于数据库级属性。
  - 当前样例库 `jyp_test_m` 通过 `postgres://root@10.250.0.51:8000/jyp_test_m?sslmode=require` 接入，`SHOW sql_compatibility;` 返回 `M`。
  - 因此 `GaussDBMySQL` 的实现不能直接等价为 “MySQL wire protocol”；需要区分连接协议与 SQL 兼容模式。
- 真实环境变量默认通过 `dt-tests/tests/.env.local` 提供。

## Done-When

- `SUBTASKS.csv` 中所有 active child 均为 `DONE`，且每个 child 都有自动化验证与真实环境证据。
