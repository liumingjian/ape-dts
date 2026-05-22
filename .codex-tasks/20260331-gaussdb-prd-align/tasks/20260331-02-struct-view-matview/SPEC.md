# struct: view+matview（双向）

## 目标

在现有 `PgStructExtractor/PgStructFetcher` 的基础上补齐对象同步：

- 普通视图（view）
- 物化视图（matview，`WITH NO DATA`）

要求：

- 双向可用：`PG → GaussDBPg` 与 `GaussDBPg → PG`
- 幂等：view 用 `CREATE OR REPLACE`；matview 用 `DO ... EXCEPTION ...` 在目标已存在时跳过
- router：只路由对象 header（schema/name），不改写定义体内部引用
- 过滤：通过 `do_structures` 支持 `view`

## 成功标准

1. 新增 `do_structures=view` 后，struct 任务能在目标端创建 view/matview。
2. `dt-tests` 新增双向 struct 用例通过（至少验证：目标端对象存在且可查询）。
3. 真实环境联调证据（脱敏）落在本任务 `raw/`（可后续补充）。

## 验收命令（最小）

```bash
cargo test -p dt-tests --test integration_test -- pg_to_gaussdb::struct_tests::test::struct_view_routine_test --nocapture
cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::struct_tests::test::struct_view_routine_test --nocapture
```

