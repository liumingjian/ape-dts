# GaussDB Oracle 兼容模式路线图（Blocked）

> 状态：`BLOCKED`

当前阶段不启动 `GaussDBOracle` 主实现，原因：

- 仓库内尚无 Oracle 兼容 connector 家族可直接复用
- 与 `GaussDBMySQL` 相比，`GaussDBOracle` 需要更大的新建面：sinker / query builder / struct / precheck / dt-tests
- 本轮资源优先给到 `GaussDBMySQL Bootstrap` 与 `GaussDBPg Quality Coverage`

激活条件：

1. `GaussDBMySQL` 首波完成并稳定
2. 明确可写的 GaussDB Oracle 兼容环境与测试账号
3. 锁定第一阶段交付边界（建议从 target-first、non-CDC 开始）

建议首个 spec 边界：

- 只做 `Oracle -> GaussDBOracle` target-first
- 先到 `DbType + route + smoke`
- CDC 与源端抽取后置
