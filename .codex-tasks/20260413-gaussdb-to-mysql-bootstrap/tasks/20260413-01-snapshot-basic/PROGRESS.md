# Progress Log

## Context Recovery Block

- **Task**: `GaussDB→MySQL snapshot basic`
- **Shape**: `single-full`
- **Truth file**: `.codex-tasks/20260413-gaussdb-to-mysql-bootstrap/tasks/20260413-01-snapshot-basic/TODO.csv`
- **Current milestone**: `complete`
- **Current status**: `DONE`
- **Last completed**: `4 - 回写 child 与 parent 进展`
- **Current artifact**: `validated snapshot fixture and tracker updates`
- **Key context**:
  - `gaussdb_to_mysql::snapshot_tests::test::snapshot_basic_test` 已具备自动化入口并通过真实环境验证。
  - 目标端通过 `db_map=public:gaussdb_to_mysql_snapshot_dst` 将源 schema 映射到 MySQL database。
  - source cleanup 被显式跳过，因为 `src_prepare.sql` 已经在每次运行前重建源表，能保持重跑幂等。
- **Known issues**:
  - 共享 GaussDB RW 节点探测仍可能出现 `unexpected EOF` / `Connection reset by peer`，但现有 retry 已能恢复，本 child 不再有代码级 blocker。
  - 真实环境验证依赖本地 MySQL sink 在 `127.0.0.1:3308` 可用。
- **Next action**: 以同一 source/destination 契约开启 `GaussDB→MySQL check basic` child，并优先复用当前 fixture 命名与路由约定。

## 2026-04-13

- Child opened under `20260413-gaussdb-to-mysql-bootstrap`.
- Initial execution plan:
  - reuse `gaussdb_to_pg/snapshot/basic_test` as source fixture baseline
  - reuse `mysql_to_mysql` sink contract for MySQL target setup
  - prefer the smallest one-table fixture to expose routing/runtime issues early
- Added `dt-tests/tests/gaussdb_to_mysql/` snapshot module plus the minimal `basic_test` fixture set.
- Confirmed the reverse-path contract with live validation:
  - `cargo test -p dt-tests --test integration_test --no-run`
  - `cargo test -p dt-tests --test integration_test -- gaussdb_to_mysql::snapshot_tests::test::snapshot_basic_test --nocapture`
- Live run proved end-to-end snapshot success and row equality for `public.gaussdb_to_mysql_snapshot_basic` -> `gaussdb_to_mysql_snapshot_dst.gaussdb_to_mysql_snapshot_basic`.
- Removed temporary debug-only tests after isolating the earlier failure source.
- Resolved the cleanup flake by making source cleanup a no-op and relying on idempotent source prepare instead.
