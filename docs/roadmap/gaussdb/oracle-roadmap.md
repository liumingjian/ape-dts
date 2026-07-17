# GaussDB Oracle 兼容模式路线图（已解锁：远端 oracle-mode + 本机 Docker）

> 状态：`ACTIVE (CDC basic PASS)`

当前已在远端 oracle-mode 数据库验证通过 `PG -> GaussDBOracle` 与 `GaussDBOracle -> PG` 的 **snapshot/struct/check/precheck/cdc basic**，
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
  - dt-tests 默认通过 `ORACLE_SQLPLUS_DOCKER_CONTAINER=oracle-xe-local` 以 `docker exec` 调用容器内 `sqlplus`
  - 端口：`15211`（listener），`18080`（HTTP）

## 2. 当前已交付（sync basic）

- 代码：`DbType::GaussDBOracle`（`gaussdb_oracle`）已补齐 sync basic 主链路（snapshot/struct/check/precheck/cdc）
- 自动化（dt-tests）：
  - snapshot: `pg_to_gaussdb_oracle::snapshot_tests::test::smoke_test`
  - snapshot: `gaussdb_oracle_to_pg::snapshot_tests::test::smoke_test`
  - struct: `pg_to_gaussdb_oracle::struct_tests::test::struct_basic_test`
  - struct: `gaussdb_oracle_to_pg::struct_tests::test::struct_basic_test`
  - check: `pg_to_gaussdb_oracle::check_tests::test::check_basic_test`
  - check: `gaussdb_oracle_to_pg::check_tests::test::check_basic_test`
  - precheck: `pg_to_gaussdb_oracle::precheck_tests::test::struct_supported_basic_test`
  - precheck: `gaussdb_oracle_to_pg::precheck_tests::test::struct_supported_basic_test`
  - cdc: `pg_to_gaussdb_oracle::cdc_tests::test::cdc_basic_test`
  - cdc: `gaussdb_oracle_to_pg::cdc_tests::test::cdc_basic_test`
- 证据：
  - 历史本地任务/证据（未纳入仓库）
  - 历史本地任务/证据（未纳入仓库）（本机 docker 替身）
  - 历史本地任务/证据（未纳入仓库）（PG ↔ GaussDBOracle sync basic）

## 2.1 额外交付：Oracle ↔ GaussDBOracle（bootstrap）

- 代码：
  - 新增 `DbType::Oracle` + `OracleSqlPlusClient` + `OracleSnapshotExtractor` + `OracleSinker`（bootstrap：snapshot + INSERT）
  - 新增 `OracleStructExtractor`（bootstrap：struct basic，输出 PgCreateSchema/PgCreateTable）
  - 新增 `OracleLogMinerCdcExtractor`（bootstrap：logminer，支持 DML insert/update/delete，用于 `Oracle -> GaussDBOracle`）
  - 兼容保留：`OracleCdcExtractor`（bootstrap：trigger-based）
- 自动化（dt-tests）：
  - snapshot: `oracle_to_gaussdb_oracle::snapshot_tests::test::smoke_test`
  - struct: `oracle_to_gaussdb_oracle::struct_tests::test::struct_basic_test`
  - check: `oracle_to_gaussdb_oracle::check_tests::test::check_basic_test`
  - cdc: `oracle_to_gaussdb_oracle::cdc_tests::test::cdc_basic_test`
  - snapshot: `gaussdb_oracle_to_oracle::snapshot_tests::test::smoke_test`
  - cdc: `gaussdb_oracle_to_oracle::cdc_tests::test::cdc_basic_test`
  - script: `bash scripts/e2e/oracle_gaussdboracle_bootstrap.sh`
- 证据：
  - 历史本地任务/证据（未纳入仓库）
  - 历史本地任务/证据（未纳入仓库）
  - 历史本地任务/证据（未纳入仓库）
  - 历史本地任务/证据（未纳入仓库）
  - 历史本地任务/证据（未纳入仓库）
  - 历史本地任务/证据（未纳入仓库）

## 3. 下一步建议（按优先级）

1. 扩展 struct 覆盖（view/routine 等）与 Oracle-mode 差异用例
2. 扩展 precheck（负例/边界用例），避免误配上线
3. `Oracle <-> GaussDBOracle` 扩展 snapshot 类型覆盖，并评估 CDC（LogMiner/OGG）与防环（DataMarker 拓扑）策略

## 4. 明确不做（当前阶段）

- 不实现 Oracle wire-protocol/OCI/JDBC 级别连接器（bootstrap 走 `sqlplus` CLI）
- 不承诺 `GaussDBOracle` 的 CDC resume/failover/DDL-CDC（后续按需要单开 Epic）
