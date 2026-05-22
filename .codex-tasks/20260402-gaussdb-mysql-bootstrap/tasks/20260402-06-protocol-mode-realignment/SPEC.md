# GaussDB 协议与兼容模式解耦

目标：修正 `GaussDBMySQL` 首波实现的根假设，把“连接协议（wire protocol）”与“数据库兼容模式（SQL compatibility mode）”拆开建模，为后续 `GaussDBMySQL / GaussDBOracle` 提供统一的连接层基础。

已确认环境事实：

- GaussDB 兼容模式在建库时指定，属于数据库级属性。
- 当前样例库 `jyp_test_m` 使用 `postgres://root@10.250.0.51:8000/jyp_test_m?sslmode=require` 接入。
- 连接后 `SHOW sql_compatibility;` 返回 `M`。

本 child 的交付边界：

- 更新 Epic/docs，显式记录原假设失效的原因。
- 新增一份连接模型设计文档，明确：
  - `DbType`
  - `wire protocol`
  - `sql_compatibility`
  - env naming contract
- 在代码里补最小基础抽象与单测，至少能表达：
  - `postgres://.../jyp_test_m` 属于 `PostgreSQL` wire protocol
  - `sql_compatibility=M` 属于 `GaussDB MySQL-compatible mode`

非目标：

- 本 child 不直接交付 `MySQL -> GaussDBMySQL` smoke PASS。
- 本 child 不直接完成 `GaussDBOracle` 支持，只为其预留正确模型。
