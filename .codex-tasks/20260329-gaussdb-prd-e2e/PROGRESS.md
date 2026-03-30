# Progress Log

> Taskmaster single-full. Decision log + context recovery anchor.

---

## Context Recovery Block

- **Task**: 对照 PRD 评估 GaussDB 实现阶段 + 跑一次真实 e2e（dt-tests）
- **Shape**: `single-full`
- **Progress**: 6/6（#5 DONE）
- **Current**: 完成（GaussDB->PG snapshot/check/cdc 均已通过）
- **Artifact**: `.codex-tasks/20260329-gaussdb-prd-e2e/TODO.csv`
- **Next action**: 无

---

## PRD Phase Assessment

### Phase 1: 基础框架 + GaussDB Sinker

- **状态**: 已完成（限定为 `DbType::GaussDBPg` / MD5 主路径）。
- **证据**:
  - `DbType::GaussDBPg` 已实现：`dt-common/src/config/config_enums.rs`
  - GaussDBPg 非 CDC 场景复用 PG 路由（snapshot/struct/check/write）：`dt-common/src/config/task_config.rs`、`dt-task/src/*_util.rs`
  - `dt-tests` 存在 `pg_to_gaussdb` 的 `snapshot/struct/check` 基础用例骨架
- **PRD 差距**:
  - PRD 提到的 `DbType::GaussDBMySQL` / `DbType::GaussDBOracle` 未实现（PRD 完整版范围，且不在已完成 MVP 的非目标内）。

### Phase 2: GaussDB CDC Extractor（mppdb_decoding / JSON）

- **状态**: 已完成（GaussDBPg -> PG CDC 主路径已落地）。
- **证据**:
  - 已存在 gaussdb extractor 模块：`dt-connector/src/extractor/gaussdb/*`
  - Extractor 工厂已注册：`dt-task/src/extractor_util.rs`
  - `dt-tests` 存在 `gaussdb_to_pg/cdc/basic_test`

### Phase 3: 数据校验 + 对象同步完善 + 预检查增强 + GaussDB 特有类型

- **状态**: 部分完成（以“真实 L4 联调闭环”为准，已覆盖 PRD P0 的 snapshot/check/struct 基础链路，但未覆盖 PRD 中更宽的对象同步面）。
- **已覆盖点（偏 P0）**:
  - 双向 `check basic` 用例已存在并已在历史 L4 验证任务中跑通（见 `.codex-tasks/20260317-gaussdb-l4-validation`）。
  - 结构同步/校验口径已对 GaussDBPg 做“逻辑结构等价”适配，避免 catalog 物理差异导致误报。
- **未覆盖点（PRD 要求但当前仓库未见落地或测试覆盖）**:
  - 对象同步范围：当前结构模型只覆盖 `database/table/constraint/sequence/comment/index/rbac`，未覆盖 view/function/procedure/trigger 等。
  - SHA256 认证（PRD 单列为必须支持，当前仅有后续路线文档，未实现）。
  - 更系统的 GaussDB 特有类型覆盖（目前以联调中遇到的问题点修复为主，而非按 PRD 表格完整铺开）。

### Phase 4: 完整版扩展（MySQL/Oracle 兼容模式等）

- **状态**: 未开始（符合既有 MVP 的 Non-Goals）。

---

## Milestone 2: 验证 PG/GaussDB 连接可用

- **Status**: DONE（2026-03-29 17:26 +0800 recheck OK）
- **PostgreSQL (local)**:
  - `psql -h 127.0.0.1 -p 5432 -d postgres -U lmj --no-password -c "select 1"` → success
- **GaussDB (remote)**:
  - `psql -h 10.250.0.30 -p 8000 -U root -d postgres -c "select 1"` → success
- **Note**: 2026-03-29 16:xx 前后曾出现 `server closed the connection unexpectedly` / `pg_isready no response`（疑似链路抖动或实例状态变化）；后续已恢复，但 CDC 用例仍表现出主备/连接不稳定症状。

---

## Milestone 4: e2e（PG -> GaussDB）

- ✅ `pg_to_gaussdb::snapshot_basic_test` OK（`raw/20260329_pg_to_gaussdb_snapshot_basic_test_run3.log`）
- ✅ `pg_to_gaussdb::struct_basic_test` OK（`raw/20260329_pg_to_gaussdb_struct_basic_test_run3.log`）
- ✅ `pg_to_gaussdb::check_basic_test` OK（`raw/20260329_pg_to_gaussdb_check_basic_test_run2.log`）

---

## Milestone 5: e2e（GaussDB -> PG）

- ✅ `gaussdb_to_pg::snapshot_basic_test` OK（`raw/20260330_gaussdb_to_pg_snapshot_basic_test_run2.log`）
- ✅ `gaussdb_to_pg::check_basic_test` OK（`raw/20260330_gaussdb_to_pg_check_basic_test_run2.log`）
- ✅ `gaussdb_to_pg::cdc_basic_test` OK（`raw/20260330_gaussdb_to_pg_cdc_basic_test_run10.log`）
- History: 2026-03-29 `cdc_basic_test` 曾失败（`raw/20260329_gaussdb_to_pg_cdc_basic_test_run3.log`，表现为 update 未同步 + slot active/连接抖动连锁）

---

## 2026-03-30 Follow-up（本机 PG Docker + CDC 稳定性改进）

- **本机 PG 环境**: 启动 `postgres:15` Docker 容器（host `127.0.0.1:5434`，与 `dt-tests/tests/.env` 默认 `pg_sinker_without_auth_url` 对齐）。
- **最终结果**:
  - ✅ `gaussdb_to_pg::cdc_basic_test` 已跑通（`raw/20260330_gaussdb_to_pg_cdc_basic_test_run10.log`）。
- **已做的稳定性改动（代码层）**:
  - `dt-task`: 为 `TaskRunner` 增加 abort/drop 保护，避免测试重试时仅 abort 外层 JoinHandle 导致 extractor/pipeline/monitor 内部任务泄漏，从而引发 slot 长时间 active。
  - `dt-tests`: GaussDB RW endpoint 选择更严格（`transaction_read_only` + 事务内 DDL/DML probe + timeout）；CDC attempt 内避免“每次执行 SQL 重新 resolve”导致的 endpoint 漂移/脏状态；CDC DML 改为尽快执行 insert/update/delete，最后做一次最终 compare（降低 stage 间主备切换窗口）。
  - `dt-connector`: 清理 GaussDB CDC extractor 中无效的 `should_reconnect` 赋值告警（不改变逻辑语义）。
