# struct: routine（function/procedure，仅 plpgsql/sql，双向）

## 目标

在现有 PG 结构同步链路基础上，补齐 routine 对象同步：

- function + procedure（统一称 routine）
- 仅支持 `plpgsql` 与 `sql` 语言（其余语言 skip + warn）
- 双向可用：`PG → GaussDBPg` 与 `GaussDBPg → PG`

约束（锁定）：

- router：不改写 routine 定义体内部引用，仅改写 **header** 里的 `schema.name`（必要时同时路由 name）。
- 幂等：保留 `pg_get_functiondef(oid)` 返回的 `CREATE OR REPLACE ...`。
- 执行顺序：schema/table → routine → view/matview → rbac。

## 成功标准

1. 新增 `do_structures=routine` 可控制是否同步 routine。
2. routine 抽取使用 `pg_proc + pg_language`，用 `pg_get_functiondef(oid)` 获取 CREATE 语句。
3. 非 `plpgsql/sql` 的 routine 会被跳过并打印 warn（包含 schema/name/language）。
4. dt-tests（双向）能验证：
   - 目标端 routine 存在
   - 目标端至少一个 function 可 `SELECT func()` 成功
   - procedure 可 `CALL` 成功（如目标端版本支持）

## 验收命令（最小）

```bash
cargo test -p dt-common -p dt-connector
# e2e/集成（需要本地终端环境可连数据库）：
cargo test -p dt-tests --test integration_test -- pg_to_gaussdb::struct_tests::test::struct_view_routine_test --nocapture
cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::struct_tests::test::struct_view_routine_test --nocapture
```

