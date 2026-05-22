# Progress Log

## Context Recovery Block

- **Task**: `GaussDBPg -> MySQL struct basic`
- **Shape**: `single-full`
- **Truth file**: `.codex-tasks/20260415-gaussdb-to-mysql-struct-basic/TODO.csv`
- **Current status**: `DONE`
- **Last completed**: `5 - 更新 tracker/e2e 矩阵并收口证据`
- **Key context**:
  - `PgStructExtractor` 产出的 struct statements 为 PG 风格；MySQL 目标端需要显式转换后再执行。
  - 当前实现只覆盖 `PgCreateSchema/PgCreateTable` 的最小转换，以便 bootstrap 级别用例可跑通。
- **Validation**:
  - `set -a; source dt-tests/tests/.env.local; set +a && cargo test -p dt-tests --test integration_test -- gaussdb_to_mysql::struct_tests::test::struct_basic_test --nocapture` ✅（MySQL8 expect 基线同步后 PASS）

## 验证记录

### 1) 真实环境运行（首次失败）

- 命令：
```bash
set -a; source dt-tests/tests/.env.local; set +a
cargo test -p dt-tests --test integration_test -- \
  gaussdb_to_mysql::struct_tests::test::struct_basic_test --nocapture
```
- 结果：FAIL
- 根因：MySQL 8.0 `SHOW CREATE DATABASE` 返回包含 `COLLATE utf8mb4_0900_ai_ci`，原 `expect_ddl_8.0.sql` 缺失该片段，导致 database DDL 做严格字符串对比失败。

### 2) 同步 MySQL8 expect 基线后重跑（PASS）

- 基线修复：
  - `dt-tests/tests/gaussdb_to_mysql/struct/basic_test/expect_ddl_8.0.sql`
  - 将 DB DDL 更新为包含 `DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_ai_ci`
- 命令（为减少 RW host 探测时间可临时收敛候选）：
```bash
set -a; source dt-tests/tests/.env.local; set +a
export gaussdb_pg_candidate_hosts=10.250.0.51:8000
cargo test -p dt-tests --test integration_test -- \
  gaussdb_to_mysql::struct_tests::test::struct_basic_test --nocapture
```
- 结果：PASS（期间出现少量 `Operation timed out` 触发 `run_with_retry` 自动重试，最终通过）
