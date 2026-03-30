# 子任务 1：基础兼容层接入（single-full）

## Goal

- 在 `dt-common` 中新增 `DbType::GaussDBPg`（解析值：`gaussdb_pg`）
- 在配置、工厂、预检查、公共工具链中补齐 `GaussDBPg` 分支，让非 CDC 场景可复用现有 Pg 路径
- Precheck：沿用 PostgreSQL 预检查骨架，但允许 GaussDB 独立版本判断与系统 schema 过滤

## Non-Goals

- 本子任务不实现 GaussDB CDC Extractor（见子任务 3）
- 本子任务不做 SHA256 认证支持

## Done-When

- `gaussdb_pg` 能正确解析为 `DbType::GaussDBPg`
- 非 CDC 场景下 GaussDB 不会因缺少 `DbType` 分支在启动阶段失败
- 预检查可输出 GaussDB 版本与 CDC 兼容性相关结果

