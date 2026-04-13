# GaussDB PRD 驱动迭代计划

> **日期**：2026-04-02  
> **需求真相源**：`docs/agent-summary/gaussdb-prd.md`  
> **执行真表（Epic）**：`.codex-tasks/20260331-gaussdb-prd-align/SUBTASKS.csv`

本文件从 2026-03-31 起演进为“PRD 驱动的迭代计划”。2026-03 的 MVP 收敛计划已完成，保留在文末附录用于溯源。

## 1. 当前状态（已完成）

MVP（`DbType::GaussDBPg`）已在真实环境闭环验证：

- `PG → GaussDBPg`：snapshot / struct / check ✅
- `PG → GaussDBPg`：cdc ✅
- `GaussDBPg → PG`：snapshot / cdc / check ✅
- Struct 扩展（view/matview/routine/routine grants，双向）✅
- CDC P0 稳定性增强：candidate-first + sticky + HA 端口 `sql_port+1` + replication NoTLS-first + fail-fast 诊断 ✅
- CDC P1 resilience：resume / failover / negatives / e2e ✅

证据入口（示例）：

- `.codex-tasks/20260316-gaussdb-mvp/SUBTASKS.csv`
- `.codex-tasks/20260329-gaussdb-prd-e2e/PROGRESS.md`
- `.codex-tasks/20260331-gaussdb-p0-stability/PROGRESS.md`
- `.codex-tasks/20260331-gaussdb-cdc-resilience/PROGRESS.md`

## 2. 下一阶段（双 Epic 并行）

### 2.1 Active Epic A：`GaussDBMySQL CDC Expansion`

目标：在 `GaussDBMySQL Bootstrap` 已闭环的基础上，继续推进
**`MySQL -> GaussDBMySQL`** 的下一阶段能力，优先补齐 **CDC basic**。

本轮范围锁定：

- 只做 `MySQL -> GaussDBMySQL`
- 首波 bootstrap 已交付到 `snapshot + struct + check + precheck + docs`
- 当前下一阶段从 `cdc basic` 开始
- **不做 `GaussDBMySQL -> MySQL` 源端抽取**

环境约束：

- `mysql` 源端统一使用本机 Docker
- `gaussdb` 目标端使用现有可写的 **MySQL 兼容模式**实例
- 新发现的环境事实：
  - GaussDB 兼容模式是在**建库时**指定的，属于数据库级属性
  - 当前已验证示例：`postgres://root@10.250.0.51:8000/jyp_test_m?sslmode=require`
  - 连接到该库后 `SHOW sql_compatibility;` 返回 `M`
  - 这说明 `GaussDBMySQL` 不能简单等同于 “MySQL wire protocol”；后续实现需要拆分“连接协议”和“SQL 兼容模式”
- 当前已验证能力：
  - `MySQL -> GaussDBMySQL snapshot basic` 已通过真实环境验证
  - `MySQL -> GaussDBMySQL struct basic` 已通过真实环境验证
  - `MySQL -> GaussDBMySQL check basic` 已通过真实环境验证
  - `MySQL -> GaussDBMySQL precheck` 已通过真实环境验证
  - 关键适配包括 pg-wire MySQL-mode 写入、目标 simple-query 对账、以及候选主库 RW 重写
- 当前第二阶段执行策略：
  - child 1：`cdc basic`
  - child 2：`cdc type-matrix`
  - child 3：`cdc resilience + negatives`
  - child 4：`docs/runbook/tracker` 收口
  - 当前进度（2026-04-13）：child 1/2/3 已完成，待收口 docs/tracker
- 推荐通过 `dt-tests/tests/.env.local` 提供：
  - `gaussdb_mysql_sinker_without_auth_url`
  - `gaussdb_mysql_sinker_username`
  - `gaussdb_mysql_sinker_password`
  - 这些变量名仍是测试框架里的历史角色命名，但其值在真实环境中可以是 `postgres://.../<mysql-compatible-db>` 形式

执行真表：

- `.codex-tasks/20260410-gaussdb-mysql-cdc-expansion/SUBTASKS.csv`

### 2.2 Active Epic B：`GaussDBPg Quality Coverage`

目标：继续围绕 `DbType::GaussDBPg` 收口 PRD 中尚未形成执行真表的质量项。

本轮优先级锁定：

- 真相源归一（去掉已过时的“CDC resilience 仍属下一阶段”的表述）
- 已完成首批 `gaussdb_pg` 特有类型 alias/codec 契约（`smalldatetime/tinyint/nvarchar2/clob/blob`）
- 类型矩阵 / codec / compare 兼容
- 非 CDC 类型矩阵 e2e
- CDC 类型矩阵 + fail-fast 证据
- 性能 / 可观测 / check 边界深化

