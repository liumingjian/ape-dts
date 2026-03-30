# GaussDB PG 兼容模式 MVP（Epic）

## Goal

- 在 `ape-dts` 中新增 `DbType::GaussDBPg` 并完成主仓配置、路由、预检查、测试链路接入
- 复用现有 PostgreSQL（Pg）实现打通：
  - `PG -> GaussDB`：snapshot / struct / check
  - `GaussDB -> PG`：snapshot / cdc / check（CDC 基于 `mppdb_decoding`）
- MVP 只支持 `MD5` 认证主路径（由 GaussDB 侧配置 `password_encryption_type=1` 保障）

## Non-Goals

- 不做 `GaussDBMySQL`、`GaussDBOracle`
- 不把 `SHA256` 认证作为 MVP 验收前置条件（仅预留封装点 + 路线说明）
- 不扩展超出现有 PG 结构链路能力的对象同步（不做 view/function/trigger/custom type 等）
- 不做 GaussDB 分布策略映射配置
- 不把专网 GaussDB 联调接入公共 CI

## Constraints

- Rust workspace：尽量复用现有 Pg 代码路径，避免复制粘贴
- CDC：不得依赖 `publication + pgoutput`，需新建 GaussDB CDC Extractor（`mppdb_decoding` / JSON）
- 允许缺少真实 GaussDB 环境时完成编译/单测；真实联调作为人工门禁

## Risk Assessment

- `mppdb_decoding` JSON 格式与文档示例不一致：优先扩展解码兼容层，避免改动下游数据模型
- GaussDB 版本号/系统 schema 与 PostgreSQL 不同：precheck + system schema 过滤需单独处理
- 结构同步能力边界误解：计划/测试/文档三处一致声明

## Child Deliverables

- 子任务 1：基础兼容层接入
- 子任务 2：PG 兼容数据面打通（snapshot/struct/check 复用）
- 子任务 3：GaussDB CDC Extractor 新建（mppdb_decoding / JSON）
- 子任务 4：测试与联调 Harness（集成测试骨架 + 证据归档点）
- 子任务 5：文档收口与 SHA256 后续路线

## Dependency Notes

- 子任务 2、3 依赖子任务 1
- 子任务 4 依赖子任务 2、3
- 子任务 5 依赖子任务 4

## Done-When

- [ ] `SUBTASKS.csv` 全部为 `DONE`
- [ ] `cargo test --workspace --all-targets --no-run` 通过

