# DbType::GaussDBMySQL 骨架 + 路由 + smoke

目标：让 `db_type=gaussdb_mysql` 在配置解析、连接/路由、precheck 与 `dt-tests` smoke 测试入口中成为可用的一等类型。

新增环境约束（2026-04-02）：

- GaussDB 兼容模式在建库时指定，属于数据库级属性。
- 当前样例库 `jyp_test_m` 通过 `postgres://root@10.250.0.51:8000/jyp_test_m?sslmode=require` 接入，`SHOW sql_compatibility;` 返回 `M`。
- 因此本 child 的后续实现需要澄清：`gaussdb_mysql` 表示的是 SQL 兼容模式、连接协议，还是两者的组合；当前代码仅完成了“按 MySQL 兼容连接器复用”的第一版骨架。
