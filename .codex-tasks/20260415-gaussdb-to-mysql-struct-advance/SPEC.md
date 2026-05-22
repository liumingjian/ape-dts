# Task Specification

## Task Shape

- **Shape**: `single-full`

## Goals

- 扩展 `GaussDBPg -> MySQL` 的 struct 转换能力（在已有 `PgCreateSchema/PgCreateTable` 基础上）：
  - 支持常见 column default（数值/字符串/布尔/`now()`）的 MySQL 侧落盘
  - 支持 identity / `nextval(...)` 风格默认值到 MySQL `AUTO_INCREMENT` 的最小映射（仅整数列）
  - 支持简单 btree index（普通/unique）的 MySQL 侧落盘（从 PG `pg_indexes.indexdef` 解析列名）
- 新增更强覆盖的 `dt-tests` 用例（struct advanced），覆盖：
  - 复合主键
  - unique index + normal index
  - default 值映射（数字/字符串/布尔/时间）
- 保持对现有 `mysql_to_mysql` 等 struct 测试不回归。

## Non-Goals

- 不在本任务内补齐 PG -> MySQL 的完整类型/约束/注释/视图/函数/权限等全量迁移。
- 不支持表达式索引、partial index、非 btree index（遇到则跳过）。
- 不承诺所有 default 表达式都可转换；仅覆盖测试用例所需的最小集合。

## Constraints

- 不提交凭据：连接参数仍只来自 `dt-tests/tests/.env.local` / `dt-tests/tests/.env`。
- 任何无法安全转换的对象必须显式跳过，避免在 struct 流程中硬失败污染环境。

## Done-When

- `cargo test -p dt-tests --test integration_test -- gaussdb_to_mysql::struct_tests::test::struct_advanced_test --nocapture` 在真实环境 PASS
- 编译验证通过：
  - `cargo test -p dt-connector --no-run`
  - `cargo test -p dt-tests --test integration_test --no-run`

