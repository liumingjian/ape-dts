# Progress Log

## Context Recovery Block

- **Task**: `DbType::GaussDBMySQL 骨架 + 路由 + smoke`
- **Shape**: `single-full`
- **Truth file**: `.codex-tasks/20260402-gaussdb-mysql-bootstrap/tasks/20260402-02-gaussdb-mysql-skeleton-smoke/TODO.csv`

## 2026-04-02

- Added `DbType::GaussDBMySQL`.
- Routed `gaussdb_mysql` through MySQL-compatible config/task/precheck code paths.
- Added `dt-tests/tests/mysql_to_gaussdb_mysql/` with a snapshot smoke test scaffold.
- Started local MySQL 8 source container:
  - container: `ape-dts-mysql8`
  - port: `3311`
  - validated: `VERSION()=8.0.44`, `binlog_format=ROW`, `binlog_row_image=FULL`, `gtid_mode=ON`
  - evidence: `raw/mysql8-local-setup.txt`
- Confirmed a critical environment fact by live probe:
  - URL: `postgres://root@10.250.0.51:8000/jyp_test_m?sslmode=require`
  - `current_database()` = `jyp_test_m`
  - `SHOW sql_compatibility;` = `M`
  - implication: GaussDB compatibility mode is database-level and may still use a `postgres://` endpoint
  - evidence: `raw/jyp_test_m_probe.txt`
- Updated `docs/templates/mysql_to_gaussdb_mysql.md` with:
  - Docker setup commands for local MySQL 8
  - a copy-ready `dt-tests/tests/.env.local` snippet
  - an explicit `KEY=value` / no-`export` reminder
- Current validation plan:
  - `cargo test -p dt-common test_db_type_gaussdb_mysql_parse_and_display -- --nocapture` PASS
  - `cargo test -p dt-common -p dt-task -p dt-precheck --no-run` PASS
  - `cargo test -p dt-tests --test integration_test --no-run` PASS
  - local MySQL 8 source contract is ready
  - real smoke is blocked by a larger design issue:
    - the code currently assumes `DbType::GaussDBMySQL` uses MySQL-compatible connector plumbing
    - the real HCS environment shows “MySQL-compatible database” over a `postgres://` endpoint
    - next step must separate connection protocol from SQL compatibility mode before env variables alone are sufficient
- Resolution:
  - keep the compile/no-run artifacts from this child
  - mark the original child as failed rather than silently mutating its premise
  - continue delivery via `tasks/20260402-06-protocol-mode-realignment/`
