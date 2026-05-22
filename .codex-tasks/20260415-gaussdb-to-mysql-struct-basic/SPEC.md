# Task Specification

## Task Shape

- **Shape**: `single-full`

## Goals

- 支持 `GaussDBPg -> MySQL` 的 **struct basic**：至少能把源端 `PgCreateSchema/PgCreateTable` 转成 MySQL DDL 并落到目标端。
- 新增 `dt-tests` 用例：`gaussdb_to_mysql::struct_tests::test::struct_basic_test`，在真实环境中验证可运行。
- 验证通过后更新 tracker / e2e matrix，将 `GaussDBPg -> MySQL` 的 `struct` 从 `-` 关闭为 ✅。

## Non-Goals

- 不在本任务内补齐完整的 PG -> MySQL 类型映射、索引/约束/注释/视图/函数等全量对象迁移。
- 不承诺 struct 的“完全等价”，本任务只做 bootstrap 级别的最小可运行闭环。

## Constraints

- 不提交凭据：连接参数仅来自 `dt-tests/tests/.env.local`。
- 结构同步对 MySQL 的支持必须不破坏现有 `mysql_to_mysql` 等 struct 测试。

## Done-When

- `cargo test -p dt-tests --test integration_test -- gaussdb_to_mysql::struct_tests::test::struct_basic_test --nocapture` 在真实环境 PASS
- 文档状态同步到 `docs/agent-summary/gaussdb-progress-tracker.md` / `docs/agent-summary/gaussdb-e2e-test-plan.md`

