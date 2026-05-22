# Progress Log

---

## Session Start

- **Date**: 2026-03-16
- **Task name**: `20260316-01-base-compat`
- **Task dir**: `.codex-tasks/20260316-gaussdb-mvp/tasks/20260316-01-base-compat/`
- **Spec**: `SPEC.md`
- **Plan**: `TODO.csv`
- **Environment**: Rust workspace / Cargo

---

## Context Recovery Block

- **Current milestone**: DONE
- **Current status**: DONE
- **Last completed**: #4 — 子任务验收回归
- **Current artifact**: `TODO.csv`
- **Key context**: 已完成 GaussDBPg 基础路由 + Precheck 接入：GaussDBPg 走 PG 预检查骨架（版本门槛不硬编码 PG）、系统 schema 过滤使用 GAUSSDB_PG 列表，并增加 mppdb_decoding 可用性检查。
- **Known issues**: N/A（本机需安装 `cmake` 才能通过 rdkafka-sys 构建）
- **Next action**: 回写 Epic `SUBTASKS.csv` 子任务 1 状态为 DONE，开始子任务 2。

---

## Milestone 1: 新增 DbType::GaussDBPg

- **Status**: DONE
- **Started**: 20:00
- **Completed**: 20:02
- **What was done**:
  - 新增 `DbType::GaussDBPg`（解析值：`gaussdb_pg`）
  - 新增最小单测验证解析与 Display 输出
  - `task_config` 中补齐 extractor/sinker match 以通过编译
- **Validation**: `docker run --rm -v "$PWD":/app -w /app rust:1.85.0-bullseye cargo test -p dt-common` → exit 0
- **Files changed**:
  - `dt-common/src/config/config_enums.rs` — 增加 `GaussDBPg` + 单测
  - `dt-common/src/config/task_config.rs` — 增加 `GaussDBPg` 分支（非 CDC 复用 Pg）
- **Next step**: Milestone 2: 补齐基础路由与公共工具分支

---

## Milestone 2: 补齐基础路由与公共工具分支

- **Status**: DONE
- **Started**: 20:02
- **Completed**: 20:12
- **What was done**:
  - `task_config`：新增 `DbType::GaussDBPg` extractor/sinker 路由（非 CDC 复用 Pg）
  - `SystemDb/SqlUtil`：GaussDBPg 继承 Pg 的转义与 token 规则，并补充保守的系统 schema 列表
  - `TaskUtil`：schema/table 列举与估算支持 GaussDBPg
  - `ConstraintType`、`DdlParser`：将 GaussDBPg 视为 Pg 兼容分支
  - `ResumerUtil`、`DataMarker`、`RdbQueryBuilder`：补齐 GaussDBPg 兼容分支
- **Validation**: `docker run --rm -v "$PWD":/app -w /app -v "$PWD/.cargo-cache/registry":/usr/local/cargo/registry -v "$PWD/.cargo-cache/git":/usr/local/cargo/git ape-dts-rust-dev:1.85.0 cargo test -p dt-common -p dt-task` → exit 0
- **Files changed**:
  - `dt-common/src/system_dbs.rs` — 增加 `GAUSSDB_PG` 系统 schema 列表
  - `dt-common/src/utils/sql_util.rs` — GaussDBPg 复用 Pg 转义规则
  - `dt-common/src/config/task_config.rs` — GaussDBPg extractor/sinker 路由
  - `dt-common/src/meta/ddl_meta/ddl_parser.rs` — GaussDBPg 复用 Pg DDL 解析路径
  - `dt-common/src/meta/struct_meta/structure/constraint.rs` — GaussDBPg 复用 Pg 约束类型映射
  - `dt-task/src/task_util.rs` — GaussDBPg schema/table 路由
  - `dt-task/src/extractor_util.rs` — extractor meta_manager 支持 GaussDBPg
  - `dt-task/src/task_runner.rs` — heartbeat/data_marker 建表使用配置的 db_type
  - `dt-connector/src/rdb_query_builder.rs` — GaussDBPg 视为 Pg upsert
  - `dt-connector/src/data_marker.rs` — GaussDBPg 走 RDB marker 解析
  - `dt-connector/src/extractor/resumer/utils.rs` — GaussDBPg 复用 Pg pool
- **Next step**: Milestone 3: Precheck 支持 GaussDBPg

---

## Milestone 3: Precheck 支持 GaussDBPg

- **Status**: DONE
- **Completed**: 08:30
- **What was done**:
  - `PrecheckerBuilder`：新增 `DbType::GaussDBPg` → 复用 `PostgresqlPrechecker` 骨架
  - `PostgresqlPrechecker`：新增 `db_type` 字段，避免硬编码 `DbType::Pg`
  - 版本检查：PG 保持最小版本门槛；GaussDBPg 不硬编码 PG 门槛，并在 warn 中输出 `server_version_num`
  - CDC 检查：GaussDBPg 增加 `mppdb_decoding` 可用性检查（`pg_available_extensions`）
  - `PgFetcher`：系统 schema 过滤改为使用 `SystemDb::get_system_dbs(db_type)`（覆盖 `cstore/db4ai/dbe_perf/...`）
  - `CheckResult`：补齐 GaussDBPg 的 CDC/版本建议信息
- **Validation**:
  - `cargo test -p dt-precheck` → exit 0（首次失败因缺少 `cmake`，已通过 `brew install cmake` 解决）
- **Files changed**:
  - `dt-precheck/src/builder/prechecker_builder.rs`
  - `dt-precheck/src/prechecker/pg_prechecker.rs`
  - `dt-precheck/src/fetcher/postgresql/pg_fetcher.rs`
  - `dt-precheck/src/meta/check_result.rs`
- **Next step**: Milestone 4: 子任务验收回归

---

## Milestone 4: 子任务验收回归

- **Status**: DONE
- **Completed**: 08:30
- **Validation**: `cargo test -p dt-common -p dt-task -p dt-precheck` → exit 0
