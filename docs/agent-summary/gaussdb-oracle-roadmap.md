# GaussDB Oracle 兼容模式路线图（已解锁：远端 oracle-mode + 本机 Docker）

> 状态：`ACTIVE (non-CDC basic PASS)`

当前已在远端 oracle-mode 数据库验证通过 `PG -> GaussDBOracle` 的 **snapshot/struct/check basic**，
并保留本机 openGauss oracle-mode 作为快速回归替身环境。

## 1. 环境

- 远端 GaussDBOracle（oracle-mode DB）
  - 通过 `dt-tests/tests/.env.local` 覆盖：
    - `gaussdb_oracle_sinker_without_auth_url`
    - `gaussdb_oracle_sinker_username`
    - `gaussdb_oracle_sinker_password`
  - HA 候选主机：`gaussdb_pg_candidate_hosts`（自动选 RW 主）
- 本机 GaussDBOracle 替身（openGauss `sql_compatibility=A`）
  - `docker compose -f dt-tests/docker-compose.gaussdb_oracle.yml up -d`
  - `SHOW sql_compatibility;` 必须为 `A`
  - 端口：`127.0.0.1:55432`
- 本机 Oracle XE 11g（仅环境，供后续 Oracle connector 联调）
  - `docker compose -f dt-tests/docker-compose.oracle_xe.yml up -d`
  - 端口：`15211`（listener），`18080`（HTTP）

## 2. 当前已交付（non-CDC basic）

- 代码：`DbType::GaussDBOracle`（`gaussdb_oracle`）已补齐 non-CDC 主链路所需的 DDL/struct-check 兼容
- 自动化（dt-tests）：
  - snapshot: `pg_to_gaussdb_oracle::snapshot_tests::test::smoke_test`
  - struct: `pg_to_gaussdb_oracle::struct_tests::test::struct_basic_test`
  - check: `pg_to_gaussdb_oracle::check_tests::test::check_basic_test`
- 证据：
  - `.codex-tasks/20260415-gaussdb-oracle-next/PROGRESS.md`
  - `.codex-tasks/20260415-gaussdb-oracle-bootstrap/PROGRESS.md`（本机 docker 替身）

## 3. 下一步建议（按优先级）

1. `PG -> GaussDBOracle` 补齐 `precheck basic`（struct_supported）
2. 扩展 struct 覆盖（view/routine 等）与 Oracle-mode 差异用例
3. 未来如需：单开 Epic 做 `Oracle -> GaussDBOracle`（依赖 Oracle connector；本轮已提供 Oracle XE 环境）

## 4. 明确不做（当前阶段）

- 不实现 Oracle wire-protocol/OCI 级别连接器（仅提供 Oracle XE 环境）
- 不承诺 `GaussDBOracle` 的 CDC/resume/failover（后续单开 Epic）
