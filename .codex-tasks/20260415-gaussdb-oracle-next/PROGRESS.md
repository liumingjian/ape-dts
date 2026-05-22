# Progress Log

## Session Start

- **Date**: 2026-04-15 +0800
- **Task name**: `20260415-gaussdb-oracle-next`
- **Task dir**: `.codex-tasks/20260415-gaussdb-oracle-next/`
- **Spec**: `SPEC.md`
- **Plan**: `TODO.csv`

## Context

用户提供远端 `GaussDB Oracle compatibility mode` 的 `testdb` 供测试，并给出 HA 候选：

- `gaussdb_pg_candidate_hosts=10.250.0.52:8000,10.250.0.30:8000,10.250.0.51:8000`

凭据通过本地 `dt-tests/tests/.env.local` 注入（gitignored），不写入仓库文件。

## 2026-04-15

### 1) dt-tests：DbType::GaussDBOracle 纳入 RW 选主与执行 SQL

- 修复点：`dt-tests/tests/test_runner/rdb_test_runner.rs`
  - `DbType::GaussDBOracle` 视作 pg-wire gaussdb，复用 `gaussdb_pg_candidate_hosts` 的 RW URL 解析逻辑

### 2) struct：Oracle 模式 DDL + catalog 差异适配

- DDL 兼容：
  - `CREATE SCHEMA IF NOT EXISTS` 在 Oracle mode 下报语法错，改为 `DO $$ ... $$` 捕获 `duplicate_schema`
  - `CREATE INDEX` 对 “约束隐式索引已存在” 场景做幂等包装（`duplicate_table`）
- struct check：
  - `PgStructCheckFetcher` 对 `GaussDBOracle` 复用 `GaussDBPg` 的降级 catalog 查询（缺失列：`relrowsecurity/attgenerated/...`）
  - `RdbStructTestRunner` 将 `GaussDBOracle` 纳入 cross-engine normalization（ubtree/btree、summary keys 子集）

### 3) dt-tests：新增 pg_to_gaussdb_oracle struct/check basic

- 新增用例：
  - `pg_to_gaussdb_oracle::struct_tests::test::struct_basic_test`
  - `pg_to_gaussdb_oracle::check_tests::test::check_basic_test`

### 4) 远端验证（PASS）

验证命令（均 PASS）：

```bash
cargo test -p dt-tests --test integration_test -- pg_to_gaussdb_oracle::snapshot_tests::test::smoke_test --nocapture
cargo test -p dt-tests --test integration_test -- pg_to_gaussdb_oracle::struct_tests::test::struct_basic_test --nocapture
cargo test -p dt-tests --test integration_test -- pg_to_gaussdb_oracle::check_tests::test::check_basic_test --nocapture
```

### 5) Oracle XE 本机环境（Docker）

- 新增 compose：`dt-tests/docker-compose.oracle_xe.yml`
- 启动并验证健康：

```bash
docker compose -f dt-tests/docker-compose.oracle_xe.yml up -d
docker ps --filter name=oracle-xe-local --format '{{.Status}}'
```

> 端口映射：`15211->1521`（Oracle listener），`18080->8080`（APEX/HTTP）。

## Next

- 已更新文档真相源（入口可检索）：
  - `docs/agent-summary/gaussdb-progress-tracker.md`
  - `docs/agent-summary/gaussdb-e2e-test-plan.md`
  - `docs/agent-summary/gaussdb-oracle-roadmap.md`
