# struct: routine grants（EXECUTE）

## 目标

在现有 `PgCreateRbacStatement.privileges` 基础上补齐 routine grants：

- 从源端 `pg_proc.proacl` 解析 `EXECUTE` 权限（`aclexplode`）
- 生成并回放：
  - `GRANT EXECUTE ON FUNCTION schema.name(args) TO role [WITH GRANT OPTION]`
  - `GRANT EXECUTE ON PROCEDURE schema.name(args) TO role [WITH GRANT OPTION]`

约束：

- 不新增公开配置项。
- 保持 fail-fast：RBAC 执行失败仍由 `conflict_policy` 控制（默认 interrupt）。

## 成功标准

1. `PgStructFetcher` 能抽取 routine grants 并并入 privileges。
2. 生成 SQL 使用 identity args（`pg_get_function_identity_arguments(oid)`）确保可唯一定位重载函数。
3. 对 `pg_proc.prokind` 不可用的环境有可控 fallback（尽量推断 procedure，否则按 function 处理）。
4. 单测覆盖：
   - grant SQL 生成（含空参、含参、WITH GRANT OPTION）
   - procedure/function 分型

## 验收（最小）

```bash
cargo test -p dt-common -p dt-connector
```

