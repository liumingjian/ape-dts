# 子任务 5：文档收口与 SHA256 后续路线（single-full）

## Goal

- 文档可独立指导工程师完成 GaussDB PG 兼容模式的环境准备与联调：
  - `PG -> GaussDB`：snapshot / struct / check
  - `GaussDB -> PG`：snapshot / cdc / check
- 提供 GaussDB 配置模板（`docs/templates/`）与测试环境变量说明（`dt-tests/tests/.env(.local)`）
- 形成可执行的 SHA256 认证后续路线（边界清晰、与 MVP 不相互阻塞）

## Non-Goals

- 不在本子任务中实现/交付 SHA256 认证能力（仅提供路线与封装点建议）
- 不把真实 GaussDB 联调接入公共 CI

## Done-When

- `docs/templates/` 存在 GaussDB 相关配置模板
- `docs/agent-summary/` 提供联调 runbook + 常见错误排查 + 证据归档建议
- SHA256 路线说明清晰：独立仓分支验证、回切方式、主仓最小改动点

