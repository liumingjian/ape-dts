# GaussDBPg Quality Coverage（Epic）

## Goal

围绕 `DbType::GaussDBPg` 的已上线主链路，继续收口 PRD 中尚未形成执行真表的质量缺口：

1. 统一真相源，去掉过时路线描述。
2. 推进类型矩阵 / codec / compare 兼容。
3. 补齐非 CDC / CDC 的类型矩阵 e2e。
4. 增加性能 / 可观测 / check 边界的质量门槛。

## Non-Goals

- 不在本 epic 内推进 SHA256。
- 不在本 epic 内扩展到 `GaussDBMySQL` 或 `GaussDBOracle`。

## Done-When

- `SUBTASKS.csv` 中所有 child 为 `DONE`，并且每个 child 都带有自动化验证与真实环境证据入口。