执行真表：

- `.codex-tasks/20260402-gaussdbpg-quality-coverage/SUBTASKS.csv`

### 2.3 Blocked Backlog

- `SHA256`
  - 继续单独阻塞管理，不进入本轮 active Epic
  - 依赖 `apecloud/rust-postgres` 与专用联调环境
  - 参考：`docs/agent-summary/gaussdb-sha256-roadmap.md`
- `GaussDBOracle`
  - 本轮只保留 roadmap / blocked 条目
  - 不与 `GaussDBMySQL` 首波骨架混做
  - 参考：`docs/agent-summary/gaussdb-oracle-roadmap.md`

---

## 附录A：GaussDB MVP 实施计划（历史，已完成）

## 1. 计划摘要

本计划基于以下事实制定：

- 当前 `ape-dts` 的 PostgreSQL 路径已经覆盖快照、CDC、结构同步、数据校验、预检查和集成测试骨架。
- 当前 PostgreSQL CDC 实现强依赖 `publication + pgoutput`，不能直接复用到 GaussDB。
- 当前 PostgreSQL 结构同步的真实能力集中在 `schema/table/sequence/comment/index/constraint/rbac`，尚未覆盖 PRD 列出的全部对象类型。
- 当前连接认证配置仅支持用户名/密码注入，不支持显式声明认证协议。

本期目标收敛为 GaussDB PG 兼容模式 MVP：

- 打通 `PG -> GaussDB` 的快照迁移、结构同步、数据校验。
- 打通 `GaussDB -> PG` 的快照迁移、CDC 实时同步、数据校验。
- 只支持 `DbType::GaussDBPg`。
- 采用 `MD5` 认证作为 MVP 主路径。
- `SHA256` 认证纳入总路线，但不作为 MVP 验收前置条件。

## 2. 交付边界

### 2.1 本期必须交付

- 新增 `DbType::GaussDBPg`，并完成主仓配置、路由、预检查、测试链路接入。
- 让 GaussDB 在非 CDC 场景下复用现有 PostgreSQL 实现路径。
- 新建 `GaussDB CDC Extractor`，基于 `mppdb_decoding` 拉取并解析 JSON 事件。
- 完成自动化测试与真实 GaussDB 手工联调证据归档。
- 形成 GaussDB 配置模板、联调 runbook、故障排查说明。

### 2.2 本期明确不做（历史 MVP 边界）

- 不做 `GaussDBMySQL`、`GaussDBOracle`。
- 不做 `SHA256` 认证正式交付。
- 不做 `view/function/trigger/custom type` 等超出现有 PG 结构链路能力的对象同步。
- 不做 GaussDB 分布策略映射配置。
- 不把专网 GaussDB 联调接入公共 CI。

## 3. 执行形态与 Harness 规则

### 3.1 任务形态

采用 `taskmaster` 的 `Epic Task` 形态。实施开始时创建：

- `.codex-tasks/<gaussdb-mvp>/EPIC.md`
- `.codex-tasks/<gaussdb-mvp>/SUBTASKS.csv`
- `.codex-tasks/<gaussdb-mvp>/PROGRESS.md`

子任务按依赖顺序拆成 5 个：

1. 基础兼容层接入
2. PG 兼容数据面打通
3. GaussDB CDC Extractor 新建
4. 测试与联调 harness
5. 文档收口与 SHA256 后续路线

### 3.2 权限分级

- `L1`：只读分析代码、文档、样本和现有测试。
- `L2`：运行单测、静态检查、解析器测试、配置测试。
- `L3`：修改主仓代码并执行自动化回归。
- `L4`：连接真实 GaussDB，执行手工联调和证据采集。

### 3.3 自主预算与求助阈值

- 单个子任务允许最多 20 分钟自主探索，或最多 2 次同类失败。
- 任一条件满足则主动求助：
  - 同类错误连续 2 次且模式一致。
  - 缺少真实样本、插件权限或环境参数。
  - 需要越过安全边界或访问外部受限资源。
  - 出现明显多解且代价差异显著。
- 求助时必须提供：
  - 已尝试动作和证据。
  - 根因假设。
  - A/B/C 方案及成本、风险、时间。

## 4. Epic 子任务拆解

### 子任务 1：基础兼容层接入

目标：让 `GaussDBPg` 在配置、工厂、预检查、元数据工具链中成为一等 `DbType`。

实施内容：

- 在 `dt-common/src/config/` 中新增 `DbType::GaussDBPg`。
- 在 `task_config` 中让 GaussDB 的非 CDC 场景走现有 PG 配置分支。
- 在 `dt-task/src/` 中补齐 GaussDB 的 extractor/sinker/conn pool/list schema/list table 路由。
- 在 `dt-precheck/src/` 中让 `GaussDBPg` 走 PostgreSQL 预检查骨架，但允许独立版本判断和系统 schema 过滤。
- 在 `system_dbs`、`sql_util`、`rdb_query_builder`、结构约束映射等公共组件中补齐 `GaussDBPg` 分支。

