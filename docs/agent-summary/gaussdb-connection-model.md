# GaussDB 连接模型说明

> 最后更新：2026-04-02

## 1. 背景

在当前 HCS 环境中，GaussDB 的兼容模式是在**建库时**指定的，而不是简单由连接协议决定。

已验证样例：

- endpoint: `postgres://root@10.250.0.51:8000/jyp_test_m?sslmode=require`
- `current_database()` = `jyp_test_m`
- `SHOW sql_compatibility;` = `M`

这说明：

- `wire protocol` 可以是 `PostgreSQL`
- `SQL compatibility mode` 可以是 `MySQL-compatible`

因此，不能再把 `DbType::GaussDBMySQL` 简化理解为 “一定通过 `mysql://` 访问的库”。

## 2. 三个层次

### 2.1 连接协议（Wire Protocol）

决定客户端驱动和连接池类型，例如：

- `mysql://...` -> MySQL protocol
- `postgres://...` -> PostgreSQL protocol

### 2.2 数据库兼容模式（SQL Compatibility Mode）

决定语义、系统视图、DDL/DML 差异，例如：

- `P` -> PostgreSQL-compatible
- `M` -> MySQL-compatible
- `A` -> Oracle-compatible

### 2.3 任务语义（DbType）

`DbType` 仍然代表 ape-dts 在该任务中的行为语义，例如：

- `gaussdb_pg`
- `gaussdb_mysql`
- 未来的 `gaussdb_oracle`

但 `DbType` 不应再单独承担 “连接协议” 的含义。

## 3. 设计结论

### 3.1 必须解耦

后续实现中至少要显式区分：

- `DbType`
- `wire_protocol`
- `sql_compatibility`

### 3.2 对 `GaussDBMySQL` 的影响

`GaussDBMySQL` 的首波实现不能继续沿用“直接复用 MySQL connector path”这一单一假设。

正确方向应是：

1. 先根据 URL 或显式配置确定 `wire_protocol`
2. 再根据 `db_type` / 目标环境事实确定 `sql_compatibility`
3. 由二者共同决定：
   - 使用哪种连接池
   - 使用哪套元数据抓取
   - 使用哪套 SQL 生成与对象同步逻辑

### 3.3 对 `GaussDBOracle` 的意义

未来 `GaussDBOracle` 也可能复用同一套接入 endpoint，但落到不同的数据库兼容模式。因此本模型不是只为 `M` 模式服务，而是 `P/M/A` 的统一基础。

## 4. 环境变量命名建议

长期建议采用“端点身份优先”的命名，而不是“当前任务角色优先”的命名。

例如：

```dotenv
gaussdb_endpoint_without_auth_url=postgres://10.250.0.51:8000/jyp_test_m?sslmode=require
gaussdb_endpoint_username=root
gaussdb_endpoint_password=***
gaussdb_sql_compatibility=M
```

然后由测试/配置层再映射到 extractor/sinker 角色。

## 5. 当前状态

- 该模型已作为 `GaussDBMySQL Bootstrap` Epic 的活动修正路径。
- 原 `gaussdb_mysql == MySQL wire protocol` 第一版探索已被显式标记为失败。
- 下一步是在代码中补最小抽象，再回到 `snapshot/struct/check` 的真实联调。
