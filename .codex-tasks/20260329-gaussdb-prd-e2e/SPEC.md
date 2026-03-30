# Task Specification

## Task Shape

- **Shape**: `single-full`

## Goals

- 对照 [docs/agent-summary/gaussdb-prd.md](/Users/lmj/projects/ai-project/my-projects/ape-dts/docs/agent-summary/gaussdb-prd.md) 评估当前仓库实现处于哪个 Phase（并明确缺口）。
- 使用 Boss 提供的 PostgreSQL / GaussDB 连接信息，执行一次真实 e2e（以 `dt-tests` GaussDB 相关集成测试为准）。

## Non-Goals

- 不新增功能开发，不做重构；如 e2e 失败，仅定位根因并记录（除非 Boss 继续下达开发指令）。
- 不实现 SHA256 认证（除非 Boss 明确要求）。
- 不将任何口令写入可被 git 跟踪的文件（仅允许写入已被 gitignore 忽略的 `dt-tests/tests/.env.local`）。

## Constraints

- 遵守 Debug-First：不引入“为了跑通”的静默降级/兜底逻辑；失败要暴露清楚。
- 归档证据时需确保日志/文件不包含明文口令。

## Acceptance

- `PROGRESS.md` 含 “PRD Phase Assessment” 小节，明确列出：已完成 Phase、部分完成 Phase、未开始 Phase 与关键缺口。
- e2e：以下 6 个测试均通过（或明确记录失败原因与证据）：
  - `pg_to_gaussdb::snapshot_tests::test::snapshot_basic_test`
  - `pg_to_gaussdb::struct_tests::test::struct_basic_test`
  - `pg_to_gaussdb::check_tests::test::check_basic_test`
  - `gaussdb_to_pg::snapshot_tests::test::snapshot_basic_test`
  - `gaussdb_to_pg::check_tests::test::check_basic_test`
  - `gaussdb_to_pg::cdc_tests::test::cdc_basic_test`

## Validation Commands

```bash
psql -h 127.0.0.1 -p 5432 -d postgres -U lmj --no-password -c "select 1"
PGPASSWORD=*** psql -h 10.250.0.30 -p 8000 -U root -d postgres --no-password -c "select 1"

cargo test -p dt-tests --test integration_test -- pg_to_gaussdb::snapshot_tests::test::snapshot_basic_test --nocapture
cargo test -p dt-tests --test integration_test -- pg_to_gaussdb::struct_tests::test::struct_basic_test --nocapture
cargo test -p dt-tests --test integration_test -- pg_to_gaussdb::check_tests::test::check_basic_test --nocapture
cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::snapshot_tests::test::snapshot_basic_test --nocapture
cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::check_tests::test::check_basic_test --nocapture
cargo test -p dt-tests --test integration_test -- gaussdb_to_pg::cdc_tests::test::cdc_basic_test --nocapture
```