验收标准：

- `gaussdb_pg` 能正确解析为配置对象。
- 非 CDC 场景下，GaussDB 不会因为缺少 `DbType` 分支而在启动阶段失败。
- 预检查能输出可理解的 GaussDB 版本和 CDC 兼容性结果。

验证方式：

- 配置解析单测。
- `DbType` 分支和公共工具函数单测。
- 预检查相关单测。

### 子任务 2：PG 兼容数据面打通

目标：在不复制 PostgreSQL 实现的前提下，让 GaussDB 复用现有快照、结构同步、校验路径。

实施内容：

- `PG -> GaussDB` 快照写入直接复用 `PgSinker`。
- `PG -> GaussDB` 结构同步复用 `PgStructExtractor + PgStructSinker`。
- `PG -> GaussDB` 数据校验复用 `PgCheck` 路径。
- 对结构同步增加最小语法适配，确保遇到 GaussDB 特有分布式语法时不阻塞标准 PG 兼容对象迁移。
- 结构同步能力按当前仓库真实能力收敛到：
  - `schema`
  - `table`
  - `sequence`
  - `comment`
  - `index`
  - `constraint`
  - `rbac`

验收标准：

- `PG -> GaussDB` 的 snapshot/struct/check 三条链路都能跑通。
- 结构同步的能力边界在文档中明确，不出现“PRD 写了但实现未覆盖”的静默承诺。

验证方式：

- 新增 `pg_to_gaussdb` 目录下的快照、结构、校验集成测试。
- 真实 GaussDB 环境中执行快照、结构、校验三类手工联调。

### 子任务 3：GaussDB CDC Extractor 新建

目标：实现 `GaussDB -> PG` 的增量 CDC 主路径。

实施内容：

- 在 `dt-connector/src/extractor/gaussdb/` 下新增：
  - `mod.rs`
  - `gaussdb_cdc_client.rs`
  - `gaussdb_cdc_extractor.rs`
  - `gaussdb_json_decoder.rs`
- 在 `dt-common/src/config/extractor_config.rs` 新增 `ExtractorConfig::GaussDBCdc`。
- 在 `dt-task/src/extractor_util.rs` 注册 `GaussDBCdcExtractor`。
- `GaussDBCdcClient` 负责：
  - 复制连接建立
  - `mppdb_decoding` 复制槽创建
  - `START_REPLICATION` 启动
  - keepalive/ack 发送
- `GaussDBJsonDecoder` 负责：
  - `BEGIN`
  - `COMMIT`
  - `INSERT`
  - `UPDATE`
  - `DELETE`
  的 JSON 解析与 `DtData/RowData` 转换。
- 默认复用 `Position::PgCdc` 记录位点，前提是 LSN 与时间戳格式兼容。
- 默认复用 `PgMetaManager`、`PgColValueConvertor`、`PgSinker`，把新增逻辑控制在“复制协议差异”和“JSON 解析差异”两层。

本子任务的明确边界：

- CDC MVP 只覆盖 DML 和事务位点推进。
- CDC MVP 不承诺 GaussDB 原生 DDL 捕获。
- 若 `mppdb_decoding` JSON 格式与 POC 样本不一致，以“解码兼容层扩展”优先，不先改动下游数据模型。

验收标准：

- 能从真实 GaussDB 拉起复制槽并持续消费日志。
- 基本的 insert/update/delete 事件可落成目标 PG 数据。
- 位点可恢复，任务重启后不会从头重放。

验证方式：

- `GaussDBJsonDecoder` 单测。
- `ExtractorConfig::GaussDBCdc` 配置解析单测。
- `gaussdb_to_pg/cdc/basic_test` 集成测试。
- 真实 GaussDB 环境的建槽、拉流、提交、恢复手工联调证据。

### 子任务 4：测试与联调 Harness

目标：把 GaussDB 验证纳入现有测试骨架，而不是临时手工脚本。

实施内容：

- 在 `dt-tests/tests/` 下新增：
  - `pg_to_gaussdb/`
  - `gaussdb_to_pg/`
- 沿用现有 `RdbTestRunner`、`RdbStructTestRunner`、`RdbCheckTestRunner` 骨架。
- 在 `dt-tests/tests/.env` 与 `.env.local` 机制中新增 GaussDB URL 占位变量。
- 为 GaussDB 增加代表性测试场景：
  - snapshot basic
  - struct basic
  - struct rbac 或 filter
  - check basic
  - cdc basic
- 真实 GaussDB 验证采用“自动化回归 + 人工门禁”的双层策略。

验收标准：

- 仓库内自动化测试可以在无专网条件下跑通单测和可模拟集成测试。
- 真实环境联调至少覆盖 4 组证据：
  - `PG -> GaussDB snapshot`
  - `PG -> GaussDB struct/check`
  - `GaussDB -> PG snapshot/check`
  - `GaussDB -> PG cdc`

验证方式：

- `cargo test --package dt-tests --test integration_test ...`
- 真实环境日志、原始 JSON 样本、复制槽信息、比对结果归档。

### 子任务 5：文档收口与 SHA256 后续路线

目标：让 MVP 可交付、可联调、可继续演进到 SHA256。

实施内容：

- 在 `docs/agent-summary/` 和相关模板文档中补充：
  - GaussDB 配置样例
  - 测试环境变量说明
  - 手工联调步骤
  - 常见错误与排查流程
- 为 `apecloud/rust-postgres` 建立独立后续路线：
  - 单独分支实现 SHA256
  - 独立样例程序验证握手
  - 成功后通过 git rev/tag 回切到主仓依赖
- 主仓本期只做“后续可接入”的封装，不让 SHA256 接入再次横切所有模块。

验收标准：

- 文档能独立指导另一个工程师完成环境准备和联调。
- SHA256 路线有清晰的仓库边界、回切方式和非阻塞关系说明。

验证方式：

- 文档 walkthrough。
- 依赖清单和后续事项审查。

## 5. 公共接口与配置变更

本期新增或调整的接口如下：

- 新增 `DbType::GaussDBPg`。
- 新增 `ExtractorConfig::GaussDBCdc`。
- 新增 `dt-connector/src/extractor/gaussdb/` 模块。
- 新增 `dt-tests` 中的 GaussDB 测试目录和环境变量占位。

本期明确不新增的公开配置：

- 不新增 `auth_mode` 公开配置。
- 不新增 GaussDB 分布策略映射配置。
- 不新增新的位置类型，默认继续使用 `Position::PgCdc`。

## 6. 测试矩阵

### 6.1 单元测试

- `DbType::GaussDBPg` 配置解析。
- 系统库过滤与 SQL 转义。
- 预检查版本判断与 CDC 兼容性判断。
- `GaussDBJsonDecoder` 的 `BEGIN/COMMIT/INSERT/UPDATE/DELETE` 解析。

### 6.2 集成测试

- `pg_to_gaussdb/snapshot/basic_test`
- `pg_to_gaussdb/struct/basic_test`
- `pg_to_gaussdb/check/basic_test`
- `gaussdb_to_pg/snapshot/basic_test`
- `gaussdb_to_pg/check/basic_test`
- `gaussdb_to_pg/cdc/basic_test`

### 6.3 手工联调

- 建槽是否成功。
- `mppdb_decoding` 是否可用。
- 原始 JSON 样本是否与解析器兼容。
- LSN 恢复后是否会重复消费或丢事件。
- 目标端数据是否与源端一致。

## 7. 风险与对应策略

### 风险 1：`mppdb_decoding` 输出格式与 POC 不一致

策略：

- 先抓原始样本，再改解析器。
- 优先扩展解码兼容层，不先改通用数据模型。
- 若同类格式问题连续失败 2 次，立即升级为人工决策点。

### 风险 2：GaussDB 版本号或系统 schema 与 PostgreSQL 不同

策略：

- 在 precheck 和 system db 过滤中单独处理 `GaussDBPg`。
- 不把版本检查硬编码成纯 PostgreSQL 版本门槛。

### 风险 3：结构同步需求与现有能力认知不一致

策略：

- 计划、文档、测试三处统一声明对象边界。
- 先交付现有能力等价覆盖，再进入下一期对象扩展。

### 风险 4：SHA256 认证影响主路径节奏

策略：

- 将 SHA256 从 MVP 验收剥离。
- 主仓只预留封装点，不阻塞本期交付。
- 后续在 `apecloud/rust-postgres` 独立仓分支推进。

## 8. 验收口径

MVP 完成的定义如下：

- `DbType::GaussDBPg` 已在主仓被完整识别。
- `PG -> GaussDB` 的快照、结构、校验跑通。
- `GaussDB -> PG` 的快照、CDC、校验跑通。
- 自动化测试通过。
- 真实 GaussDB 环境的 4 组联调证据齐全。
- 文档足以支持另一个工程师复现联调。

## 9. 默认假设

- 当前已有可持续使用的 GaussDB HCS 25.1.30 环境。
- 当前已有或可快速抓取 `mppdb_decoding` 原始样本。
- 本期真实认证主路径为 `MD5`。
- 真实 GaussDB 联调作为人工门禁，不纳入公共 CI。
